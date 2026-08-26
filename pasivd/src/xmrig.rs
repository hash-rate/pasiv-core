//! The daemon's XMRig plumbing: fetch-and-verify the pinned binary, spawn it
//! with the 0600 runtime config, and thin HTTP wrappers over the shared
//! contract in `pasiv_core::xmrig` (which owns every decision — URLs, the
//! http block, parsers). Split from main.rs 2026-08-26; behaviour unchanged.

use crate::{
    data_dir, pool, HTTP_PORT, XMRIG_BIN_SHA256, XMRIG_DIR_IN_TAR, XMRIG_SHA256, XMRIG_URL,
};
use sha2::Digest;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

pub async fn ensure_xmrig(client: &reqwest::Client) -> Result<PathBuf, String> {
    let bin = data_dir().join("xmrig");
    // Verify on EVERY start, not just first install. Checking only `exists()`
    // meant the pin ran once in a machine's lifetime: a tampered or truncated
    // binary was then trusted forever, and bumping XMRIG_SHA256 in a new
    // release was a no-op on every existing node — we could never ship an
    // xmrig security update.
    if bin.exists() {
        match std::fs::read(&bin) {
            Ok(bytes) if hex::encode(sha2::Sha256::digest(&bytes)) == XMRIG_BIN_SHA256 => {
                return Ok(bin)
            }
            Ok(_) => {
                eprintln!("xmrig on disk failed verification — refetching");
                let _ = std::fs::remove_file(&bin);
            }
            Err(e) => {
                eprintln!("could not read cached xmrig ({e}) — refetching");
                let _ = std::fs::remove_file(&bin);
            }
        }
    }
    println!("fetching xmrig (sha256-verified)…");
    let bytes = client
        .get(XMRIG_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let digest = hex::encode(sha2::Sha256::digest(&bytes));
    if digest != XMRIG_SHA256 {
        return Err(format!("xmrig checksum mismatch: {digest}"));
    }
    let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(&bytes[..]));
    let mut ar = tar::Archive::new(gz);
    let want = format!("{XMRIG_DIR_IN_TAR}/xmrig");
    // Unpack to a temp path and rename into place: writing straight to `bin`
    // meant a crash mid-unpack left a truncated binary that the old
    // exists()-only check would then execute forever.
    let partial = bin.with_extension("partial");
    let _ = std::fs::remove_file(&partial);
    for entry in ar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        // Only ever unpack a regular file. A symlink entry would otherwise be
        // created at our destination and the chmod/exec would follow it.
        if !entry.header().entry_type().is_file() {
            continue;
        }
        if entry.path().map_err(|e| e.to_string())?.to_string_lossy() == want {
            entry.unpack(&partial).map_err(|e| e.to_string())?;
        }
    }
    if !partial.exists() {
        return Err("xmrig not found in archive".into());
    }
    let got = hex::encode(sha2::Sha256::digest(
        std::fs::read(&partial).map_err(|e| e.to_string())?,
    ));
    if got != XMRIG_BIN_SHA256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("extracted xmrig checksum mismatch: {got}"));
    }
    std::fs::rename(&partial, &bin).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
    }
    Ok(bin)
}

pub struct Miner {
    pub child: tokio::process::Child,
    pub token: String,
}

/// The exact xmrig command line for a headless node. Pure and unit-tested on
/// purpose: a missing flag here fails silently at runtime, not at build. Only
/// the pool, the payout, and the runtime-config path travel in argv — the
/// whole `http` block (with its token) is in the 0600 file behind `-c`.
pub fn xmrig_args(payout: &str, runtime_config: &str) -> Vec<String> {
    vec![
        "-o".into(),
        pool(),
        "-u".into(),
        payout.into(),
        "-p".into(),
        "pasiv".into(),
        "-k".into(),
        "--donate-level".into(),
        "1".into(),
        "--no-color".into(),
        "-c".into(),
        runtime_config.into(),
    ]
}

pub fn spawn_xmrig(
    bin: &PathBuf,
    payout: &str,
    token: &str,
) -> Result<tokio::process::Child, String> {
    let runtime = data_dir().join("xmrig-runtime.json");
    // The http block (restricted:false, loopback, the token) is built and
    // written 0600 by the SHARED contract — pasiv_core::xmrig — so it cannot
    // drift from the desktop again.
    pasiv_core::xmrig::write_runtime_config(&runtime, token, HTTP_PORT)
        .map_err(|e| format!("write {}: {e}", runtime.display()))?;
    tokio::process::Command::new(bin)
        .args(xmrig_args(payout, &runtime.to_string_lossy()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn xmrig: {e}"))
}

pub async fn xmrig_summary(client: &reqwest::Client, token: &str) -> Option<serde_json::Value> {
    client
        .get(pasiv_core::xmrig::summary_url(HTTP_PORT))
        .bearer_auth(token)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()
}

/// What address xmrig is ACTUALLY logged in with right now. The reconciler
/// compares against this rather than a local boolean, so a swap that silently
/// didn't apply — or an outside party rewriting the config through the same
/// local API — gets corrected on the next tick instead of persisting.
pub async fn xmrig_current_user(client: &reqwest::Client, token: &str) -> Option<String> {
    let cfg: serde_json::Value = client
        .get(pasiv_core::xmrig::config_url(HTTP_PORT))
        .bearer_auth(token)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    pasiv_core::xmrig::user_from_config(&cfg)
}

/// Swap the pool login (fee slice ↔ payout) via config hot-reload — identical
/// mechanism to the desktop: the RandomX dataset derives from the chain seed,
/// not the login, so this costs nothing.
pub async fn xmrig_set_user(client: &reqwest::Client, token: &str, user: &str) -> bool {
    let Ok(resp) = client
        .get(pasiv_core::xmrig::config_url(HTTP_PORT))
        .bearer_auth(token)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return false;
    };
    let Ok(cfg) = resp.json::<serde_json::Value>().await else {
        return false;
    };
    let Ok(cfg) = pasiv_core::xmrig::config_with_user(cfg, user) else {
        return false;
    };
    client
        .put(pasiv_core::xmrig::config_url(HTTP_PORT))
        .bearer_auth(token)
        .json(&cfg)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
