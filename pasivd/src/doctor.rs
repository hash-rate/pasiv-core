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

    // perf: is the RandomX boost actually in effect? Huge pages and the MSR
    // preset are applied by the installer's ExecStartPre (pasivd-boost.sh), but
    // a locked-down kernel or a missing tool can silently skip either — and the
    // only symptom is a node quietly ~5-15% slow. Surface it so "why is this
    // node under-earning" has an answer without guessing. Linux-only, and a
    // WARN never a FAIL: the node still mines, just not at its ceiling.
    #[cfg(target_os = "linux")]
    {
        let (total, free) = read_hugepages();
        if total == 0 {
            report(
                "WARN",
                "perf.hugepages",
                "no huge pages reserved — RandomX loses a few percent; the installer's boost sets these".into(),
            );
        } else {
            report(
                "PASS",
                "perf.hugepages",
                format!("{total} reserved ({free} free)"),
            );
        }

        match msr_boost_state() {
            MsrState::Applied => report("PASS", "perf.msr", "RandomX MSR preset applied".into()),
            MsrState::LockedDown => report(
                "WARN",
                "perf.msr",
                "kernel lockdown (Secure Boot) blocks the MSR preset — disabling Secure Boot unlocks ~5-15% hashrate".into(),
            ),
            MsrState::NotApplied => report(
                "WARN",
                "perf.msr",
                "MSR preset not applied — the boost couldn't run (missing msr-tools, or not started via systemd)".into(),
            ),
            MsrState::Unknown => report(
                "PASS",
                "perf.msr",
                "MSR state not readable here (needs root); the boost runs under systemd".into(),
            ),
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

// ---------------------------------------------------------------- perf ----

/// HugePages_Total / HugePages_Free from /proc/meminfo.
#[cfg(target_os = "linux")]
fn read_hugepages() -> (u64, u64) {
    let raw = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    (
        parse_meminfo_field(&raw, "HugePages_Total"),
        parse_meminfo_field(&raw, "HugePages_Free"),
    )
}

/// Pure: one numeric field out of /proc/meminfo text. Zero when absent —
/// a kernel without hugepage support reports the same as none reserved,
/// which is the honest reading for a miner.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_meminfo_field(meminfo: &str, field: &str) -> u64 {
    meminfo
        .lines()
        .find_map(|l| {
            let (key, rest) = l.split_once(':')?;
            if key.trim() != field {
                return None;
            }
            rest.split_whitespace().next()?.parse().ok()
        })
        .unwrap_or(0)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum MsrState {
    Applied,
    NotApplied,
    LockedDown,
    Unknown,
}

/// Did the boot-time MSR preset actually land? Reads ONE marker register and
/// compares it with what pasivd-boost.sh writes there. Read-only — the doctor
/// diagnoses, the ExecStartPre fixes.
#[cfg(target_os = "linux")]
fn msr_boost_state() -> MsrState {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let Some((reg, expected, mask)) = msr_marker(&cpuinfo) else {
        return MsrState::Unknown;
    };
    use std::os::unix::fs::FileExt;
    let Ok(f) = std::fs::File::open("/dev/cpu/0/msr") else {
        // No module or no root: cannot tell from here. Lockdown is still worth
        // naming — it means the preset cannot be applied at all on this box.
        return if lockdown_active() {
            MsrState::LockedDown
        } else {
            MsrState::Unknown
        };
    };
    let mut buf = [0u8; 8];
    if f.read_exact_at(&mut buf, reg).is_err() {
        return if lockdown_active() {
            MsrState::LockedDown
        } else {
            MsrState::Unknown
        };
    }
    let v = u64::from_le_bytes(buf);
    if v & mask == expected & mask {
        MsrState::Applied
    } else if lockdown_active() {
        MsrState::LockedDown
    } else {
        MsrState::NotApplied
    }
}

#[cfg(target_os = "linux")]
fn lockdown_active() -> bool {
    std::fs::read_to_string("/sys/kernel/security/lockdown")
        .map(|s| s.contains("[integrity]") || s.contains("[confidentiality]"))
        .unwrap_or(false)
}

/// Pure: the ONE register that proves the preset for this CPU, the value
/// pasivd-boost.sh writes there, and the bits worth comparing. The register
/// table mirrors the installer's (which mirrors xmrig's randomx_boost.sh);
/// a test pins the two against each other so they cannot drift apart.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn msr_marker(cpuinfo: &str) -> Option<(u64, u64, u64)> {
    let field = |name: &str| -> Option<u64> {
        cpuinfo.lines().find_map(|l| {
            let (key, rest) = l.split_once(':')?;
            if key.trim() != name {
                return None;
            }
            rest.trim().parse().ok()
        })
    };
    if cpuinfo.contains("GenuineIntel") {
        // MISC_FEATURE_CONTROL — the boost sets the four prefetcher-disable
        // bits; other bits vary by SKU, so compare only those four.
        return Some((0x1a4, 0xf, 0xf));
    }
    if cpuinfo.contains("AuthenticAMD") {
        let fam = field("cpu family")?;
        let model = field("model").unwrap_or(0);
        let expected: u64 = match fam {
            25 if model == 97 || model == 117 => 0x2040cc10, // Zen4
            25 => 0x2000cc10,                                // Zen3
            26 => 0x2040cc10,                                // Zen5
            _ => 0x2000cc16,                                 // Zen1/Zen2
        };
        return Some((0xc001102b, expected, u64::MAX));
    }
    None
}

#[cfg(test)]
mod perf_tests {
    use super::*;

    #[test]
    fn meminfo_parsing_reads_the_exact_field_not_a_prefix() {
        let sample = "MemTotal: 32 kB\nHugePages_Total:    1188\nHugePages_Free:       5\nHugepagesize:    2048 kB\n";
        assert_eq!(parse_meminfo_field(sample, "HugePages_Total"), 1188);
        assert_eq!(parse_meminfo_field(sample, "HugePages_Free"), 5);
        // "HugePages" must not accidentally match "Hugepagesize" or vice versa.
        assert_eq!(parse_meminfo_field(sample, "HugePages"), 0);
    }

    #[test]
    fn the_marker_register_matches_the_cpu() {
        let intel =
            "vendor_id\t: GenuineIntel\nmodel name\t: 12th Gen Intel(R) Core(TM) i5-12400F\n";
        assert_eq!(msr_marker(intel), Some((0x1a4, 0xf, 0xf)));

        // "model name" must not satisfy a lookup for "model" — the classic
        // cpuinfo prefix trap.
        let zen3 = "vendor_id\t: AuthenticAMD\ncpu family\t: 25\nmodel\t\t: 33\nmodel name\t: AMD Ryzen 9 5950X\n";
        assert_eq!(msr_marker(zen3).unwrap().1, 0x2000cc10);

        let zen4 = "vendor_id\t: AuthenticAMD\ncpu family\t: 25\nmodel\t\t: 97\n";
        assert_eq!(msr_marker(zen4).unwrap().1, 0x2040cc10);

        let zen5 = "vendor_id\t: AuthenticAMD\ncpu family\t: 26\nmodel\t\t: 68\n";
        assert_eq!(msr_marker(zen5).unwrap().1, 0x2040cc10);

        let zen2 = "vendor_id\t: AuthenticAMD\ncpu family\t: 23\nmodel\t\t: 113\n";
        assert_eq!(msr_marker(zen2).unwrap().1, 0x2000cc16);

        assert_eq!(msr_marker("vendor_id\t: SomethingElse\n"), None);
    }

    /// The doctor's table and the installer's boost script describe the same
    /// hardware pokes in two languages. Nothing but this test connects them:
    /// change a value in one and this fails until the other moves too.
    #[test]
    fn the_installer_writes_every_value_the_doctor_expects() {
        let installer = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh"))
            .expect("pasivd/install.sh must sit beside this crate");
        for needle in [
            "0x1a4 0xf",  // Intel
            "0x2040cc10", // Zen4/Zen5 marker value
            "0x2000cc10", // Zen3
            "0x2000cc16", // Zen1/Zen2
            "0xc001102b", // the marker register itself
            "vm.nr_hugepages",
            "ExecStartPre=-+/usr/local/libexec/pasivd-boost.sh",
        ] {
            assert!(
                installer.contains(needle),
                "install.sh no longer contains {needle:?} — doctor and installer have drifted"
            );
        }
    }
}
