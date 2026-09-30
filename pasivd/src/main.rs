// SPDX-License-Identifier: GPL-3.0-only
//! pasivd — the headless Pasiv node.
//!
//! One job: turn a server/lab box into a rig in your Pasiv fleet with two
//! commands and zero UI. A daemon can't do SIWE, so it pairs like a TV app:
//!
//!   pasivd claim   → prints a 6-char code; approve it in the Pasiv companion
//!   pasivd run     → mines Monero (CPU) paid to YOUR address — in USDT via
//!                    unMineable when the account has a USDT payout, else in
//!                    XMR direct — publishes state to the fleet, obeys
//!                    start/stop from the phone
//!
//! Trust model mirrors the desktop (docs/FEES.md — the never-list):
//!   - non-custodial: mines straight to the owner's payout address
//!   - fee parity: the same time-sliced 4% (20 s per 500 s of Mining), to the
//!     same compile-time fee address as the desktop on the same route (the
//!     BTC treasury on unMineable, the Monero address direct), via the same
//!     xmrig config hot-reload
//!   - remote actions are start, stop and update, and an update installs only
//!     a release Pasiv signed (docs/FEES.md never-list item 8; see update.rs)
//!   - the miner binary is fetched from xmrig's official release and
//!     sha256-verified against a compile-time pin before first run

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pasiv_core::address::is_valid_xmr_address;
use pasiv_core::fee::{self, PayoutSide, SliceScheduler, SwapFailure, FEE_ADDRESS_XMR};
use serde::{Deserialize, Serialize};
mod doctor;
mod ui;
mod update;
mod xmrig;
use doctor::cmd_doctor;
use xmrig::{ensure_xmrig, spawn_xmrig, xmrig_pools_on, xmrig_set_user, xmrig_summary, Miner};

/// The HTTP client every cloud call shares. Both timeouts are load-bearing:
/// a `reqwest::Client::new()` has none, so a stalled TLS handshake or a
/// half-open connection to the edge function could hold a request forever —
/// and before 0.1.9 that request ran on the same task that respawns the
/// miner and ends fee slices. Downloads that legitimately take longer (the
/// xmrig tarball, a staged update) override the total per request.
pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Atomic, owner-only file write. The bytes go to a temp file in the SAME
/// directory (created 0600 before anything is written), are fsync'd, and are
/// renamed over `path` — so a crash or a full disk mid-write leaves the old
/// file intact, never a truncated one. Returns `Ok(false)` without touching
/// the disk when `path` already holds exactly `bytes`.
///
/// The truncate-then-write it replaces had a window in which the file was
/// empty: a power cut there cost the node its identity (device id + secret)
/// and the payout it needs to mine without the cloud.
pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    use std::io::Write;
    if std::fs::read(path).is_ok_and(|cur| cur == bytes) {
        return Ok(false);
    }
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config".into());
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        let mut f = opts
            .open(&tmp)
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        std::fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))?;
        // The rename is durable only once the directory entry is: fsync the
        // directory too (best-effort — not every filesystem allows it).
        if let Some(dir) = dir {
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(true)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Write the device config. It holds the device secret — a bearer credential
/// for this node's cloud identity — so it must never be world-readable, which
/// is what `std::fs::write` produces under a default umask. Atomic (see
/// [`write_private_atomic`]), and afterwards handed to the service user so
/// the sandboxed unit can read what root wrote at claim time.
fn write_config(path: &Path, cfg: &DeviceConfig) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    write_private_atomic(path, &bytes)?;
    hand_to_service_user(path);
    Ok(())
}

/// The name of the static system user the unit runs as (install.sh).
pub(crate) const SERVICE_USER: &str = "pasivd";

/// `pasivd claim` runs as root and writes the config 0600; the service runs
/// as [`SERVICE_USER`] and has to read it. Hand the file over when that user
/// exists. Best-effort and silent otherwise (a non-systemd or non-root
/// install, or a caller that isn't root — chown then fails with EPERM and
/// the file is already readable by whoever wrote it).
#[cfg(unix)]
fn hand_to_service_user(path: &Path) {
    let Some((uid, gid)) = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|p| passwd_ids(&p, SERVICE_USER))
    else {
        return;
    };
    let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
}

#[cfg(not(unix))]
fn hand_to_service_user(_path: &Path) {}

/// Pure: `(uid, gid)` for `user` out of /etc/passwd text.
pub(crate) fn passwd_ids(passwd: &str, user: &str) -> Option<(u32, u32)> {
    passwd.lines().find_map(|l| {
        let mut f = l.split(':');
        if f.next()? != user {
            return None;
        }
        f.next()?; // password field
        Some((
            f.next()?.trim().parse().ok()?,
            f.next()?.trim().parse().ok()?,
        ))
    })
}

/// The payout the node last heard from the account, kept in the STATE
/// directory — the one place the sandboxed service can write (`/etc` is
/// read-only under `ProtectSystem=strict`, so the config file itself is
/// root's, written at claim time). This is what lets a node keep mining when
/// the cloud is down or the device was revoked: the address is the owner's
/// own, heard from their account, and mining to it costs nobody anything.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub(crate) struct PayoutCache {
    #[serde(default)]
    payout_xmr: Option<String>,
    #[serde(default)]
    payout_usdt: Option<String>,
}

pub(crate) fn payout_cache_path() -> PathBuf {
    data_dir().join("payout.json")
}

fn read_payout_cache() -> Option<PayoutCache> {
    serde_json::from_str(&std::fs::read_to_string(payout_cache_path()).ok()?).ok()
}

/// Atomic and only when changed; failure is logged, never fatal — a node
/// with a read-only state directory still mines, it just can't survive a
/// cloud outage across a restart.
fn save_payout_cache(cache: &PayoutCache) {
    let Ok(bytes) = serde_json::to_vec_pretty(cache) else {
        return;
    };
    if let Err(e) = write_private_atomic(&payout_cache_path(), &bytes) {
        eprintln!("could not cache the payout locally: {e}");
    }
}

/// The payout to start from: the state-dir cache when it exists (it is at
/// least as new as the claim), else what `pasivd claim` wrote.
pub(crate) fn cached_payout(cfg: &DeviceConfig) -> PayoutCache {
    read_payout_cache().unwrap_or_else(|| PayoutCache {
        payout_xmr: cfg.payout_xmr.clone(),
        payout_usdt: cfg.payout_usdt.clone(),
    })
}

// The Pasiv cloud + pool are DEFAULTS, overridable by environment so a fork —
// or an auditor — can point the daemon anywhere without patching source:
//   PASIVD_API_URL   the device API endpoint
//   PASIVD_ANON_KEY  the publishable key for it (RLS/edge auth do the enforcing)
//   PASIVD_POOL      the direct-route stratum host:port (the unMineable route
//                    always uses unMineable's RandomX host)
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
    /// USDT payout (unMineable route) from the owner's account, when they
    /// chose it in the desktop app. Preferred over payout_xmr — see `target`.
    #[serde(default)]
    payout_usdt: Option<String>,
}

/// Where this node mines and who each fee side pays.
#[derive(Debug, PartialEq)]
pub(crate) struct MiningTarget {
    pub pool: String,
    /// The owner's login: `USDT:<addr>.<host>#<referral>` on unMineable, or
    /// the bare XMR address on the direct pool.
    pub user: String,
    /// The fee slice's login: the treasury on unMineable, FEE_ADDRESS_XMR direct.
    pub fee: String,
    /// On the unMineable route — decides what the fee ledger records.
    pub unmineable: bool,
}

impl MiningTarget {
    /// A fresh fee scheduler whose ledger records where this node's slices
    /// really go (the treasury on unMineable, the XMR fee address direct).
    fn scheduler(&self) -> SliceScheduler {
        if self.unmineable {
            SliceScheduler::for_unmineable(pasiv_core::types::Coin::Xmr)
        } else {
            SliceScheduler::new()
        }
    }
}

/// The route rule, pure so it is tested: a valid USDT address on the account
/// means the owner chose unMineable (the desktop publishes it only then);
/// otherwise the direct Monero pool with the XMR address, exactly as before.
pub(crate) fn target(usdt: Option<&str>, xmr: Option<&str>, host: &str) -> Option<MiningTarget> {
    use pasiv_core::unmineable::{login, Algo, PayoutAsset};
    if let Some(addr) = usdt.map(str::trim) {
        if let Some(asset) = PayoutAsset::usdt_for(addr) {
            if let Some(user) = login(asset, addr, host, fee::UNMINEABLE_REFERRAL) {
                return Some(MiningTarget {
                    pool: format!("{}:{}", Algo::RandomX.host(), Algo::PORT),
                    user,
                    fee: fee::unmineable_fee_login(host),
                    unmineable: true,
                });
            }
        }
    }
    xmr.filter(|a| is_valid_xmr_address(a))
        .map(|a| MiningTarget {
            pool: pool(),
            user: a.to_string(),
            fee: FEE_ADDRESS_XMR.to_string(),
            unmineable: false,
        })
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
    let client = http_client();
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
            let payout_usdt = v["payout_usdt"].as_str().map(|s| s.to_string());
            let cfg = DeviceConfig {
                device_id,
                secret,
                payout_xmr: payout.clone(),
                payout_usdt: payout_usdt.clone(),
            };
            let path = config_path();
            write_config(&path, &cfg)?;
            // A re-claim binds the node to whoever approved THIS code; a
            // payout cached from the previous owner must not outlive that.
            let _ = std::fs::remove_file(payout_cache_path());
            println!();
            println!(
                "  {} {} — config saved to {}",
                ui::tick(),
                ui::bold("Claimed"),
                ui::dim(&path.display().to_string())
            );
            if payout.is_none() && payout_usdt.is_none() {
                println!(
                    "  {} No payout on your account yet. Set one in the Pasiv desktop",
                    ui::warn_mark()
                );
                println!("    app (Wallets) and it syncs here automatically.");
            }
            println!(
                "  Start mining:  {}",
                ui::bold("sudo systemctl enable --now pasivd")
            );
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
fn side_address(side: PayoutSide, t: &MiningTarget) -> &str {
    match side {
        PayoutSide::Fee => &t.fee,
        PayoutSide::User => &t.user,
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
//
// Two tasks, one rule: the miner loop never waits on the network.
//
//   miner loop  — spawns/respawns xmrig, reads its local stats, drives the
//                 fee-slice reconcile, applies remote commands. Every await
//                 in it is loopback with a 3 s timeout.
//   cloud link  — pushes state, receives commands, re-polls the payout,
//                 checks for updates, turns on unMineable auto pay. Every
//                 request is bounded by `http_client`'s timeouts, and a slow
//                 one delays the NEXT push, never a fee-slice end.
//
// They talk over channels: the miner loop publishes a `MinerReport` (watch —
// the link reads the latest whenever it pushes); the link sends `CloudEvent`s
// (commands, a changed payout, a staged update); the miner loop answers
// commands with `Completion`s the link posts back.

/// What the cloud link publishes about the miner (the latest wins).
#[derive(Clone)]
struct MinerReport {
    state: &'static str,
    hashrate: f64,
    accepted: u64,
    rejected: u64,
    mining_secs: u64,
}

impl Default for MinerReport {
    fn default() -> Self {
        MinerReport {
            state: "starting",
            hashrate: 0.0,
            accepted: 0,
            rejected: 0,
            mining_secs: 0,
        }
    }
}

/// From the cloud link to the miner loop.
enum CloudEvent {
    /// A remote command to apply (start / stop / anything else = unsupported).
    Command { id: String, action: String },
    /// The account's payout as the hourly poll last heard it.
    Payout(PayoutCache),
    /// A signed update is staged: stop the miner and exit into it.
    RestartInto(String),
}

/// A command's outcome, for the link to post back as `complete`.
struct Completion {
    id: String,
    ok: bool,
    result: String,
}

/// What a `poll` said.
enum Poll {
    Claimed(PayoutCache),
    /// The cloud answered but this device is not (or no longer) claimed.
    NotClaimed(String),
}

async fn poll_payout(client: &reqwest::Client, cfg: &DeviceConfig) -> Result<Poll, String> {
    let v = api(
        client,
        serde_json::json!({"action":"poll","device_id":cfg.device_id,"secret":cfg.secret}),
    )
    .await?;
    if v["status"] != "claimed" {
        return Ok(Poll::NotClaimed(
            v["status"].as_str().unwrap_or("unknown").to_string(),
        ));
    }
    Ok(Poll::Claimed(PayoutCache {
        payout_xmr: v["payout_xmr"].as_str().map(str::to_string),
        payout_usdt: v["payout_usdt"].as_str().map(str::to_string),
    }))
}

fn target_for(payout: &PayoutCache, host: &str) -> Option<MiningTarget> {
    target(
        payout.payout_usdt.as_deref(),
        payout.payout_xmr.as_deref(),
        host,
    )
}

/// How long until the next poll after this one: an hour once the cloud
/// answers (claimed or not — a revoked device is re-checked hourly and warned
/// about hourly), a minute while it does not.
const POLL_OK: Duration = Duration::from_secs(3600);
const POLL_RETRY: Duration = Duration::from_secs(60);

/// Startup: one poll, then mine. The cloud is consulted, never obeyed into
/// silence — before 0.1.9 a failed poll here was `?`, so an edge-function
/// outage, a TLS hiccup, or a revoked device made `pasivd run` exit 1 and
/// systemd restart it forever without a hash. Now: any failure logs loudly
/// and, if a payout is cached locally, mining starts on it; the cloud link
/// keeps re-polling. Only a node with NO payout anywhere waits — there is
/// nothing to mine to.
///
/// Returns the target and how long the link should wait before its next poll.
async fn startup_target(
    client: &reqwest::Client,
    cfg: &DeviceConfig,
    host: &str,
) -> (MiningTarget, PayoutCache, Duration) {
    let mut payout = cached_payout(cfg);
    let mut warned_no_payout = false;
    loop {
        let next = match poll_payout(client, cfg).await {
            Ok(Poll::Claimed(fresh)) => {
                if fresh != payout {
                    save_payout_cache(&fresh);
                    payout = fresh;
                }
                if let Some(t) = target_for(&payout, host) {
                    return (t, payout, POLL_OK);
                }
                if !warned_no_payout {
                    eprintln!(
                        "no payout on the account yet — set one in the Pasiv app; checking every 60s"
                    );
                    warned_no_payout = true;
                }
                POLL_RETRY
            }
            Ok(Poll::NotClaimed(status)) => {
                if let Some(t) = target_for(&payout, host) {
                    eprintln!(
                        "warning: the cloud reports this device as {status} — mining on the \
                         cached payout anyway; re-claim it from the companion to restore remote control"
                    );
                    return (t, payout, POLL_OK);
                }
                eprintln!("device is {status} and no payout is cached — run `pasivd claim`; retrying in 60s");
                POLL_RETRY
            }
            Err(e) => {
                if let Some(t) = target_for(&payout, host) {
                    eprintln!("warning: cloud unreachable ({e}) — mining on the cached payout");
                    return (t, payout, POLL_RETRY);
                }
                eprintln!("cloud unreachable ({e}) and no payout cached yet — retrying in 60s");
                POLL_RETRY
            }
        };
        tokio::time::sleep(next).await;
    }
}

/// Is this build proven good enough to keep? Judged by WORK, not by the
/// cloud: five minutes of hashing, or any accepted share, in this process.
/// A push that succeeded used to count — so a build whose miner never hashed
/// stayed "healthy" as long as the uplink worked, and one that mined
/// perfectly while the cloud was down was abandoned after three starts. A
/// node the owner has stopped can't hash, so it counts as healthy once it
/// has simply stayed up ten minutes without crash-looping.
fn build_checked_in(hash_secs: u64, accepted: u64, want_mining: bool, uptime_secs: u64) -> bool {
    hash_secs >= 300 || accepted > 0 || (!want_mining && uptime_secs >= 600)
}

/// Consecutive local stats failures after which the last hashrate is no
/// longer believed: two misses (10 s) of a 3 s-timeout loopback call means
/// xmrig is wedged or gone, and "still mining at N H/s" would be a lie the
/// fleet view repeats for as long as it lasts.
const STATS_MISSES_UNKNOWN: u32 = 2;

/// How often a missing/failed xmrig binary is re-fetched.
const ENSURE_RETRY: Duration = Duration::from_secs(600);

async fn cmd_run() -> Result<(), String> {
    let path = config_path();
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("no config at {} — run `pasivd claim` first", path.display()))?;
    let cfg: DeviceConfig = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let host = hostname();
    let client = http_client();

    let (mut tgt, mut payout, first_poll_in) = startup_target(&client, &cfg, &host).await;

    // The miner binary, fetched and pinned. A failure here is no longer
    // fatal: the loop below retries every ENSURE_RETRY, so a node installed
    // while the release host is unreachable starts mining when it is back.
    let mut bin: Option<PathBuf> = match ensure_xmrig(&client).await {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("{e} — retrying every {}s", ENSURE_RETRY.as_secs());
            None
        }
    };
    let mut last_ensure = Instant::now();

    let mut miner: Option<Miner> = None;
    // A headless node's default job is to mine — unless its owner stopped it.
    // The stop survives a restart (an update restarts the process), so a
    // node the owner paused never starts mining again by itself.
    let stopped_flag = data_dir().join("stopped");
    let mut want_mining = !stopped_flag.exists();
    let mut mining_secs: u64 = 0;
    // The shared enforcement state machine — fresh per spawn (a respawned
    // miner always comes up on the user's address).
    let mut sched = tgt.scheduler();
    let mut last_hashrate = 0.0_f64;
    let mut stats_misses: u32 = 0;
    let mut accepted: u64 = 0;
    let mut rejected: u64 = 0;
    // Rollback health (update.rs): judged by work done in this process.
    let started = Instant::now();
    let mut hash_secs: u64 = 0;
    let mut checked_in = false;

    // Detect the hardware ONCE — the CPU cannot change under a running process,
    // and detect() shells out to read the model — then send it with every push.
    // Without this a headless rig's `hardware` was always {}, so the companion
    // showed no CPU for it while every desktop rig showed one. It is the SAME
    // pasiv_core::hardware::detect() the desktop app serialises into its own rig
    // row, so the shape the companion parses is identical — capability, never
    // identity: core counts, usable threads and the CPU model, no serial, no id.
    // Null rather than a spurious {} if it somehow fails to serialise.
    let hardware =
        serde_json::to_value(pasiv_core::hardware::detect()).unwrap_or(serde_json::Value::Null);

    let (report_tx, report_rx) = tokio::sync::watch::channel(MinerReport::default());
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel::<CloudEvent>();
    let (done_tx, done_rx) = tokio::sync::mpsc::unbounded_channel::<Completion>();
    tokio::spawn(cloud_link(CloudLink {
        client: client.clone(),
        cfg: cfg.clone(),
        host: host.clone(),
        hardware,
        unmineable_usdt: tgt.unmineable.then(|| payout.payout_usdt.clone()).flatten(),
        first_poll_in,
        report_rx,
        events_tx,
        done_rx,
    }));

    println!("{VERSION} — node {host} → {}", tgt.pool);

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Apply whatever the cloud link delivered since the last tick.
        while let Ok(ev) = events_rx.try_recv() {
            match ev {
                CloudEvent::Command { id, action } => {
                    println!("remote command: {action}");
                    let (ok, result) = match action.as_str() {
                        "start" => {
                            want_mining = true;
                            let _ = std::fs::remove_file(&stopped_flag);
                            (true, "start ok".to_string())
                        }
                        "stop" => {
                            want_mining = false;
                            let _ = std::fs::write(&stopped_flag, b"");
                            (true, "stop ok".to_string())
                        }
                        other => (false, format!("unsupported command: {other}")),
                    };
                    let _ = done_tx.send(Completion { id, ok, result });
                }
                CloudEvent::Payout(fresh) => {
                    // The account's payout moved. A payout REMOVED from the
                    // account is not followed mid-run (the address in hand is
                    // still the owner's own); it takes effect at the next
                    // start, exactly as before.
                    if fresh == payout {
                        continue;
                    }
                    let Some(new_tgt) = target_for(&fresh, &host) else {
                        eprintln!(
                            "warning: the account no longer has a payout — still mining on the \
                             one this node last heard; set one in the Pasiv app"
                        );
                        continue;
                    };
                    save_payout_cache(&fresh);
                    payout = fresh;
                    if new_tgt != tgt {
                        println!(
                            "payout changed on the account — switching to {} → {}…",
                            new_tgt.pool,
                            new_tgt.user.chars().take(16).collect::<String>()
                        );
                        tgt = new_tgt;
                        if let Some(m) = &mut miner {
                            let _ = m.child.kill().await;
                        }
                        miner = None; // respawns next tick on the new target
                        last_hashrate = 0.0;
                    }
                }
                CloudEvent::RestartInto(version) => {
                    restart_for_update(&mut miner, &version).await;
                }
            }
        }

        // Reconcile desired vs actual miner state.
        match (&mut miner, want_mining) {
            (None, true) => {
                // No usable binary (the release host was unreachable at start,
                // or the file vanished): re-fetch, rate-limited so a long
                // outage never hammers it. The download is the one network
                // call on this task, and only ever runs while no miner exists
                // — there is no fee slice to stall.
                if bin.is_none() && last_ensure.elapsed() >= ENSURE_RETRY {
                    last_ensure = Instant::now();
                    match ensure_xmrig(&client).await {
                        Ok(b) => bin = Some(b),
                        Err(e) => eprintln!("{e} — retrying in {}s", ENSURE_RETRY.as_secs()),
                    }
                }
                // No binary yet: nothing to spawn this tick; the report below
                // still goes out so the fleet sees "starting", not a stale row.
                if let Some(b) = &bin {
                    let token: String = {
                        use rand::Rng;
                        let mut r = rand::thread_rng();
                        (0..32)
                            .map(|_| format!("{:x}", r.gen_range(0..16)))
                            .collect()
                    };
                    match spawn_xmrig(b, &tgt.pool, &tgt.user, &token) {
                        Ok(child) => {
                            println!(
                                "miner started ({} → {}…)",
                                tgt.pool,
                                tgt.user.chars().take(16).collect::<String>()
                            );
                            sched = tgt.scheduler();
                            stats_misses = 0;
                            miner = Some(Miner { child, token });
                        }
                        Err(e) => {
                            eprintln!("{e}");
                            if !b.exists() {
                                eprintln!("xmrig binary is missing — it will be re-fetched");
                                bin = None;
                            }
                        }
                    }
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
                    last_hashrate = 0.0;
                    continue;
                }
            }
            (None, false) => {}
        }

        // Stats + fee slice while mining.
        if let Some(m) = &mut miner {
            match xmrig_summary(&client, &m.token).await {
                Some(s) => {
                    stats_misses = 0;
                    last_hashrate = s["hashrate"]["total"][0].as_f64().unwrap_or(0.0);
                    accepted = s["results"]["shares_good"].as_u64().unwrap_or(accepted);
                    let total = s["results"]["shares_total"].as_u64().unwrap_or(0);
                    rejected = total.saturating_sub(accepted);
                }
                None => {
                    stats_misses += 1;
                    if stats_misses == STATS_MISSES_UNKNOWN {
                        eprintln!("xmrig stats unavailable — reporting hashrate as unknown");
                    }
                    if stats_misses >= STATS_MISSES_UNKNOWN {
                        last_hashrate = 0.0;
                    }
                }
            }
            // Mining time only accrues while actually hashing.
            if last_hashrate > 0.0 {
                mining_secs += 5;
                hash_secs += 5;
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
            // Now: every tick, ask xmrig where it is actually mining and
            // correct it. "Where" means EVERY pool entry (all_pools_use), so a
            // backup pool xmrig failed over to can't hide a wrong login.
            let want = sched.desired(last_hashrate > 0.0, mining_secs);
            let target = side_address(want, &tgt);
            match xmrig_pools_on(&client, &m.token, target).await {
                Some(true) => {
                    confirm_side(&mut sched, want, last_hashrate);
                }
                Some(false) | None => {
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

        // Rollback health: this build has done real work, keep choosing it.
        if !checked_in
            && build_checked_in(
                hash_secs,
                accepted,
                want_mining,
                started.elapsed().as_secs(),
            )
        {
            update::mark_healthy();
            checked_in = true;
        }

        // "idle" only when the owner has it stopped; a node that wants to mine
        // and can't yet (no binary, a spawn that failed) is still "starting".
        let state = if miner.is_some() {
            if last_hashrate > 0.0 {
                "mining"
            } else {
                "starting"
            }
        } else if want_mining {
            "starting"
        } else {
            "idle"
        };
        let _ = report_tx.send(MinerReport {
            state,
            hashrate: last_hashrate,
            accepted,
            rejected,
            mining_secs,
        });
    }
}

/// Everything the cloud link owns. It never touches the miner.
struct CloudLink {
    client: reqwest::Client,
    cfg: DeviceConfig,
    host: String,
    hardware: serde_json::Value,
    /// The USDT address to keep unMineable auto pay on for, on that route.
    unmineable_usdt: Option<String>,
    first_poll_in: Duration,
    report_rx: tokio::sync::watch::Receiver<MinerReport>,
    events_tx: tokio::sync::mpsc::UnboundedSender<CloudEvent>,
    done_rx: tokio::sync::mpsc::UnboundedReceiver<Completion>,
}

/// Push cadence, in seconds.
const PUSH_SECS: u64 = 30;
/// Pushes per day, for the daily chores.
const PUSHES_PER_DAY: u64 = 86_400 / PUSH_SECS;

async fn cloud_link(link: CloudLink) {
    let CloudLink {
        client,
        cfg,
        host,
        hardware,
        unmineable_usdt,
        first_poll_in,
        report_rx,
        events_tx,
        mut done_rx,
    } = link;

    // Earnings estimate: a separate client because CoinGecko 403s reqwest's
    // default agent, and a cached rate refreshed ~every 10 min (the desktop
    // re-ranks on a similar cadence) — the per-tick hashrate is what varies,
    // not the network rate.
    let rate_client = reqwest::Client::builder()
        .user_agent(concat!("pasivd/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| http_client());
    let mut rate_per_kh: Option<f64> = None;

    let mut push = tokio::time::interval(Duration::from_secs(PUSH_SECS));
    push.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    push.tick().await; // the first tick fires immediately; push 30 s in
    let mut next_poll = tokio::time::Instant::now() + first_poll_in;
    let mut tick: u64 = 0;
    let mut push_failures: u64 = 0;

    loop {
        tokio::select! {
            Some(c) = done_rx.recv() => {
                let _ = api(
                    &client,
                    serde_json::json!({
                        "action":"complete","device_id":cfg.device_id,"secret":cfg.secret,
                        "command_id": c.id, "ok": c.ok, "result": c.result,
                    }),
                )
                .await;
            }
            _ = tokio::time::sleep_until(next_poll) => {
                let wait = match poll_payout(&client, &cfg).await {
                    Ok(Poll::Claimed(fresh)) => {
                        let _ = events_tx.send(CloudEvent::Payout(fresh));
                        POLL_OK
                    }
                    Ok(Poll::NotClaimed(status)) => {
                        eprintln!(
                            "warning: the cloud reports this device as {status} — still mining \
                             on the cached payout; re-claim it from the companion to restore \
                             remote control"
                        );
                        POLL_OK
                    }
                    Err(e) => {
                        eprintln!("cloud poll failed: {e} — retrying in {}s", POLL_RETRY.as_secs());
                        POLL_RETRY
                    }
                };
                next_poll = tokio::time::Instant::now() + wait;
            }
            _ = push.tick() => {
                tick += 1;
                // Refresh the earnings rate on the first push and ~every 10 min.
                // A failure keeps the last known rate (or none) and retries.
                if tick == 1 || tick.is_multiple_of(20) {
                    if let Some(r) = fetch_xmr_rate_per_kh(&rate_client).await {
                        rate_per_kh = Some(r);
                    }
                }
                let r = report_rx.borrow().clone();
                let snapshot = build_snapshot(r.state, r.hashrate, rate_per_kh);
                let pushed = api(
                    &client,
                    serde_json::json!({
                        "action": "push",
                        "device_id": cfg.device_id,
                        "secret": cfg.secret,
                        "name": host,
                        "platform": "linux",
                        "app_version": VERSION,
                        "hardware": hardware,
                        "active_coin": "XMR",
                        // No "payouts" — see remote/api.rs. It was never read, and
                        // sending it contradicted the privacy policy. The edge
                        // function and the rigs trigger both drop it now anyway.
                        "snapshot": snapshot,
                        "stats": {"xmrig": {"hashrate_avg": r.hashrate, "accepted": r.accepted, "rejected": r.rejected}},
                        "session_mining_ms": r.mining_secs * 1000,
                    }),
                )
                .await;
                match pushed {
                    Err(e) => {
                        push_failures += 1;
                        // The first failure, then one line every ~10 min: a
                        // revoked device or a long outage fails every push,
                        // and 2 880 identical lines a day help nobody.
                        if push_failures == 1 || push_failures.is_multiple_of(20) {
                            eprintln!("push failed ({push_failures} in a row): {e} — mining continues");
                        }
                    }
                    Ok(v) => {
                        if push_failures > 0 {
                            println!("cloud reachable again after {push_failures} failed pushes");
                            push_failures = 0;
                        }
                        for cmd in v["commands"].as_array().cloned().unwrap_or_default() {
                            let id = cmd["id"].as_str().unwrap_or("").to_string();
                            let action = cmd["action"].as_str().unwrap_or("").to_string();
                            if action == "update" {
                                // Network work stays here; the miner loop only
                                // gets told to stop and exit once it is staged.
                                println!("remote command: update");
                                let (ok, result, staged) = match update::fetch_and_stage(&client).await {
                                    Ok(Some(v)) => (true, format!("updating to {v}"), Some(v)),
                                    Ok(None) => (true, "already up to date".to_string(), None),
                                    Err(e) => (false, format!("update failed: {e}"), None),
                                };
                                let _ = api(
                                    &client,
                                    serde_json::json!({
                                        "action":"complete","device_id":cfg.device_id,"secret":cfg.secret,
                                        "command_id": id, "ok": ok, "result": result,
                                    }),
                                )
                                .await;
                                if let Some(v) = staged {
                                    let _ = events_tx.send(CloudEvent::RestartInto(v));
                                }
                            } else {
                                let _ = events_tx.send(CloudEvent::Command { id, action });
                            }
                        }
                    }
                }

                // unMineable only pays an address automatically once its "auto pay"
                // is on, and it starts off. Check ~5 min after start, then daily.
                if tick % PUSHES_PER_DAY == 10 {
                    if let Some(addr) = unmineable_usdt.as_deref() {
                        ensure_auto_pay(&client, addr).await;
                    }
                }

                // Daily update check, first one ~10 min after start so a
                // crash-looping node never hammers the release host.
                if tick % PUSHES_PER_DAY == 20 {
                    match update::fetch_and_stage(&client).await {
                        Ok(Some(v)) => {
                            let _ = events_tx.send(CloudEvent::RestartInto(v));
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("update check failed: {e}"),
                    }
                }
            }
        }
    }
}

/// Turn on unMineable's daily auto pay for the owner's USDT address if it is
/// off (it is off for every new address, and off means "press Payout now on
/// the website"). Needs the address's unMineable id, which exists once the
/// address has mined; before that this quietly tries again tomorrow.
async fn ensure_auto_pay(client: &reqwest::Client, address: &str) {
    use pasiv_core::unmineable::{address_api_url, auto_pay_url, PayoutAsset};
    let Some(url) = PayoutAsset::usdt_for(address).and_then(|a| address_api_url(a, address)) else {
        return;
    };
    let Ok(resp) = client.get(url).send().await else {
        return;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return;
    };
    if v["data"]["auto"].as_bool() != Some(false) {
        return;
    }
    let Some(set) = v["data"]["uuid"].as_str().and_then(auto_pay_url) else {
        return;
    };
    match client
        .post(set)
        .json(&serde_json::json!({ "setting": true }))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            println!("unMineable auto pay turned on (paid daily at 13:00 UTC over the threshold)")
        }
        Ok(r) => eprintln!("could not turn on unMineable auto pay: {}", r.status()),
        Err(e) => eprintln!("could not turn on unMineable auto pay: {e}"),
    }
}

/// A verified update is staged: stop the miner and exit, so systemd
/// (Restart=always) starts the process again and the launcher execs the new
/// build. The miner is killed first so it never outlives the process that
/// owns its fee slice.
async fn restart_for_update(miner: &mut Option<Miner>, version: &str) {
    if let Some(m) = miner {
        let _ = m.child.kill().await;
    }
    *miner = None;
    println!("update {version} staged — restarting into it");
    std::process::exit(0);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // A bare `pasivd` shows help rather than silently starting the daemon: the
    // systemd unit runs `pasivd run` explicitly (install.sh), so nothing depends
    // on the old bare-means-run default, and "run help when unsure" is what a
    // person expects from a modern CLI.
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    // `-h`/`--help` anywhere turns a command into its own help page.
    let wants_help = args.iter().any(|a| a == "-h" || a == "--help");

    let result = match cmd {
        "help" | "-h" | "--help" => {
            ui::print_help(VERSION);
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("{VERSION}");
            Ok(())
        }
        "claim" | "run" | "doctor" | "update" if wants_help => {
            ui::print_command_help(cmd);
            Ok(())
        }
        "claim" => cmd_claim().await,
        "run" => {
            update::launch_staged_if_any();
            cmd_run().await
        }
        "doctor" => cmd_doctor().await,
        "update" => update::cmd_update().await,
        other => {
            // Usage error (exit 2), distinct from a runtime failure (exit 1), so
            // a wrapper script can tell "you typed it wrong" from "it broke".
            ui::unknown(other);
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("{} {e}", ui::red("error:"));
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
        let direct = MiningTarget {
            pool: pool(),
            user: "4user".into(),
            fee: FEE_ADDRESS_XMR.into(),
            unmineable: false,
        };
        assert_eq!(
            side_address(PayoutSide::Fee, &direct),
            pasiv_core::fee::FEE_ADDRESS_XMR
        );
        assert_eq!(side_address(PayoutSide::User, &direct), "4user");
        // And the fee address must itself be a valid payout, or the node can't
        // even mine its own slice.
        assert!(is_valid_xmr_address(FEE_ADDRESS_XMR));
    }

    /// Not just "20 of every 500" but WHICH 20 — the LAST, matching the
    /// desktop (both call the shared `in_fee_slice`). The slice closes each
    /// window so a session that ends early never paid it; ledger timestamps
    /// line up across a fleet, keeping the shared-audit promise.
    #[test]
    fn fee_slice_is_the_last_20_seconds_of_each_window() {
        assert!(!in_fee_slice(0), "a new session starts on the user");
        assert!(!in_fee_slice(479), "still the user's");
        assert!(in_fee_slice(480), "slice opens 20 s before the window ends");
        assert!(in_fee_slice(499), "last second of the window is the fee's");
        assert!(!in_fee_slice(500), "next window opens on the user");
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
        let args = xmrig::xmrig_args(&pool(), "4payoutaddr", "/tmp/xmrig-runtime.json");
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
            payout_usdt: None,
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

    /// The write is atomic: the bytes land in a temp file beside the target
    /// and are renamed over it, so the old file is intact until the new one
    /// is complete; a rewrite of identical contents touches nothing (the
    /// service does this every poll); and no temp file is left behind.
    #[test]
    fn config_writes_are_atomic_and_skipped_when_unchanged() {
        let dir = std::env::temp_dir().join(format!("pasivd-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("config.json");
        assert!(write_private_atomic(&path, b"one").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        // Unchanged contents: no write (the return value says so).
        assert!(!write_private_atomic(&path, b"one").unwrap());
        assert!(write_private_atomic(&path, b"two").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // Nothing but the target remains in the directory.
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["config.json"], "temp file left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The unit must run as the SAME static user the claim hands the config
    /// to. DynamicUser mints a new uid per start that can never own the 0600
    /// secret — the 0.1.8 stock install that could not read its own config.
    #[test]
    fn the_installer_runs_the_unit_as_the_static_service_user() {
        let sh = include_str!("../install.sh");
        assert!(sh.contains(&format!("\nUser={SERVICE_USER}\n")));
        assert!(sh.contains(&format!("\nGroup={SERVICE_USER}\n")));
        assert!(!sh.contains("DynamicUser=yes"), "DynamicUser is back");
        assert!(
            sh.contains("useradd --system"),
            "the installer must create the user"
        );
        assert!(sh.contains("ProtectSystem=strict"), "the sandbox must stay");
    }

    /// The uid/gid the claim-time config is handed to, out of /etc/passwd.
    #[test]
    fn passwd_lookup_finds_the_service_user_exactly() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      pasivd-old:x:990:990::/nonexistent:/usr/sbin/nologin\n\
                      pasivd:x:991:992::/nonexistent:/usr/sbin/nologin\n";
        assert_eq!(passwd_ids(passwd, "pasivd"), Some((991, 992)));
        assert_eq!(passwd_ids(passwd, "root"), Some((0, 0)));
        assert_eq!(passwd_ids(passwd, "nobody"), None);
        assert_eq!(passwd_ids("", "pasivd"), None);
        assert_eq!(passwd_ids("pasivd:x:notanumber:1::\n", "pasivd"), None);
    }

    /// Rollback health is judged by work, never by the cloud: a build that
    /// hashes is kept; one whose uplink works but whose miner never hashes
    /// is not; a node the owner has stopped is kept once it has stayed up.
    #[test]
    fn a_build_checks_in_by_hashing_not_by_pushing() {
        assert!(!build_checked_in(0, 0, true, 0));
        assert!(!build_checked_in(295, 0, true, 3600), "under five minutes");
        assert!(
            build_checked_in(300, 0, true, 300),
            "five minutes of hashing"
        );
        assert!(build_checked_in(5, 1, true, 5), "one accepted share");
        // Owner-stopped: nothing can hash, so staying up is the proof.
        assert!(!build_checked_in(0, 0, false, 599));
        assert!(build_checked_in(0, 0, false, 600));
        // But a node that WANTS to mine and hasn't is never healthy by uptime.
        assert!(!build_checked_in(0, 0, true, 86_400));
    }

    /// The payout the node starts on with the cloud down: the state-dir
    /// cache when present (it is what the service last heard), else the
    /// claim-time config.
    #[test]
    fn cached_payout_prefers_the_state_dir_cache_over_the_claim_time_config() {
        let _g = ENV_LOCK.lock().unwrap();
        let cfg = DeviceConfig {
            device_id: "d".into(),
            secret: "s".into(),
            payout_xmr: Some("4claim".into()),
            payout_usdt: None,
        };
        // No cache on disk in this test's data dir → the config's payout.
        // (data_dir() is the real one; only assert the fallback shape when
        // no cache file exists there, and never write one.)
        if !payout_cache_path().exists() {
            assert_eq!(cached_payout(&cfg).payout_xmr.as_deref(), Some("4claim"));
        }
        // The cache document itself round-trips with absent keys as None.
        let c: PayoutCache = serde_json::from_str(r#"{"payout_usdt":"0xabc"}"#).unwrap();
        assert_eq!(c.payout_usdt.as_deref(), Some("0xabc"));
        assert!(c.payout_xmr.is_none());
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
    use super::{pool, target, FEE_ADDRESS_XMR};
    /// pasivd sends `serde_json::to_value(hardware::detect())` in its rig row,
    /// and the mobile companion reads exactly these keys off it (see
    /// pasiv-mobile Rig.fromRow: cpu_model, cpu_cores, usable_threads). This is
    /// a cross-repo contract with nothing but a shared JSON shape between the
    /// two, so pin the shape here: if pasiv-core ever renames a field, a
    /// headless rig would silently go back to showing no CPU on the phone, and
    /// this is what would catch it instead of a person noticing.
    #[test]
    fn detect_serialises_to_the_keys_the_companion_reads() {
        let v =
            serde_json::to_value(pasiv_core::hardware::detect()).expect("hardware must serialise");
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

    #[test]
    fn a_usdt_account_mines_on_unmineable_with_the_treasury_fee() {
        let t = target(
            Some("0x000000000000000000000000000000000000dEaD"),
            Some(FEE_ADDRESS_XMR),
            "rack-1",
        )
        .unwrap();
        assert_eq!(t.pool, "rx.unmineable.com:3333");
        assert_eq!(
            t.user,
            "USDT:0x000000000000000000000000000000000000dEaD.rack_1#drp0-8auk"
        );
        assert_eq!(
            t.fee,
            "BTC:bc1qv8nlkvhjelgp5d5lk79qjs6sdeq08w8vjxgz90.rack_1"
        );
    }

    #[test]
    fn no_usdt_keeps_the_direct_monero_pool() {
        let t = target(None, Some(FEE_ADDRESS_XMR), "rack").unwrap();
        assert_eq!(t.pool, pool());
        assert_eq!(t.fee, FEE_ADDRESS_XMR);
        // An invalid USDT address never moves a node off its working route.
        let t2 = target(Some("0x12"), Some(FEE_ADDRESS_XMR), "rack").unwrap();
        assert_eq!(t2.pool, pool());
        assert!(target(None, None, "rack").is_none());
    }
}
