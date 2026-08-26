//! `pasivd doctor` — one PASS/WARN/FAIL pass over everything a headless node
//! needs. Split from main.rs 2026-08-26; behaviour unchanged.

use crate::{api, config_path, data_dir, fee_ledger_path, pool, DeviceConfig, XMRIG_BIN_SHA256};
use pasiv_core::address::is_valid_xmr_address;
use pasiv_core::fee;
use sha2::Digest;

/// One diagnostic pass, greppable output (`PASS|WARN|FAIL <id> — <detail>`),
/// exit 1 iff any FAIL — systemd/cron friendly. Self-contained on purpose:
/// pasivd shares no code with the desktop (the tolerated-drift pattern this
/// file already uses for the fee engine), so these checks mirror the desktop
/// doctor's SHAPE, not its source. Adopted from an internal provider-audit
/// checklist (2026-08-17).
pub async fn cmd_doctor() -> Result<(), String> {
    let mut failed = false;
    let mut report = |status: &str, id: &str, detail: String| {
        if status == "FAIL" {
            failed = true;
        }
        println!("{status} {id} — {detail}");
    };

    // config: present, parseable, secret not world-readable.
    let path = config_path();
    let cfg: Option<DeviceConfig> = match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<DeviceConfig>(&raw) {
            Ok(c) => {
                report("PASS", "config", format!("{} loads", path.display()));
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let mode = meta.permissions().mode() & 0o777;
                        if mode & 0o077 != 0 {
                            report(
                                "WARN",
                                "config.perms",
                                format!(
                                    "{path:?} is 0o{mode:o} — the device secret should be 0600",
                                    path = path
                                ),
                            );
                        } else {
                            report("PASS", "config.perms", format!("0o{mode:o}"));
                        }
                    }
                }
                Some(c)
            }
            Err(e) => {
                report("FAIL", "config", format!("{}: {e}", path.display()));
                None
            }
        },
        Err(_) => {
            report(
                "WARN",
                "config",
                format!("{} missing — run `pasivd claim` first", path.display()),
            );
            None
        }
    };

    // payout: the address every share pays to.
    match cfg.as_ref().and_then(|c| c.payout_xmr.as_deref()) {
        Some(a) if is_valid_xmr_address(a) => {
            report("PASS", "payout", "XMR address shape ok".into())
        }
        Some(_) => report(
            "FAIL",
            "payout",
            "saved XMR address has the wrong shape".into(),
        ),
        None => report(
            "WARN",
            "payout",
            "no payout set — approve the claim in the companion to receive one".into(),
        ),
    }

    // miner binary: present + still matching the pin (a drifted cache is the
    // desktop's v0.4.28 bug class).
    let bin = data_dir().join("xmrig");
    if bin.exists() {
        match std::fs::read(&bin) {
            Ok(bytes) => {
                let got = hex::encode(sha2::Sha256::digest(&bytes));
                if got == XMRIG_BIN_SHA256 {
                    report("PASS", "binary", "xmrig matches the pinned sha256".into());
                } else {
                    report(
                        "FAIL",
                        "binary",
                        format!("xmrig sha256 {got} != pinned — will be re-fetched on next run"),
                    );
                }
            }
            Err(e) => report("FAIL", "binary", format!("xmrig unreadable: {e}")),
        }
    } else {
        report(
            "WARN",
            "binary",
            "xmrig not fetched yet — `pasivd run` provisions it".into(),
        );
    }

    // pool: TCP reach on the stratum port (a Cloudflare-parked host connects
    // on 443 and never here — the desktop's AlphaPool trap).
    {
        use std::net::{TcpStream, ToSocketAddrs};
        let started = std::time::Instant::now();
        let pool = pool();
        let ok = pool
            .to_socket_addrs()
            .ok()
            .into_iter()
            .flatten()
            .any(|a| TcpStream::connect_timeout(&a, std::time::Duration::from_secs(3)).is_ok());
        if ok {
            report(
                "PASS",
                "pool.stratum",
                format!("{pool} in {}ms", started.elapsed().as_millis()),
            );
        } else {
            report("FAIL", "pool.stratum", format!("{pool} unreachable"));
        }
    }

    // fee ledger: readable + parse count (same JSONL the desktop writes).
    let ledger = fee_ledger_path();
    match std::fs::read_to_string(&ledger) {
        Ok(raw) => {
            let total = raw.lines().filter(|l| !l.trim().is_empty()).count();
            let parsed = fee::parse_ledger(&raw).len();
            if parsed == total {
                report("PASS", "fee.ledger", format!("{parsed} events"));
            } else {
                report(
                    "WARN",
                    "fee.ledger",
                    format!("{parsed}/{total} lines parse — the rest are garbled"),
                );
            }
        }
        Err(_) => report("PASS", "fee.ledger", "no ledger yet (no fee slices)".into()),
    }

    // cloud: one real poll with the device's own credentials — the same call
    // `run` lives on. Offline is a WARN, not a FAIL: mining continues without
    // the uplink; only stale companion data results.
    if let Some(cfg) = &cfg {
        let client = reqwest::Client::new();
        match api(
            &client,
            serde_json::json!({"action":"poll","device_id":cfg.device_id,"secret":cfg.secret}),
        )
        .await
        {
            Ok(_) => report(
                "PASS",
                "cloud",
                "poll ok — device is claimed and linked".into(),
            ),
            Err(e) => report("WARN", "cloud", format!("poll failed: {e}")),
        }
    }

    if failed {
        std::process::exit(1);
    }
    Ok(())
}
