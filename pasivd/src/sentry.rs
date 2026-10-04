// SPDX-License-Identifier: GPL-3.0-only
//! Crash reporting (Sentry) for the daemon — panics and nothing else.
//!
//! A headless node that panics is restarted by systemd and nobody learns
//! why. This reports the panic — its message, `file:line`, a backtrace and
//! three tags (`os`, `payout_route`, the daemon version as the release) —
//! and nothing that could name a person or a machine:
//!
//! * **Off switch, two ways** (docs/FEES.md never-list: telemetry you can
//!   switch off): `"telemetry": false` in the device config
//!   (`/etc/pasivd.json`), or `PASIVD_TELEMETRY=0` in the unit's
//!   environment. Either means no client is created at all.
//! * **Scrubbed at the boundary** — `crate::scrub` on every event: coin
//!   addresses, home paths, e-mails, IPs, MACs, hostnames, and this
//!   machine's own hostname and account name.
//! * **No identity, no session, no breadcrumbs, no tracing**:
//!   `send_default_pii: false`, `server_name` cleared, `user`/`request`
//!   cleared, `max_breadcrumbs: 0`, no session tracking (the `release-health`
//!   feature is off, so none exists), no traces.
//! * **Delivered even from a main-thread panic**: a hook chained after
//!   Sentry's flushes for up to three seconds before the process exits.

use std::borrow::Cow;
use std::path::Path;
use std::sync::{Arc, Once, OnceLock};
use std::time::Duration;

use sentry::protocol::Event;
use sentry::ClientInitGuard;

use crate::scrub::{scrub_event, Names};

/// The project's client key. Public by design (it is compiled into every
/// release); it can only *send* events to this project, never read anything.
const DSN: &str = "https://e24d4ccdaa18e8971ebe5d7efbb59003@o4511460191567872.ingest.de.sentry.io/4512194840559696";

/// `pasivd@<version>` — pasivd versions on its own 0.1.x track, apart from
/// the desktop's `pasiv@…` releases.
pub const RELEASE: &str = concat!("pasivd@", env!("CARGO_PKG_VERSION"));
pub const ENVIRONMENT: &str = "release";
/// `PASIVD_TELEMETRY=0` (or `false`/`off`/`no`) turns crash reports off.
pub const ENV_SWITCH: &str = "PASIVD_TELEMETRY";
/// The device-config key that turns crash reports off when `false`.
pub const CONFIG_KEY: &str = "telemetry";

const PANIC_FLUSH: Duration = Duration::from_secs(3);

static GUARD: OnceLock<ClientInitGuard> = OnceLock::new();

/// Pure: is crash reporting on, given the env switch and the raw device
/// config (if any)? The env wins; the config's `telemetry` key is next; the
/// default — no config yet (`pasivd claim` has not run), no key — is on.
pub fn enabled_by(env_switch: Option<&str>, config_json: Option<&str>) -> bool {
    if let Some(v) = env_switch {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "0" | "false" | "off" | "no") {
            return false;
        }
    }
    config_allows(config_json)
}

/// The config's say alone (what `pasivd claim` preserves on a re-claim).
pub fn config_allows(config_json: Option<&str>) -> bool {
    config_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| v.get(CONFIG_KEY).and_then(|t| t.as_bool()))
        .unwrap_or(true)
}

/// `config_allows` over the file at `path` (missing or unreadable = on).
pub fn config_file_allows(path: &Path) -> bool {
    config_allows(std::fs::read_to_string(path).ok().as_deref())
}

pub fn enabled() -> bool {
    enabled_by(
        std::env::var(ENV_SWITCH).ok().as_deref(),
        std::fs::read_to_string(crate::config_path())
            .ok()
            .as_deref(),
    )
}

/// The machine's own names, for the scrubber. Computed once.
fn own_names() -> &'static Names {
    static NAMES: OnceLock<Names> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut hosts = Vec::new();
        if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
            let h = h.trim().to_string();
            if let Some(first) = h.split('.').next() {
                hosts.push(first.to_string());
            }
            hosts.push(h);
        }
        let mut users = Vec::new();
        for var in ["USER", "LOGNAME"] {
            if let Ok(u) = std::env::var(var) {
                users.push(u);
            }
        }
        if let Some(leaf) = std::env::var("HOME").ok().and_then(|h| {
            Path::new(&h)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        }) {
            users.push(leaf);
        }
        Names { hosts, users }
    })
}

/// The gate every event passes. Pure over its inputs.
pub fn before_send_in(names: &Names, mut event: Event<'static>) -> Option<Event<'static>> {
    scrub_event(&mut event, names);
    Some(event)
}

fn before_send(event: Event<'static>) -> Option<Event<'static>> {
    before_send_in(own_names(), event)
}

/// Initialise crash reporting, once, before any command runs. A no-op when
/// the switch is off.
pub fn init() {
    if GUARD.get().is_some() || !enabled() {
        return;
    }
    let Ok(dsn) = DSN.parse() else {
        return;
    };
    // Field assignment, not a struct literal, so a `#[non_exhaustive]`
    // `ClientOptions` (0.49+) needs no change here.
    let mut options = sentry::ClientOptions::new();
    options.dsn = Some(dsn);
    options.release = Some(Cow::Borrowed(RELEASE));
    options.environment = Some(Cow::Borrowed(ENVIRONMENT));
    options.sample_rate = 1.0;
    options.traces_sample_rate = 0.0;
    options.max_breadcrumbs = 0;
    options.attach_stacktrace = true;
    options.send_default_pii = false;
    options.before_send = Some(Arc::new(before_send));
    options.before_breadcrumb = Some(Arc::new(|_| None));
    options.user_agent = Cow::Borrowed(concat!("pasivd/", env!("CARGO_PKG_VERSION")));
    let guard = sentry::init(options);
    sentry::configure_scope(|s| s.set_tag("os", "linux"));
    install_flush_hook();
    let _ = GUARD.set(guard);
}

/// `usdt` (unMineable) or `direct`, once the mining target is known.
pub fn set_route(unmineable: bool) {
    sentry::configure_scope(|s| {
        s.set_tag("payout_route", if unmineable { "usdt" } else { "direct" })
    });
}

/// Sentry's own hook captures the panic on a background transport; a panic
/// that unwinds `main` would exit before it sent. Ours runs first (installed
/// last), calls Sentry's, then flushes.
fn install_flush_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let next = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            next(info);
            if let Some(client) = sentry::Hub::current().client() {
                client.flush(Some(PANIC_FLUSH));
            }
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_by_default_and_off_two_ways() {
        assert!(enabled_by(None, None), "no env, no config: on");
        assert!(enabled_by(None, Some(r#"{"device_id":"d","secret":"s"}"#)));
        assert!(enabled_by(None, Some(r#"{"telemetry":true}"#)));
        assert!(!enabled_by(None, Some(r#"{"telemetry":false}"#)));
        assert!(!enabled_by(Some("0"), None));
        assert!(!enabled_by(Some("false"), Some(r#"{"telemetry":true}"#)));
        assert!(!enabled_by(Some("OFF"), None));
        assert!(enabled_by(Some("1"), Some(r#"{"telemetry":true}"#)));
        // Junk never turns it on past a config that says no, nor off by itself.
        assert!(enabled_by(Some("maybe"), None));
        assert!(!enabled_by(Some("maybe"), Some(r#"{"telemetry":false}"#)));
        assert!(enabled_by(None, Some("not json")), "unreadable config: on");
        assert!(config_file_allows(Path::new("/nonexistent/pasivd.json")));
    }

    #[test]
    fn before_send_scrubs_and_strips_identity() {
        let e = Event {
            message: Some("panicked at /home/jane/x.rs: 192.168.1.1".into()),
            server_name: Some("rack.lan".into()),
            ..Default::default()
        };
        let out = before_send_in(&Names::default(), e).expect("sent");
        assert_eq!(
            out.message.as_deref(),
            Some("panicked at /home/<user>/x.rs: <ip>")
        );
        assert_eq!(out.server_name, None);
    }

    #[test]
    fn the_release_is_fixed() {
        assert_eq!(RELEASE, concat!("pasivd@", env!("CARGO_PKG_VERSION")));
        assert!(DSN.starts_with("https://") && DSN.contains("sentry.io"));
    }
}
