// SPDX-License-Identifier: GPL-3.0-only
//! The XMRig local-API contract — every DECISION about how Pasiv drives its
//! bundled XMRig, shared by both consumers (the desktop adapter and pasivd).
//!
//! The two used to carry independent copies of this and they drifted: pasivd
//! 0.1.0 launched xmrig without an unrestricted API, the fee scheduler's
//! config calls 403'd, the fail-safe stopped mining, and a fresh node earned
//! nothing. With the `http` block, the URLs, and the parsers defined once
//! here, that class of bug has nowhere to live.
//!
//! Deliberately I/O-light: this module builds values and parses responses;
//! the consumers own their HTTP clients and process spawning. The only I/O is
//! [`write_runtime_config`] (filesystem), because the 0600-before-token-write
//! discipline is itself a decision that must not drift.

use crate::types::{DeviceStat, MinerStats};

/// XMRig's `/2/summary` endpoint for a local API on `port`.
pub fn summary_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/2/summary")
}

/// XMRig's `/1/config` endpoint — the one the payout hot-swap reads AND
/// writes, and therefore the one the level-triggered fee reconcile reads back.
pub fn config_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/1/config")
}

/// The `http` block XMRig should serve.
///
/// `restricted: false` is load-bearing: restricted mode 403s `/1/config` and
/// `/2/config` even to an authenticated caller, which breaks the payout
/// hot-swap and trips the fee fail-safe (pasivd's 0.1.0 → 0.1.1 bug). The
/// token gates the API to the loopback caller that knows it; the API can
/// rewrite the payout address, so the token is a real secret — see
/// [`write_runtime_config`] for how it must reach XMRig.
pub fn runtime_config(token: &str, port: u16) -> serde_json::Value {
    serde_json::json!({
        "http": {
            "enabled": true,
            "host": "127.0.0.1",
            "port": port,
            "access-token": token,
            "restricted": false,
        }
    })
}

/// Write the runtime config readable only by this user, for `-c`.
///
/// Process argv is world-readable (`ps`, `/proc/<pid>/cmdline`), so the token
/// must never travel there — both consumers pass this file with `-c` instead.
/// Created 0600 *before* the token is written, and unlinked first so a
/// pre-existing file with looser permissions can't be inherited — otherwise
/// the fix would just move the secret from `ps` to a world-readable file.
pub fn write_runtime_config(path: &std::path::Path, token: &str, port: u16) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let mut opts = std::fs::OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(runtime_config(token, port).to_string().as_bytes())
}

/// Normalise XMRig's `/2/summary` into [`MinerStats`].
///
/// Every field is read defensively: a miner that changes shape should cost us
/// a stat, never a panic in a stats loop. `hashrate.total` is `[10s, 60s,
/// 15m]`; the 10s figure is what surfaces show. `shares_total` counts every
/// submission, good or not.
pub fn parse_summary(v: &serde_json::Value) -> MinerStats {
    let hashrate = v["hashrate"]["total"][0].as_f64().unwrap_or(0.0);
    let accepted = v["results"]["shares_good"].as_u64().unwrap_or(0);
    let total = v["results"]["shares_total"].as_u64().unwrap_or(0);
    MinerStats {
        hashrate,
        accepted,
        rejected: total.saturating_sub(accepted),
        hottest_c: None,
        devices: vec![DeviceStat {
            name: "CPU".into(),
            hashrate,
        }],
    }
}

/// The primary pool's login (`pools[0].user`) from a `/1/config` response.
///
/// Kept for callers that only ever configure one pool. With a backup pool
/// present this is NOT the ground truth on its own — XMRig fails over
/// silently, and the login on the pool it fell back to is what earns — so
/// the fee reconcile should use [`all_pools_use`] instead, which is exact
/// whichever entry is live.
pub fn user_from_config(v: &serde_json::Value) -> Option<String> {
    Some(v.get("pools")?.get(0)?.get("user")?.as_str()?.to_string())
}

/// Every pool login in a `/1/config` response, in `pools` order. Empty when
/// the document has no pools. A pool entry without a `user` is skipped, so
/// the length can be shorter than `pools` — compare with [`all_pools_use`]
/// rather than counting.
pub fn users_from_config(v: &serde_json::Value) -> Vec<String> {
    v.get("pools")
        .and_then(|p| p.as_array())
        .map(|pools| {
            pools
                .iter()
                .filter_map(|p| p.get("user")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Is EVERY pool in `config` logged in as `user`? This is what the
/// level-triggered fee reconcile asks: "is the miner, on whichever pool it
/// is currently connected to, paying the side I want?" — true only when all
/// entries agree, false for a config with no pools (nothing is mining to
/// `user`) or with any entry missing a `user`.
pub fn all_pools_use(config: &serde_json::Value, user: &str) -> bool {
    match config.get("pools").and_then(|p| p.as_array()) {
        Some(pools) if !pools.is_empty() => pools
            .iter()
            .all(|p| p.get("user").and_then(|u| u.as_str()) == Some(user)),
        _ => false,
    }
}

/// A `/1/config` document with EVERY pool's login replaced — the body the
/// payout hot-swap PUTs back. All entries, not just `pools[0]`: XMRig fails
/// over to a backup pool on its own and keeps that pool's login, so a swap
/// that touched only the primary would leave a failed-over miner paying the
/// wrong side for the rest of the session, and the reconcile reading
/// `pools[0]` back would never notice. XMRig re-logins to the same pool
/// (algo unchanged, RandomX dataset kept), so the cost is a sub-second
/// reconnect. Nothing but `user` is touched on any entry.
pub fn config_with_user(
    mut config: serde_json::Value,
    address: &str,
) -> Result<serde_json::Value, &'static str> {
    match config.get_mut("pools").and_then(|p| p.as_array_mut()) {
        Some(pools) if !pools.is_empty() => {
            for pool in pools.iter_mut() {
                pool["user"] = serde_json::Value::String(address.to_string());
            }
            Ok(config)
        }
        _ => Err("xmrig config has no pool"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_is_unrestricted_loopback_and_carries_the_token() {
        let v = runtime_config("tok123", 42_170);
        assert_eq!(v["http"]["enabled"], true);
        assert_eq!(
            v["http"]["restricted"], false,
            "restricted mode 403s the fee swap — the 0.1.0 bug"
        );
        assert_eq!(v["http"]["access-token"], "tok123");
        assert_eq!(v["http"]["host"], "127.0.0.1");
        assert_eq!(v["http"]["port"], 42_170);
    }

    #[test]
    fn written_runtime_config_is_owner_only() {
        let dir = std::env::temp_dir().join(format!("pasiv-core-xmrig-{}", std::process::id()));
        let path = dir.join("xmrig-runtime.json");
        write_runtime_config(&path, "tok123", 1).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["http"]["access-token"], "tok123");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the token file must be owner-only");
        }
        // And a looser pre-existing file must not survive a rewrite.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            write_runtime_config(&path, "tok456", 1).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summary_parses_the_live_shape_and_survives_garbage() {
        let live: serde_json::Value = serde_json::json!({
            "hashrate": { "total": [123.4, 120.0, 118.0] },
            "results": { "shares_good": 7, "shares_total": 9 }
        });
        let s = parse_summary(&live);
        assert_eq!(s.hashrate, 123.4);
        assert_eq!((s.accepted, s.rejected), (7, 2));

        let garbage = serde_json::json!({ "unexpected": true });
        let s = parse_summary(&garbage);
        assert_eq!((s.hashrate, s.accepted, s.rejected), (0.0, 0, 0));
    }

    #[test]
    fn config_user_round_trips() {
        let cfg = serde_json::json!({ "pools": [{ "user": "4old", "url": "p:1" }] });
        assert_eq!(user_from_config(&cfg).as_deref(), Some("4old"));
        let swapped = config_with_user(cfg, "4new").unwrap();
        assert_eq!(user_from_config(&swapped).as_deref(), Some("4new"));
        assert_eq!(swapped["pools"][0]["url"], "p:1", "everything else kept");
        assert!(config_with_user(serde_json::json!({}), "4new").is_err());
        assert!(config_with_user(serde_json::json!({ "pools": [] }), "4new").is_err());
    }

    /// With a backup pool configured, XMRig fails over silently and keeps
    /// THAT pool's login — so the swap must rewrite every entry, and the
    /// reconcile must ask "do all pools agree?" rather than read `pools[0]`.
    #[test]
    fn the_swap_covers_every_pool_and_the_reconcile_checks_all_of_them() {
        let cfg = serde_json::json!({ "pools": [
            { "user": "4old", "url": "primary:1", "pass": "x" },
            { "user": "4old", "url": "backup:2", "tls": true },
        ]});
        assert_eq!(users_from_config(&cfg), vec!["4old", "4old"]);
        assert!(all_pools_use(&cfg, "4old"));
        assert!(!all_pools_use(&cfg, "4new"));

        let swapped = config_with_user(cfg, "4new").unwrap();
        assert_eq!(users_from_config(&swapped), vec!["4new", "4new"]);
        assert!(all_pools_use(&swapped, "4new"));
        // pools[0] still answers the single-pool question the desktop asks.
        assert_eq!(user_from_config(&swapped).as_deref(), Some("4new"));
        // Only `user` moved; every other field on every entry is intact.
        assert_eq!(swapped["pools"][0]["url"], "primary:1");
        assert_eq!(swapped["pools"][0]["pass"], "x");
        assert_eq!(swapped["pools"][1]["url"], "backup:2");
        assert_eq!(swapped["pools"][1]["tls"], true);

        // A half-swapped config (the failure the old pools[0]-only swap could
        // leave behind) is NOT "on the user": the reconcile must correct it.
        let half = serde_json::json!({ "pools": [
            { "user": "4new", "url": "primary:1" },
            { "user": "4old", "url": "backup:2" },
        ]});
        assert!(!all_pools_use(&half, "4new"));
        assert!(!all_pools_use(&half, "4old"));
        // And a pool entry with no login at all can't be said to pay anyone.
        let missing = serde_json::json!({ "pools": [{ "url": "p:1" }] });
        assert!(!all_pools_use(&missing, "4new"));
        assert!(users_from_config(&missing).is_empty());
        assert!(!all_pools_use(&serde_json::json!({ "pools": [] }), "4new"));
        assert!(!all_pools_use(&serde_json::json!({}), "4new"));
    }

    #[test]
    fn urls_are_loopback_only() {
        assert_eq!(summary_url(7), "http://127.0.0.1:7/2/summary");
        assert_eq!(config_url(7), "http://127.0.0.1:7/1/config");
    }
}
