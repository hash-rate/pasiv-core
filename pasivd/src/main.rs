// SPDX-License-Identifier: GPL-3.0-only
//! pasivd — the headless Pasiv node.
//!
//! One job: turn a server/lab box into a rig in your Pasiv fleet with two
//! commands and zero UI. A daemon can't do SIWE, so it pairs like a TV app:
//!
//!   pasivd claim   → prints a 6-char code; approve it in the Pasiv companion
//!   pasivd run     → mines XMR (CPU) to YOUR payout, publishes state to the
//!                    fleet, obeys start/stop from the phone
//!
//! Trust model mirrors the desktop (docs/FEES.md — the never-list):
//!   - non-custodial: mines straight to the owner's payout address
//!   - fee parity: the same time-sliced 4% (20 s per 500 s of Mining), to the
//!     same compile-time fee address, via the same xmrig config hot-reload
//!   - remote actions are start/stop only (the desktop additionally accepts
//!     signed updates; see docs/FEES.md never-list item 8)
//!   - the miner binary is fetched from xmrig's official release and
//!     sha256-verified against a compile-time pin before first run

use std::path::PathBuf;
use std::time::Duration;

use pasiv_core::address::is_valid_xmr_address;
use pasiv_core::fee::{self, PayoutSide, SliceScheduler, SwapFailure, FEE_ADDRESS_XMR};
use serde::{Deserialize, Serialize};
mod doctor;
mod xmrig;
use doctor::cmd_doctor;
use xmrig::{ensure_xmrig, spawn_xmrig, xmrig_current_user, xmrig_set_user, xmrig_summary, Miner};

/// Write the device config. It holds the device secret — a bearer credential
/// for this node's cloud identity — so it must never be world-readable, which
/// is what `std::fs::write` produces under a default umask.
fn write_config(path: &std::path::Path, cfg: &DeviceConfig) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
        // Not needless: the cfg(not(unix)) tail below is stripped on unix,
        // so this return is what ends the unix body.
        #[allow(clippy::needless_return)]
        return Ok(());
    }
    #[cfg(not(unix))]
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

// The Pasiv cloud + pool are DEFAULTS, overridable by environment so a fork —
// or an auditor — can point the daemon anywhere without patching source:
//   PASIVD_API_URL   the device API endpoint
//   PASIVD_ANON_KEY  the publishable key for it (RLS/edge auth do the enforcing)
//   PASIVD_POOL      the stratum host:port
const DEFAULT_FN_URL: &str = "https://vmmiuftvngxgwimwlrke.supabase.co/functions/v1/pasivd";
// Publishable key — same one the apps ship; safe to publish, useless without RLS consent.
const DEFAULT_ANON_KEY: &str = "sb_publishable_lp01D57d8gnuW49kelunDg_6c_ld5Lb";
const DEFAULT_POOL: &str = "gulf.moneroocean.stream:10128";

pub(crate) fn fn_url() -> String {
    std::env::var("PASIVD_API_URL").unwrap_or_else(|_| DEFAULT_FN_URL.into())
}
pub(crate) fn anon_key() -> String {
    std::env::var("PASIVD_ANON_KEY").unwrap_or_else(|_| DEFAULT_ANON_KEY.into())
}
pub(crate) fn pool() -> String {
    std::env::var("PASIVD_POOL").unwrap_or_else(|_| DEFAULT_POOL.into())
}
pub(crate) const XMRIG_URL: &str =
    "https://github.com/xmrig/xmrig/releases/download/v6.26.0/xmrig-6.26.0-linux-static-x64.tar.gz";
/// sha256 of the release TARBALL, checked before it is even decompressed.
pub(crate) const XMRIG_SHA256: &str =
    "fc6f8ae5f64e4f17481f7e3be29a1c56949f216a998414188003eae1db20c9e5";
/// sha256 of the EXTRACTED binary, re-checked on every start so a cached file
/// can never drift from what we pinned (and so a version bump actually lands).
pub(crate) const XMRIG_BIN_SHA256: &str =
    "b20f39fc00d242e706b6c30367ad811c676e0575050a4ec2f30104b696944b49";
pub(crate) const XMRIG_DIR_IN_TAR: &str = "xmrig-6.26.0";
pub(crate) const HTTP_PORT: u16 = 42999;

/// Live XMR network stats, the same source the desktop's profit ranking
/// uses for Monero. Public, key-free.
const XMR_STATS_URL: &str = "https://monero.herominers.com/api/stats";

// Fee parity with the desktop is BY CONSTRUCTION now: the address, the slice
// schedule, the validator, and — since Phase B — the entire enforcement state
// machine (`fee::SliceScheduler`) come from the shared pasiv-core crate. The
// desktop supervisor drives the same scheduler.

pub(crate) const VERSION: &str = concat!("pasivd ", env!("CARGO_PKG_VERSION"));

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct DeviceConfig {
    device_id: String,
    secret: String,
    #[serde(default)]
    payout_xmr: Option<String>,
}

pub(crate) fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("PASIVD_CONFIG") {
        return PathBuf::from(p);
    }
    let etc = PathBuf::from("/etc/pasivd.json");
    if etc.exists()
        || std::fs::write("/etc/.pasivd-probe", b"")
            .map(|_| {
                let _ = std::fs::remove_file("/etc/.pasivd-probe");
            })
            .is_ok()
    {
        return etc;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/pasivd/config.json")
}

pub(crate) fn data_dir() -> PathBuf {
    if std::fs::create_dir_all("/var/lib/pasivd").is_ok() {
        return PathBuf::from("/var/lib/pasivd");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let d = PathBuf::from(home).join(".local/share/pasivd");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "pasivd-node".into())
}

pub(crate) async fn api(
    client: &reqwest::Client,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = client
        .post(fn_url())
        .header("Authorization", format!("Bearer {}", anon_key()))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{status}: {v}"));
    }
    Ok(v)
}

// ---------------------------------------------------------------- claim ----

async fn cmd_claim() -> Result<(), String> {
    let client = reqwest::Client::new();
    let started = api(
        &client,
        serde_json::json!({
            "action": "claim_start",
            "name": hostname(),
            "platform": "linux",
        }),
    )
    .await?;
    let device_id = started["device_id"]
        .as_str()
        .ok_or("no device_id")?
        .to_string();
    let secret = started["secret"].as_str().ok_or("no secret")?.to_string();
    let code = started["code"].as_str().ok_or("no code")?.to_string();

    println!();
    println!("  In the Pasiv companion app: tap +  →  enter this code:");
    println!();
    println!("      ┌──────────────┐");
    println!("      │   {code}     │");
    println!("      └──────────────┘");
    println!();
    println!("  Waiting for approval (15 minutes)…");

    for _ in 0..300 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let v = api(
            &client,
            serde_json::json!({"action":"poll","device_id":device_id,"secret":secret}),
        )
        .await?;
        if v["status"] == "claimed" {
            let payout = v["payout_xmr"].as_str().map(|s| s.to_string());
            let cfg = DeviceConfig {
                device_id,
                secret,
                payout_xmr: payout.clone(),
            };
            let path = config_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            write_config(&path, &cfg)?;
            println!("  ✓ Claimed. Config saved to {}", path.display());
            if payout.is_none() {
                println!("  ⚠ No XMR payout on your account yet — set one in the Pasiv");
                println!("    desktop app (Coins → Monero) and it syncs automatically.");
            }
            println!("  Start mining:  sudo systemctl enable --now pasivd");
            return Ok(());
        }
        print!(".");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    Err("claim window expired — run `pasivd claim` again".into())
}

pub(crate) fn fee_ledger_path() -> PathBuf {
    // Overridable like PASIVD_CONFIG, so tests never append to a live node's
    // audit trail and operators can put it on a different volume.
    if let Ok(p) = std::env::var("PASIVD_FEE_LEDGER") {
        return PathBuf::from(p);
    }
    data_dir().join("fee-ledger.jsonl")
}

/// Append-only, one JSON object per line — auditable with any text editor.
///
/// The desktop has written this since the fee shipped; a headless node taking
/// the same 4% while keeping no record is the part that was missing. Fees are
/// only defensible if they are checkable, and "checkable" cannot mean "only on
/// machines with a GUI".
fn append_fee_event(ev: &fee::FeeEvent) {
    // Best-effort: a node must keep mining even if its disk is full or
    // read-only. Losing a ledger line is bad; halting the miner is worse.
    let _ = fee::append_event(&fee_ledger_path(), ev);
}

/// The scheduler is confirmed on a side (xmrig's login was read back, so the
/// record reflects where hashes really went); ledger the slice it closes.
fn confirm_side(sched: &mut SliceScheduler, side: PayoutSide, last_hashrate: f64) {
    if let Some(ev) = sched.confirmed(side, now_unix_secs(), last_hashrate) {
        let secs = ev.ended_at.saturating_sub(ev.started_at);
        append_fee_event(&ev);
        println!(
            "fee: {secs}s slice complete — logged to {}",
            fee_ledger_path().display()
        );
    }
}

/// Which address a payout side means. Pure so the fee path is testable: the
/// Fee side is exactly the shared crate's compile-time fee address; the User
/// side is the owner's payout.
fn side_address(side: PayoutSide, payout: &str) -> &str {
    match side {
        PayoutSide::Fee => FEE_ADDRESS_XMR,
        PayoutSide::User => payout,
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// -------------------------------------------------------------- earnings ----

/// USD/day earned per **kH/s** on XMR, from raw inputs — the shared crate's
/// `profit::score` verbatim; the $/day figure itself then comes from
/// `pasiv_core::earnings::usd_per_day`, the SAME function the desktop's
/// est_usd_day command uses. Parity is by construction, not by mirroring.
fn xmr_rate_per_kh(
    price_usd: f64,
    reward_atomic: f64,
    coin_units: f64,
    difficulty: f64,
) -> Option<f64> {
    pasiv_core::profit::score(price_usd, reward_atomic, coin_units, difficulty)
}

/// Fetch the current USD/day-per-H/s rate from the same public data the desktop
/// uses: CoinGecko for the price, HeroMiners for network difficulty and block
/// reward. Never throws — a headless node must keep mining even if the estimate
/// is briefly unavailable; the card just omits `≈ $/day` until the next refresh.
async fn fetch_xmr_rate_per_kh(client: &reqwest::Client) -> Option<f64> {
    let price = client
        .get("https://api.coingecko.com/api/v3/simple/price?ids=monero&vs_currencies=usd")
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?["monero"]["usd"]
        .as_f64()?;
    let v: serde_json::Value = client
        .get(XMR_STATS_URL)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let num = |x: &serde_json::Value| {
        x.as_f64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    };
    let difficulty = num(&v["network"]["difficulty"]).filter(|d| *d > 0.0)?;
    // averageReward smooths block variance; a pool that hasn't found a block
    // recently reports 0, so fall back to the last block's reward — the same
    // guard the desktop's fetch_network uses.
    let reward = num(&v["pool"]["averageReward"])
        .filter(|r| *r > 0.0)
        .or_else(|| num(&v["lastblock"]["reward"]))?;
    let units = num(&v["config"]["coinUnits"]).filter(|u| *u > 0.0)?;
    xmr_rate_per_kh(price, reward, units, difficulty)
}

/// The `snapshot` the companion renders from: the rollup state, a single
/// CPU→XMR lane (so the phone can draw "CPU XMR <rate>" by joining it with the
/// hashrate in `stats`), and est $/day when we're actually mining and a rate is
/// known. Pure and unit-tested: omitting the lane or the estimate is exactly
/// what showed a headless node as a bare "Mining" with no numbers (the 0.1.2
/// fix), and a fabricated est on an idle/zero-hashrate node would be a lie.
fn build_snapshot(state: &str, hashrate: f64, rate_per_kh: Option<f64>) -> serde_json::Value {
    let mut snapshot = serde_json::json!({
        "rollup": {"state": state},
        "miners": {"xmrig": {"state": state}},
    });
    if state == "mining" && hashrate > 0.0 {
        if let Some(est) = pasiv_core::earnings::usd_per_day(hashrate, rate_per_kh) {
            snapshot["est_usd_day"] = serde_json::json!(est);
        }
    }
    snapshot
}

// ------------------------------------------------------------------ run ----

async fn cmd_run() -> Result<(), String> {
    let path = config_path();
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("no config at {} — run `pasivd claim` first", path.display()))?;
    let mut cfg: DeviceConfig = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();

    // Payout must exist before the first hash — refresh from the account.
    loop {
        let v = api(
            &client,
            serde_json::json!({"action":"poll","device_id":cfg.device_id,"secret":cfg.secret}),
        )
        .await?;
        if v["status"] != "claimed" {
            return Err(format!("device not claimed (status: {})", v["status"]));
        }
        if let Some(p) = v["payout_xmr"].as_str().filter(|p| is_valid_xmr_address(p)) {
            cfg.payout_xmr = Some(p.to_string());
            let _ = write_config(&path, &cfg);
            break;
        }
        eprintln!("no XMR payout on the account yet — set one in the Pasiv app; retrying in 60s");
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
    let payout = cfg.payout_xmr.clone().unwrap();
    let bin = ensure_xmrig(&client).await?;

    let mut miner: Option<Miner> = None;
    let mut want_mining = true; // a headless node's default job is to mine
    let mut mining_secs: u64 = 0;
    // The shared enforcement state machine — fresh per spawn (a respawned
    // miner always comes up on the user's address).
    let mut sched = SliceScheduler::new();
    let mut tick: u64 = 0;
    let mut last_hashrate = 0.0_f64;
    let mut accepted: u64 = 0;
    let mut rejected: u64 = 0;

    // Detect the hardware ONCE — the CPU cannot change under a running process,
    // and detect() shells out to read the model — then send it with every push.
    // Without this a headless rig's `hardware` was always {}, so the companion
    // showed no CPU for it while every desktop rig showed one. It is the SAME
    // pasiv_core::hardware::detect() the desktop app serialises into its own rig
    // row, so the shape the companion parses is identical — capability, never
    // identity: core counts, usable threads and the CPU model, no serial, no id.
    // Null rather than a spurious {} if it somehow fails to serialise.
    let hardware = serde_json::to_value(pasiv_core::hardware::detect())
        .unwrap_or(serde_json::Value::Null);

    // Earnings estimate: a separate client because CoinGecko 403s reqwest's
    // default agent, and a cached rate refreshed ~every 10 min (the desktop
    // re-ranks on a similar cadence) — the per-tick hashrate is what varies,
    // not the network rate.
    let rate_client = reqwest::Client::builder()
        .user_agent(concat!("pasivd/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut rate_per_kh: Option<f64> = None;

    println!("{VERSION} — node {} → {}", hostname(), pool());

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        tick += 1;

        // Refresh the earnings rate once at startup and ~every 10 min after.
        // A failure keeps the last known rate (or none) and retries next cycle.
        if tick == 1 || tick.is_multiple_of(120) {
            if let Some(r) = fetch_xmr_rate_per_kh(&rate_client).await {
                rate_per_kh = Some(r);
            }
        }

        // Reconcile desired vs actual miner state.
        match (&mut miner, want_mining) {
            (None, true) => {
                let token: String = {
                    use rand::Rng;
                    let mut r = rand::thread_rng();
                    (0..32)
                        .map(|_| format!("{:x}", r.gen_range(0..16)))
                        .collect()
                };
                match spawn_xmrig(&bin, &payout, &token) {
                    Ok(child) => {
                        println!(
                            "miner started (payout {}…)",
                            payout.chars().take(12).collect::<String>()
                        );
                        sched = SliceScheduler::new();
                        miner = Some(Miner { child, token });
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
            (Some(m), false) => {
                let _ = m.child.kill().await;
                miner = None;
                last_hashrate = 0.0;
                println!("miner stopped");
            }
            (Some(m), true) => {
                // Crashed? respawn next tick.
                if let Ok(Some(_)) = m.child.try_wait() {
                    miner = None;
                    continue;
                }
            }
            (None, false) => {}
        }

        // Stats + fee slice while mining.
        if let Some(m) = &mut miner {
            if let Some(s) = xmrig_summary(&client, &m.token).await {
                last_hashrate = s["hashrate"]["total"][0].as_f64().unwrap_or(0.0);
                accepted = s["results"]["shares_good"].as_u64().unwrap_or(accepted);
                let total = s["results"]["shares_total"].as_u64().unwrap_or(0);
                rejected = total.saturating_sub(accepted);
            }
            // Mining time only accrues while actually hashing.
            if last_hashrate > 0.0 {
                mining_secs += 5;
            }

            // LEVEL-TRIGGERED reconcile, deliberately OUTSIDE the hashrate
            // guard. Two bugs lived in the old edge-triggered version:
            //
            //  1. It only acted when the desired state *changed*, trusting a
            //     local bool to describe reality. A PUT that 200s but doesn't
            //     apply — or anyone else driving the same local API — left us
            //     believing something false, permanently.
            //  2. Nesting it under `last_hashrate > 0.0` meant the swap-back
            //     could never run while hashrate read zero. The fee swap itself
            //     causes a pool re-login, which momentarily reports zero — so
            //     the one moment we most need to return to the user's address
            //     was the moment we stopped trying. A pool outage right then
            //     pinned the node on the FEE address indefinitely.
            //
            // Now:every tick, ask xmrig where it is actually mining and correct it.
            let want = sched.desired(last_hashrate > 0.0, mining_secs);
            let target = side_address(want, &payout);
            match xmrig_current_user(&client, &m.token).await {
                Some(actual) if actual == target => {
                    confirm_side(&mut sched, want, last_hashrate);
                }
                Some(_) | None => {
                    if xmrig_set_user(&client, &m.token, target).await {
                        confirm_side(&mut sched, want, last_hashrate);
                    } else if let SwapFailure::StopMining { attempts } = sched.swap_failed(want) {
                        // Failing to get BACK to the user's address is the only
                        // direction that can cost them money; the shared
                        // scheduler bounds it. A respawn always comes up on the
                        // user's address.
                        eprintln!(
                            "cannot return payout to your address after {attempts} \
                             attempts — stopping so mining never continues on the fee address"
                        );
                        let _ = m.child.kill().await;
                        miner = None;
                        last_hashrate = 0.0;
                    }
                }
            }
        }

        // Push every 30 s (or immediately after a command changed state).
        if !tick.is_multiple_of(6) {
            continue;
        }
        let state = if miner.is_some() {
            if last_hashrate > 0.0 {
                "mining"
            } else {
                "starting"
            }
        } else {
            "idle"
        };
        let snapshot = build_snapshot(state, last_hashrate, rate_per_kh);
        let push = api(
            &client,
            serde_json::json!({
                "action": "push",
                "device_id": cfg.device_id,
                "secret": cfg.secret,
                "name": hostname(),
                "platform": "linux",
                "app_version": VERSION,
                "hardware": hardware,
                "active_coin": "XMR",
                // No "payouts" — see remote/api.rs. It was never read, and
                // sending it contradicted the privacy policy. The edge
                // function and the rigs trigger both drop it now anyway.
                "snapshot": snapshot,
                "stats": {"xmrig": {"hashrate_avg": last_hashrate, "accepted": accepted, "rejected": rejected}},
                "session_mining_ms": mining_secs * 1000,
            }),
        )
        .await;
        let Ok(v) = push else {
            eprintln!("push failed: {}", push.unwrap_err());
            continue;
        };
        for cmd in v["commands"].as_array().cloned().unwrap_or_default() {
            let id = cmd["id"].as_str().unwrap_or("").to_string();
            let action = cmd["action"].as_str().unwrap_or("");
            println!("remote command: {action}");
            match action {
                "start" => want_mining = true,
                "stop" => want_mining = false,
                _ => {}
            }
            let _ = api(
                &client,
                serde_json::json!({
                    "action":"complete","device_id":cfg.device_id,"secret":cfg.secret,
                    "command_id": id, "ok": true, "result": format!("{action} ok"),
                }),
            )
            .await;
        }
    }
}

#[tokio::main]
async fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "run".into());
    let result = match arg.as_str() {
        "claim" => cmd_claim().await,
        "run" => cmd_run().await,
        "doctor" => cmd_doctor().await,
        "version" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        other => Err(format!(
            "unknown command: {other} (use: claim | run | doctor | version)"
        )),
    };
    if let Err(e) = result {
        eprintln!("pasivd: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pasiv_core::fee::{in_fee_slice, SLICE_SECS, SLICE_WINDOW_SECS};
    use pasiv_core::types::Coin;

    /// Env vars are process-global; every test that sets one — or reads a
    /// value derived from one, like `pool()` — takes this lock so parallel
    /// test threads can't interleave a set/remove with a read.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The whole point of the headless ledger is that it reads the same as the
    /// desktop's, so one script (or one person) can audit a mixed fleet. This
    /// line is copied from a real desktop ledger; the field set, order, and
    /// coin casing must all match.
    #[test]
    fn fee_event_line_matches_the_desktop_ledger_format() {
        let line = serde_json::to_string(&fee::FeeEvent {
            started_at: 1785797656,
            ended_at: 1785797676,
            coin: Coin::Xmr,
            address: FEE_ADDRESS_XMR.into(),
            est_hashes: 130651,
        })
        .unwrap();
        assert_eq!(
            line,
            format!(
                "{{\"started_at\":1785797656,\"ended_at\":1785797676,\"coin\":\"xmr\",\
                 \"address\":\"{FEE_ADDRESS_XMR}\",\"est_hashes\":130651}}"
            )
        );
    }

    /// The slice lifecycle itself is pinned in pasiv-core (the shared
    /// `SliceScheduler`); what pasivd owns is the LEDGER WRITE on the falling
    /// edge — one line per closed slice, in the shared format.
    #[test]
    fn a_closed_slice_writes_exactly_one_ledger_line() {
        // Never touch the real ledger: on a machine actually running pasivd,
        // `cargo test` would otherwise append junk to its audit trail.
        let _g = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("pasivd-test-fee-ledger.jsonl");
        let _ = std::fs::remove_file(&tmp);
        // SAFETY: env mutation serialised by ENV_LOCK.
        unsafe { std::env::set_var("PASIVD_FEE_LEDGER", &tmp) };

        let mut sched = SliceScheduler::new();
        confirm_side(&mut sched, PayoutSide::Fee, 100.0); // rising edge — no line
        confirm_side(&mut sched, PayoutSide::Fee, 100.0); // hold — no line
        confirm_side(&mut sched, PayoutSide::User, 100.0); // falling edge — one line
        confirm_side(&mut sched, PayoutSide::User, 100.0); // hold — nothing

        let written = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(written.lines().count(), 1, "one slice, one ledger line");
        assert!(written.contains("\"coin\":\"xmr\""));
        unsafe { std::env::remove_var("PASIVD_FEE_LEDGER") };
        let _ = std::fs::remove_file(&tmp);
    }

    /// Fee parity with the desktop: 20 s of every 500 s of mining.
    #[test]
    fn fee_slice_is_four_percent_of_mining_time() {
        let in_slice = (0..SLICE_WINDOW_SECS).filter(|s| in_fee_slice(*s)).count();
        assert_eq!(in_slice as u64, SLICE_SECS);
        assert_eq!(SLICE_SECS * 100 / SLICE_WINDOW_SECS, 4);
    }

    /// THE parity invariant that actually protects money: a headless node must
    /// take its 4% to the SAME address the desktop does. Both now read the one
    /// compile-time constant in the shared pasiv-core crate, so drift is
    /// impossible by construction; this test pins the BEHAVIOUR — inside a fee
    /// slice the login target is exactly that constant, outside it the user's
    /// payout, and the constant itself is a mineable address.
    #[test]
    fn fee_target_is_the_shared_crate_address_inside_a_slice() {
        assert_eq!(
            side_address(PayoutSide::Fee, "4user"),
            pasiv_core::fee::FEE_ADDRESS_XMR
        );
        assert_eq!(side_address(PayoutSide::User, "4user"), "4user");
        // And the fee address must itself be a valid payout, or the node can't
        // even mine its own slice.
        assert!(is_valid_xmr_address(FEE_ADDRESS_XMR));
    }

    /// Not just "20 of every 500" but WHICH 20 — the first, matching the
    /// desktop's `mining_secs % 500 < 20`. A slice at a different offset would
    /// still be 4%, but its ledger timestamps wouldn't line up with a desktop
    /// node's on the same fleet, breaking the shared-audit promise.
    #[test]
    fn fee_slice_is_the_first_20_seconds_of_each_window() {
        assert!(in_fee_slice(0), "slice opens the window");
        assert!(in_fee_slice(19), "last second of the slice");
        assert!(!in_fee_slice(20), "back on the user immediately after");
        assert!(
            !in_fee_slice(499),
            "last second of the window is the user's"
        );
        assert!(in_fee_slice(500), "next window's slice opens");
        assert!(in_fee_slice(519));
        assert!(!in_fee_slice(520));
    }

    /// The payout arrives from the server, which we do NOT trust for a value
    /// we're about to mine to for hours: a short or multibyte one previously
    /// reached a `&payout[..12]` slice and crash-looped the node. Guard both
    /// directions — accept real addresses, reject every shape that could reach
    /// that panic.
    #[test]
    fn payout_validator_accepts_real_addresses_and_rejects_junk() {
        assert!(is_valid_xmr_address(FEE_ADDRESS_XMR)); // real standard (4…), 95, base58
        let sub = format!("8{}", "1".repeat(94)); // subaddress prefix, 95, base58
        assert!(is_valid_xmr_address(&sub));
        assert!(!is_valid_xmr_address("")); // empty
        assert!(!is_valid_xmr_address("4short")); // too short — the [..12] panic case
        assert!(!is_valid_xmr_address(&"4".repeat(94))); // wrong length
        assert!(!is_valid_xmr_address(&format!("9{}", "1".repeat(94)))); // wrong prefix
        assert!(!is_valid_xmr_address(&format!("4{}", "0".repeat(94)))); // '0' not in base58
    }

    /// The est $/day the card shows: `profit::score` for the per-kH/s rate and
    /// `earnings::usd_per_day` for the figure — the SAME two shared functions
    /// the desktop uses, so parity is by construction rather than mirroring.
    #[test]
    fn xmr_earnings_math_matches_the_desktop_score() {
        // score(price, reward, units, diff) = 86400·1000·price·(reward/units)/diff
        // is USD/day per kH/s. Concrete sanity numbers: price $160, reward
        // 0.6 XMR (6e11 atomic, 1e12 units), diff 400e9.
        let per_kh = xmr_rate_per_kh(160.0, 6e11, 1e12, 400e9).unwrap();
        // 86400·1000·160·(0.6)/400e9 ≈ 2.0736e-2 USD/day per kH/s.
        assert!((per_kh - 2.0736e-2).abs() < 1e-6, "got {per_kh}");
        // A 4 kH/s node ≈ 8.3¢/day through the shared earnings fn.
        let day = pasiv_core::earnings::usd_per_day(4000.0, Some(per_kh)).unwrap();
        assert!((day - 0.082944).abs() < 1e-6);
        // Non-positive inputs yield None, never a bogus number (score parity).
        assert!(xmr_rate_per_kh(0.0, 6e11, 1e12, 400e9).is_none());
        assert!(xmr_rate_per_kh(160.0, 6e11, 1e12, 0.0).is_none());
        assert!(xmr_rate_per_kh(160.0, 0.0, 1e12, 400e9).is_none());
    }

    /// Process argv is world-readable (`/proc/<pid>/cmdline`), and this token
    /// unlocks an UNRESTRICTED xmrig API that can rewrite the payout address —
    /// so it must NEVER appear on the command line. The desktop adapter pins
    /// the same invariant under the same name. (It used to be on argv here,
    /// with a test asserting it was — a guard holding a vulnerability in
    /// place.)
    #[test]
    fn the_api_token_never_reaches_the_command_line() {
        let _g = ENV_LOCK.lock().unwrap(); // xmrig_args reads pool()
        let args = xmrig::xmrig_args("4payoutaddr", "/tmp/xmrig-runtime.json");
        assert!(
            args.iter()
                .all(|a| !a.contains("tok123") && a != "--http-access-token"),
            "the API token must travel in the 0600 runtime config, never argv"
        );
        let ci = args
            .iter()
            .position(|a| a == "-c")
            .expect("-c flag present");
        assert_eq!(args[ci + 1], "/tmp/xmrig-runtime.json");
        // Mines to the user's payout, on the pinned pool.
        let ui = args.iter().position(|a| a == "-u").expect("wallet flag");
        assert_eq!(args[ui + 1], "4payoutaddr");
        assert!(args.iter().any(|a| *a == pool()));
    }

    /// The device config holds a bearer credential; it must round-trip intact
    /// and, on unix, never be world-readable — `write_config` exists because a
    /// plain `fs::write` under a default umask produced 0644.
    #[test]
    fn device_config_round_trips_and_is_owner_only() {
        let tmp = std::env::temp_dir().join("pasivd-test-config.json");
        let _ = std::fs::remove_file(&tmp);
        let cfg = DeviceConfig {
            device_id: "dev-1".into(),
            secret: "s3cret".into(),
            payout_xmr: Some("4addr".into()),
        };
        write_config(&tmp, &cfg).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&tmp).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode & 0o077,
                0,
                "device secret must be 0600, got 0o{mode:o}"
            );
        }
        let back: DeviceConfig =
            serde_json::from_str(&std::fs::read_to_string(&tmp).unwrap()).unwrap();
        assert_eq!(back.device_id, "dev-1");
        assert_eq!(back.secret, "s3cret");
        assert_eq!(back.payout_xmr.as_deref(), Some("4addr"));
        // Rewriting over an existing file truncates rather than appends.
        write_config(&tmp, &cfg).unwrap();
        let again: DeviceConfig =
            serde_json::from_str(&std::fs::read_to_string(&tmp).unwrap()).unwrap();
        assert_eq!(again.device_id, "dev-1");
        let _ = std::fs::remove_file(&tmp);
    }

    /// A config written before payouts existed has no `payout_xmr` key at all;
    /// it must load as None rather than fail the whole read.
    #[test]
    fn old_config_without_payout_still_loads() {
        let cfg: DeviceConfig = serde_json::from_str(r#"{"device_id":"d","secret":"s"}"#).unwrap();
        assert!(cfg.payout_xmr.is_none());
    }

    /// PASIVD_CONFIG must win over every probed default — it's how tests and
    /// non-root installs point the daemon at their own file.
    #[test]
    fn config_path_honours_the_env_override() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: env mutation serialised by ENV_LOCK.
        unsafe { std::env::set_var("PASIVD_CONFIG", "/tmp/pasivd-test-alt.json") };
        assert_eq!(
            config_path(),
            std::path::PathBuf::from("/tmp/pasivd-test-alt.json")
        );
        unsafe { std::env::remove_var("PASIVD_CONFIG") };
    }

    /// The endpoint overrides exist so a fork or an auditor can point the
    /// daemon anywhere; the defaults are the shipped product.
    #[test]
    fn endpoints_default_to_pasiv_and_honour_overrides() {
        let _g = ENV_LOCK.lock().unwrap();
        assert_eq!(fn_url(), DEFAULT_FN_URL);
        assert_eq!(pool(), DEFAULT_POOL);
        unsafe { std::env::set_var("PASIVD_POOL", "example.org:3333") };
        assert_eq!(pool(), "example.org:3333");
        unsafe { std::env::remove_var("PASIVD_POOL") };
        assert_eq!(pool(), DEFAULT_POOL);
    }

    /// REGRESSION (0.1.2): a headless node showed as a bare "Mining" with no
    /// hashrate and no $/day because the push carried neither a lane nor an
    /// estimate. The snapshot must carry the xmrig lane always, and est_usd_day
    /// only when it's real — never a fabricated number on an idle/warming node.
    #[test]
    fn snapshot_carries_the_lane_and_only_a_real_est_usd_day() {
        let mining = build_snapshot("mining", 4000.0, Some(0.03));
        assert_eq!(mining["rollup"]["state"], "mining");
        // The lane the phone joins with stats.xmrig to render "CPU XMR <rate>".
        assert_eq!(mining["miners"]["xmrig"]["state"], "mining");
        assert!((mining["est_usd_day"].as_f64().unwrap() - 0.12).abs() < 1e-9);

        // No est when idle, when not hashing, or when the rate is unknown —
        // omit it rather than publish a number that isn't true.
        assert!(build_snapshot("idle", 0.0, Some(0.03))["est_usd_day"].is_null());
        assert!(build_snapshot("mining", 0.0, Some(0.03))["est_usd_day"].is_null());
        assert!(build_snapshot("mining", 4000.0, None)["est_usd_day"].is_null());

        // The lane is present even while starting, so the card shows "warming"
        // rather than nothing.
        let starting = build_snapshot("starting", 0.0, None);
        assert_eq!(starting["miners"]["xmrig"]["state"], "starting");
        assert!(starting["est_usd_day"].is_null());
    }
}

#[cfg(test)]
mod hardware_uplink_tests {
    /// pasivd sends `serde_json::to_value(hardware::detect())` in its rig row,
    /// and the mobile companion reads exactly these keys off it (see
    /// pasiv-mobile Rig.fromRow: cpu_model, cpu_cores, usable_threads). This is
    /// a cross-repo contract with nothing but a shared JSON shape between the
    /// two, so pin the shape here: if pasiv-core ever renames a field, a
    /// headless rig would silently go back to showing no CPU on the phone, and
    /// this is what would catch it instead of a person noticing.
    #[test]
    fn detect_serialises_to_the_keys_the_companion_reads() {
        let v = serde_json::to_value(pasiv_core::hardware::detect())
            .expect("hardware must serialise");
        let obj = v.as_object().expect("hardware is a JSON object");
        for key in ["cpu_cores", "usable_threads", "cpu_model", "gpus"] {
            assert!(obj.contains_key(key), "hardware blob lost the `{key}` key");
        }
        // Core count is always a positive number on a real host; the companion
        // guards `is num` but a zero would render "0 threads", which is a lie.
        assert!(
            obj["cpu_cores"].as_u64().is_some_and(|n| n >= 1),
            "cpu_cores must be a positive integer, got {}",
            obj["cpu_cores"]
        );
    }
}
