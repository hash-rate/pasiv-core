//! Signed self-update for the headless node.
//!
//! The installed binary (/usr/local/bin/pasivd) is read-only to the sandboxed
//! service (ProtectSystem=strict, DynamicUser), so an update never overwrites
//! it. It is STAGED in the node's own state directory beside its minisign
//! signature, and the installed binary acts as a launcher: on `pasivd run` it
//! re-verifies the staged copy and, if it is signed by the pinned key and
//! newer, execs it. Nothing unsigned is ever executed — not even to read its
//! version — and every start re-checks the signature, so a file swapped on
//! disk after the fact is refused, not trusted.
//!
//! Degrade to keep working: a staged build that never reaches its first
//! successful check-in is abandoned after [`MAX_PENDING_BOOTS`] starts, and the
//! installed binary carries on mining as before.
//!
//! Triggers: the owner's "update" command from the phone (never-list item 8:
//! an update installs only a release Pasiv signed), and a daily check.

use crate::data_dir;
use std::path::{Path, PathBuf};

/// The minisign public key every Pasiv release is signed with — the same key
/// the desktop updater and install.sh pin. A test holds install.sh to it.
///
/// `PASIVD_E2E_PUBKEY` / `PASIVD_E2E_RELEASE` are read at COMPILE time only, so
/// an end-to-end test build can sign its own fake release. The release
/// workflow never sets them; a shipped binary always carries the key below.
pub(crate) const PUBKEY: &str = match option_env!("PASIVD_E2E_PUBKEY") {
    Some(k) => k,
    None => "RWQJhawYO7igroqjh+CUPCstCmt4Ka2DAmznjX2e1gsScv3k5u7jYWR3",
};

const RELEASE: &str = match option_env!("PASIVD_E2E_RELEASE") {
    Some(r) => r,
    None => "https://github.com/hash-rate/pasiv-releases/releases/latest/download",
};
const BIN_ASSET: &str = "pasivd-linux-x64";

/// A staged build gets this many starts to check in once before the launcher
/// gives up on it and runs the installed binary instead.
pub(crate) const MAX_PENDING_BOOTS: u32 = 3;

/// Set on the staged process so it never tries to launch itself again.
const LAUNCHED_ENV: &str = "PASIVD_LAUNCHED";

/// A release binary is a static musl build of a few MB; anything far larger is
/// not ours and is refused before it is buffered.
const MAX_BIN_BYTES: usize = 64 * 1024 * 1024;

fn dir() -> PathBuf {
    data_dir().join("update")
}
fn staged_bin() -> PathBuf {
    dir().join("pasivd")
}
fn staged_sig() -> PathBuf {
    dir().join("pasivd.minisig")
}
fn pending_boots_path() -> PathBuf {
    dir().join("pending-boots")
}

/// `x.y.z` out of "pasivd x.y.z" (or a bare "x.y.z").
pub(crate) fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let v = raw.split_whitespace().find(|w| w.contains('.'))?;
    let mut it = v.split(['.', '-', '+']).map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

pub(crate) fn own_version() -> (u64, u64, u64) {
    parse_version(env!("CARGO_PKG_VERSION")).expect("crate version is x.y.z")
}

/// Does `sig_text` (a .minisig file) sign `bin` with the pinned key?
pub(crate) fn verify(bin: &[u8], sig_text: &str) -> Result<(), String> {
    verify_with(PUBKEY, bin, sig_text)
}

fn verify_with(pubkey: &str, bin: &[u8], sig_text: &str) -> Result<(), String> {
    let pk = minisign_verify::PublicKey::from_base64(pubkey).map_err(|e| e.to_string())?;
    let sig = minisign_verify::Signature::decode(sig_text).map_err(|e| e.to_string())?;
    pk.verify(bin, &sig, false)
        .map_err(|e| format!("signature check failed: {e}"))
}

/// Run a SIGNED binary's `--version`. Callers verify first.
fn version_of(path: &Path) -> Option<(u64, u64, u64)> {
    let out = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

fn read_pending_boots() -> u32 {
    std::fs::read_to_string(pending_boots_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn write_pending_boots(n: u32) {
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::write(pending_boots_path(), n.to_string());
}

/// Called once the running build has checked in successfully: it works, so
/// the launcher keeps choosing it.
pub(crate) fn mark_healthy() {
    if read_pending_boots() != 0 {
        write_pending_boots(0);
    }
}

fn discard_staged(why: &str) {
    eprintln!("update: discarding staged build ({why})");
    let _ = std::fs::remove_file(staged_bin());
    let _ = std::fs::remove_file(staged_sig());
    write_pending_boots(0);
}

/// What the launcher should do with a staged build. Pure, so the rules are
/// tested without touching disk.
#[derive(Debug, PartialEq)]
pub(crate) enum LaunchDecision {
    /// Exec the staged build.
    Exec,
    /// Throw the staged build away, then run the installed binary.
    Discard(&'static str),
}

pub(crate) fn decide(
    signed: bool,
    staged: Option<(u64, u64, u64)>,
    own: (u64, u64, u64),
    pending_boots: u32,
) -> LaunchDecision {
    if !signed {
        return LaunchDecision::Discard("signature does not verify");
    }
    let Some(v) = staged else {
        return LaunchDecision::Discard("staged build did not report a version");
    };
    if v <= own {
        // The installed binary caught up (a manual reinstall): it wins.
        return LaunchDecision::Discard("installed build is as new or newer");
    }
    if pending_boots >= MAX_PENDING_BOOTS {
        return LaunchDecision::Discard("it never checked in");
    }
    LaunchDecision::Exec
}

/// On `pasivd run`: exec a verified, newer staged build, or return and let the
/// installed binary run. Never fails the start — every error path falls back
/// to the installed binary.
pub(crate) fn launch_staged_if_any() {
    if std::env::var_os(LAUNCHED_ENV).is_some() || !staged_bin().exists() {
        return;
    }
    let signed = match (
        std::fs::read(staged_bin()),
        std::fs::read_to_string(staged_sig()),
    ) {
        (Ok(bin), Ok(sig)) => verify(&bin, &sig).is_ok(),
        _ => false,
    };
    let staged = if signed {
        version_of(&staged_bin())
    } else {
        None
    };
    let boots = read_pending_boots();
    match decide(signed, staged, own_version(), boots) {
        LaunchDecision::Discard(why) => discard_staged(why),
        LaunchDecision::Exec => {
            write_pending_boots(boots + 1);
            if let Some(v) = staged {
                println!(
                    "update: starting staged build {}.{}.{} (start {} of {MAX_PENDING_BOOTS})",
                    v.0,
                    v.1,
                    v.2,
                    boots + 1
                );
            }
            use std::os::unix::process::CommandExt;
            let args: Vec<String> = std::env::args().skip(1).collect();
            let err = std::process::Command::new(staged_bin())
                .args(&args)
                .env(LAUNCHED_ENV, "1")
                .exec();
            // exec only returns on failure.
            eprintln!(
                "update: could not start the staged build ({err}) — running the installed one"
            );
        }
    }
}

/// Download the latest release, verify it, and stage it if it is newer than
/// this build. `Ok(Some(version))` = staged, restart to run it; `Ok(None)` =
/// already up to date.
pub(crate) async fn fetch_and_stage(client: &reqwest::Client) -> Result<Option<String>, String> {
    let get = |url: String| async move {
        let r = client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        if r.content_length().unwrap_or(0) as usize > MAX_BIN_BYTES {
            return Err("download too large".to_string());
        }
        let b = r.bytes().await.map_err(|e| e.to_string())?;
        if b.len() > MAX_BIN_BYTES {
            return Err("download too large".to_string());
        }
        Ok::<_, String>(b)
    };
    let sig_bytes = get(format!("{RELEASE}/{BIN_ASSET}.minisig")).await?;
    let sig =
        String::from_utf8(sig_bytes.to_vec()).map_err(|_| "signature is not text".to_string())?;
    let bin = get(format!("{RELEASE}/{BIN_ASSET}")).await?;
    // Verify BEFORE the bytes touch an executable path.
    verify(&bin, &sig)?;

    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let partial = dir().join("pasivd.partial");
    std::fs::write(&partial, &bin).map_err(|e| e.to_string())?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    let Some(v) = version_of(&partial) else {
        let _ = std::fs::remove_file(&partial);
        return Err("the release binary did not run on this machine".into());
    };
    if v <= own_version() {
        let _ = std::fs::remove_file(&partial);
        return Ok(None);
    }
    std::fs::write(staged_sig(), sig).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, staged_bin()).map_err(|e| e.to_string())?;
    write_pending_boots(0);
    Ok(Some(format!("{}.{}.{}", v.0, v.1, v.2)))
}

/// `sudo pasivd update`: stage the latest signed release by hand. Files made
/// as root are handed to the owner of the state directory (the service user),
/// or the service could not keep its start count and would never abandon a
/// build that fails to check in.
pub(crate) async fn cmd_update() -> Result<(), String> {
    let client = reqwest::Client::new();
    let staged = fetch_and_stage(&client).await?;
    if let Ok(meta) = std::fs::metadata(data_dir()) {
        use std::os::unix::fs::MetadataExt;
        for p in [dir(), staged_bin(), staged_sig(), pending_boots_path()] {
            if p.exists() {
                let _ = std::os::unix::fs::chown(&p, Some(meta.uid()), Some(meta.gid()));
            }
        }
    }
    match staged {
        Some(v) => println!(
            "pasivd {v} verified and staged. Restart to run it:  sudo systemctl restart pasivd"
        ),
        None => println!("already up to date ({})", crate::VERSION),
    }
    Ok(())
}

/// One line for `pasivd doctor`.
pub(crate) fn describe() -> String {
    if !staged_bin().exists() {
        return "no update staged — running the installed build".into();
    }
    let signed = match (
        std::fs::read(staged_bin()),
        std::fs::read_to_string(staged_sig()),
    ) {
        (Ok(bin), Ok(sig)) => verify(&bin, &sig).is_ok(),
        _ => false,
    };
    if !signed {
        return "staged update does NOT verify — it will be discarded at the next start".into();
    }
    match version_of(&staged_bin()) {
        Some(v) => format!(
            "signed update {}.{}.{} staged (pending boots: {})",
            v.0,
            v.1,
            v.2,
            read_pending_boots()
        ),
        None => "staged update did not report a version".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_sh_pins_the_same_key() {
        let sh = include_str!("../install.sh");
        assert!(sh.contains(&format!("MINISIGN_PUB=\"{PUBKEY}\"")));
    }

    #[test]
    fn parses_what_the_binary_prints() {
        assert_eq!(parse_version("pasivd 0.1.6\n"), Some((0, 1, 6)));
        assert_eq!(parse_version("0.10.2"), Some((0, 10, 2)));
        assert_eq!(parse_version("pasivd 1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("pasivd"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn versions_compare_numerically_not_as_text() {
        assert!(parse_version("0.1.10") > parse_version("0.1.9"));
    }

    #[test]
    fn launcher_rules() {
        let own = (0, 1, 6);
        assert_eq!(decide(true, Some((0, 1, 7)), own, 0), LaunchDecision::Exec);
        assert_eq!(
            decide(true, Some((0, 1, 7)), own, MAX_PENDING_BOOTS - 1),
            LaunchDecision::Exec
        );
        assert!(matches!(
            decide(false, Some((0, 1, 7)), own, 0),
            LaunchDecision::Discard(_)
        ));
        assert!(matches!(
            decide(true, None, own, 0),
            LaunchDecision::Discard(_)
        ));
        // Never downgrade, never re-run the same build.
        assert!(matches!(
            decide(true, Some((0, 1, 6)), own, 0),
            LaunchDecision::Discard(_)
        ));
        assert!(matches!(
            decide(true, Some((0, 1, 5)), own, 0),
            LaunchDecision::Discard(_)
        ));
        // A build that never checks in is abandoned.
        assert!(matches!(
            decide(true, Some((0, 1, 7)), own, MAX_PENDING_BOOTS),
            LaunchDecision::Discard(_)
        ));
    }

    // A real minisign vector (a throwaway key made with `minisign -G -W`, then
    // `minisign -S` over the bytes "test"), so the verify path runs end to end.
    const TEST_PK: &str = "RWQ7DjmXN25KVX69hDx89WNXFOTvX75hNG8S6UIM7QkUTEMsfRbeEL2E";
    const TEST_SIG: &str = "untrusted comment: signature from minisign secret key
RUQ7DjmXN25KVf/LYZWNpAjhuifOZSorTeEuPCTtdJ1GXcKA0wnbfc7Ff5gm/brBZUcsRo4M17URTp/iebmw02GuHZfoIne3Jgw=
trusted comment: file:test
VKm9LVkOv3S0hqwo4d/4LFbk9ewF9vUmw65NbHT77kGnZG9dAzqJefU+mbYGKDYMkFQpGISrHqVTcazdT7xCDw==
";

    #[test]
    fn verifies_a_good_signature_and_refuses_a_tampered_file() {
        verify_with(TEST_PK, b"test", TEST_SIG).expect("known-good vector");
        assert!(verify_with(TEST_PK, b"tesT", TEST_SIG).is_err());
        // Signed by a different key than the pinned one: refused.
        assert!(verify(b"test", TEST_SIG).is_err());
    }
}

#[cfg(test)]
mod live_release {
    /// Run by hand: PASIVD_REL_BIN=… PASIVD_REL_SIG=… cargo test -p pasivd -- --ignored
    #[test]
    #[ignore]
    fn the_published_release_verifies() {
        let bin = std::fs::read(std::env::var("PASIVD_REL_BIN").unwrap()).unwrap();
        let sig = std::fs::read_to_string(std::env::var("PASIVD_REL_SIG").unwrap()).unwrap();
        super::verify(&bin, &sig).expect("the live release must verify");
        let mut tampered = bin.clone();
        tampered[1000] ^= 1;
        assert!(super::verify(&tampered, &sig).is_err());
    }
}
