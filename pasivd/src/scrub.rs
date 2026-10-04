// SPDX-License-Identifier: GPL-3.0-only
//! The scrubber every crash report passes through before it leaves the node.
//!
//! A copy of the desktop app's scrubber (its `telemetry/sentry.rs`), kept
//! self-contained here because this repo is public and the desktop is not.
//! The two are pinned to ONE table of cases, `tests/contracts/scrub.json`,
//! loaded by both test suites (and by the desktop webview's TypeScript
//! twin), so a rule added on one side without its row fails the other.
//!
//! [`scrub`] is pure over text: coin addresses of every shape Pasiv touches,
//! home-directory segments, e-mails, long hex, MACs, IPs and generated
//! hostnames become `<token>`s. [`scrub_names`] adds the running machine's
//! own hostname and account name as whole words. [`scrub_event`] applies
//! both to every free-text field of a Sentry event and removes everything
//! identity-shaped outright.
//!
//! Order matters and is part of the contract: paths before addresses (a
//! path segment can look like a base58 run), e-mails before addresses, the
//! specific address shapes before the generic base58 run, the 64-hex rule
//! before base58, IPv4 before IPv6 (`::ffff:1.2.3.4`), and the `.local`
//! hostname rule before the Apple-style one.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::{Captures, Regex};
use sentry::protocol::{Context, DebugImage, Event, Stacktrace, Value};

/// `C:\Users\<name>\…` (either slash, doubled or not; the name may hold spaces).
const WIN_USER: &str = r#"(?i)([a-z]:[\\/]+Users[\\/]+)[^\\/\n"'<>|:*?]+"#;
/// `/Users/<name>/…` and `/home/<name>/…`.
const UNIX_USER: &str = r#"(/Users/|/home/)[^/\s"':]+"#;
const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
/// EVM: the sign-in identity and the USDT (BSC) payout.
const EVM: &str = r"\b0x[0-9a-fA-F]{40}\b";
/// bech32 (BTC native segwit and friends) — its alphabet is not base58's.
const BECH32: &str = r"\b(?:bc|tb|ltc)1[02-9ac-hj-np-z]{25,}\b";
/// Exactly 64 or 128 hex, `0x`-prefixed or not: a tx hash, a key, a
/// checksum, a signature. Exact lengths, not "64+": a 95-digit XMR address
/// is hex-shaped too and must reach the base58 rule as an address.
const HEX64: &str = r"\b(?:0x)?(?:[0-9a-fA-F]{64}){1,2}\b";
/// base58 runs of 26+: XMR (95/106), ZEPH, SAL, Verus/BTC legacy (34),
/// Tron, Solana — every coin address Pasiv can hold that isn't EVM/bech32.
const BASE58: &str = r"\b[1-9A-HJ-NP-Za-km-z]{26,}\b";
const MAC: &str = r"\b(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}\b";
/// Loopback and the unspecified address stay: `127.0.0.1:42999` is the
/// miner API and identifies nobody.
const IPV4: &str = r"\b(?:\d{1,3}\.){3}\d{1,3}\b";
/// Full form, compressed form, and a leading `::`. Candidates without a
/// digit are kept: `fee::add` and `std::fs` are Rust paths, not addresses.
const IPV6: &str = r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b|\b(?:[0-9a-fA-F]{1,4}:){1,6}:(?:[0-9a-fA-F]{1,4}(?::[0-9a-fA-F]{1,4}){0,5})?\b|::(?:[0-9a-fA-F]{1,4}(?::[0-9a-fA-F]{1,4}){0,6})\b";
const HOST_SUFFIX: &str = r"(?i)\b[a-z0-9][a-z0-9-]*\.(?:local|lan|home|localdomain|internal)\b";
/// Windows' generated computer names.
const HOST_WINDOWS: &str = r"\b(?:DESKTOP|LAPTOP|WIN)-[A-Z0-9]{5,8}\b";
/// macOS' generated computer names (`janes-macbook-pro`).
const HOST_APPLE: &str = r"(?i)\b[a-z0-9]+(?:-[a-z0-9]+)*-(?:macbook(?:-pro|-air)?|mac-studio|mac-mini|mac-pro|imac)(?:-[a-z0-9]+)*\b";

pub const T_USER: &str = "<user>";
pub const T_EMAIL: &str = "<email>";
pub const T_ADDRESS: &str = "<address>";
pub const T_HEX: &str = "<hex>";
pub const T_MAC: &str = "<mac>";
pub const T_IP: &str = "<ip>";
pub const T_HOST: &str = "<host>";

struct Rules {
    win_user: Regex,
    unix_user: Regex,
    email: Regex,
    evm: Regex,
    bech32: Regex,
    hex64: Regex,
    base58: Regex,
    mac: Regex,
    ipv4: Regex,
    ipv6: Regex,
    host_suffix: Regex,
    host_windows: Regex,
    host_apple: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let re = |p: &str| Regex::new(p).expect("scrub pattern compiles");
        Rules {
            win_user: re(WIN_USER),
            unix_user: re(UNIX_USER),
            email: re(EMAIL),
            evm: re(EVM),
            bech32: re(BECH32),
            hex64: re(HEX64),
            base58: re(BASE58),
            mac: re(MAC),
            ipv4: re(IPV4),
            ipv6: re(IPV6),
            host_suffix: re(HOST_SUFFIX),
            host_windows: re(HOST_WINDOWS),
            host_apple: re(HOST_APPLE),
        }
    })
}

/// Pure: the text with every address, user path, e-mail, long hex, MAC, IP
/// and generated hostname replaced by a `<token>`. Idempotent.
pub fn scrub(text: &str) -> String {
    let r = rules();
    let s = r
        .win_user
        .replace_all(text, |c: &Captures| format!("{}{}", &c[1], T_USER));
    let s = r
        .unix_user
        .replace_all(&s, |c: &Captures| format!("{}{}", &c[1], T_USER));
    let s = r.email.replace_all(&s, T_EMAIL);
    let s = r.evm.replace_all(&s, T_ADDRESS);
    let s = r.bech32.replace_all(&s, T_ADDRESS);
    let s = r.hex64.replace_all(&s, T_HEX);
    let s = r.base58.replace_all(&s, T_ADDRESS);
    let s = r.mac.replace_all(&s, T_MAC);
    let s = r.ipv4.replace_all(&s, |c: &Captures| match &c[0] {
        "127.0.0.1" | "0.0.0.0" => c[0].to_string(),
        _ => T_IP.to_string(),
    });
    let s = r.ipv6.replace_all(&s, |c: &Captures| {
        if c[0].chars().any(|ch| ch.is_ascii_digit()) {
            T_IP.to_string()
        } else {
            c[0].to_string()
        }
    });
    let s = r.host_suffix.replace_all(&s, T_HOST);
    let s = r.host_windows.replace_all(&s, T_HOST);
    let s = r.host_apple.replace_all(&s, T_HOST);
    s.into_owned()
}

/// Names only the running machine knows: its hostname and the account's
/// username. Scrubbed as whole words, case-insensitively, on top of [`scrub`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Names {
    pub hosts: Vec<String>,
    pub users: Vec<String>,
}

/// Names too short or too generic to scrub as words: the service runs as
/// `pasivd`, which would otherwise blank the daemon's name out of every line.
const NAME_STOPLIST: &[&str] = &[
    "pasiv",
    "pasivd",
    "root",
    "user",
    "admin",
    "runner",
    "localhost",
    "ubuntu",
    "debian",
    "fedora",
    "windows",
    "linux",
    "mac",
    "macos",
    "desktop",
    "laptop",
    "home",
    "pc",
];

fn name_regex(names: &[String]) -> Option<Regex> {
    let mut parts: Vec<String> = names
        .iter()
        .map(|n| n.trim())
        .filter(|n| n.len() >= 3 && !NAME_STOPLIST.contains(&n.to_ascii_lowercase().as_str()))
        .map(regex::escape)
        .collect();
    // Longest first: alternation is leftmost-first, so `tower.local` must
    // be tried before `tower`.
    parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
    parts.dedup();
    if parts.is_empty() {
        return None;
    }
    Regex::new(&format!(r"(?i)\b(?:{})\b", parts.join("|"))).ok()
}

pub fn scrub_names(text: &str, names: &Names) -> String {
    let mut s = Cow::Borrowed(text);
    if let Some(re) = name_regex(&names.hosts) {
        s = Cow::Owned(re.replace_all(&s, T_HOST).into_owned());
    }
    if let Some(re) = name_regex(&names.users) {
        s = Cow::Owned(re.replace_all(&s, T_USER).into_owned());
    }
    s.into_owned()
}

fn clean(text: &str, names: &Names) -> String {
    scrub_names(&scrub(text), names)
}

fn clean_opt(v: &mut Option<String>, names: &Names) {
    if let Some(s) = v.as_mut() {
        *s = clean(s, names);
    }
}

fn clean_value(v: &mut Value, names: &Names) {
    match v {
        Value::String(s) => *s = clean(s, names),
        Value::Array(a) => a.iter_mut().for_each(|x| clean_value(x, names)),
        Value::Object(o) => o.values_mut().for_each(|x| clean_value(x, names)),
        _ => {}
    }
}

fn clean_stacktrace(st: &mut Stacktrace, names: &Names) {
    for f in &mut st.frames {
        clean_opt(&mut f.filename, names);
        clean_opt(&mut f.abs_path, names);
        clean_opt(&mut f.package, names);
        clean_opt(&mut f.module, names);
        clean_opt(&mut f.context_line, names);
        f.pre_context.iter_mut().for_each(|l| *l = clean(l, names));
        f.post_context.iter_mut().for_each(|l| *l = clean(l, names));
        f.vars.values_mut().for_each(|v| clean_value(v, names));
    }
}

/// Every free-text field of an event through [`scrub`] + [`scrub_names`];
/// everything identity-shaped removed outright.
pub fn scrub_event(e: &mut Event<'static>, names: &Names) {
    e.server_name = None;
    e.user = None;
    e.request = None;
    e.breadcrumbs.values.clear();
    if let Some(m) = e.message.take() {
        e.message = Some(clean(&m, names));
    }
    e.culprit = e.culprit.take().map(|c| clean(&c, names));
    e.transaction = e.transaction.take().map(|t| clean(&t, names));
    if let Some(l) = e.logentry.as_mut() {
        l.message = clean(&l.message, names);
        l.params.iter_mut().for_each(|p| clean_value(p, names));
    }
    for ex in &mut e.exception.values {
        clean_opt(&mut ex.value, names);
        if let Some(st) = ex.stacktrace.as_mut() {
            clean_stacktrace(st, names);
        }
        if let Some(st) = ex.raw_stacktrace.as_mut() {
            clean_stacktrace(st, names);
        }
    }
    if let Some(st) = e.stacktrace.as_mut() {
        clean_stacktrace(st, names);
    }
    for t in &mut e.threads.values {
        clean_opt(&mut t.name, names);
        if let Some(st) = t.stacktrace.as_mut() {
            clean_stacktrace(st, names);
        }
        if let Some(st) = t.raw_stacktrace.as_mut() {
            clean_stacktrace(st, names);
        }
    }
    e.tags.values_mut().for_each(|v| *v = clean(v, names));
    e.extra.values_mut().for_each(|v| clean_value(v, names));
    for img in &mut e.debug_meta.to_mut().images {
        match img {
            DebugImage::Symbolic(i) => {
                i.name = clean(&i.name, names);
                clean_opt(&mut i.debug_file, names);
            }
            DebugImage::Apple(i) => i.name = clean(&i.name, names),
            _ => {}
        }
    }
    if let Some(Context::Device(d)) = e.contexts.get_mut("device") {
        d.name = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::{Exception, Frame, Values};

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        input: String,
        expected: String,
    }

    fn cases() -> Vec<Case> {
        serde_json::from_str(include_str!("../tests/contracts/scrub.json"))
            .expect("scrub.json parses")
    }

    /// ONE table, shared with the desktop app's scrubber and its webview twin.
    #[test]
    fn the_contract_table_holds() {
        let cases = cases();
        assert!(
            cases.len() >= 42,
            "the shared table lost rows: {}",
            cases.len()
        );
        for c in &cases {
            assert_eq!(scrub(&c.input), c.expected, "case {}", c.name);
        }
    }

    #[test]
    fn scrubbing_is_idempotent() {
        for c in cases() {
            let once = scrub(&c.input);
            assert_eq!(scrub(&once), once, "case {}", c.name);
        }
    }

    #[test]
    fn own_names_are_scrubbed_as_whole_words_only() {
        let names = Names {
            hosts: vec!["rack-7".into(), "pasivd".into(), "ab".into()],
            users: vec!["janedoe".into(), "root".into()],
        };
        let text = "rack-7 (Rack-7.lan) ran as JaneDoe; janedoeX stays; pasivd root ab";
        assert_eq!(
            scrub_names(&scrub(text), &names),
            "<host> (<host>) ran as <user>; janedoeX stays; pasivd root ab"
        );
        assert_eq!(scrub_names("x", &Names::default()), "x");
    }

    #[test]
    fn every_field_of_an_event_is_scrubbed_or_removed() {
        let mut e = Event {
            message: Some("could not write /home/jane/.config/pasivd/config.json".into()),
            server_name: Some("rack.lan".into()),
            culprit: Some("0x71C7656EC7ab88b098defB751B7401B5f6d8976F".into()),
            ..Default::default()
        };
        e.user = Some(sentry::User {
            username: Some("jane".into()),
            ..Default::default()
        });
        e.exception = Values::from(vec![Exception {
            ty: "panic".into(),
            value: Some("pool 192.168.1.9 refused jane@example.com".into()),
            stacktrace: Some(Stacktrace {
                frames: vec![Frame {
                    abs_path: Some("/home/bob/pasivd/src/main.rs".into()),
                    function: Some("pasivd::xmrig::spawn".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }]);
        e.tags.insert("rig".into(), "DESKTOP-4F7Q2RS".into());
        e.extra.insert(
            "cfg".into(),
            serde_json::json!({ "p": ["bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"], "n": 3 }),
        );
        scrub_event(&mut e, &Names::default());
        assert_eq!(e.server_name, None);
        assert!(e.user.is_none());
        assert_eq!(
            e.message.as_deref(),
            Some("could not write /home/<user>/.config/pasivd/config.json")
        );
        assert_eq!(e.culprit.as_deref(), Some("<address>"));
        let ex = &e.exception.values[0];
        assert_eq!(ex.value.as_deref(), Some("pool <ip> refused <email>"));
        let f = &ex.stacktrace.as_ref().unwrap().frames[0];
        assert_eq!(
            f.abs_path.as_deref(),
            Some("/home/<user>/pasivd/src/main.rs")
        );
        assert_eq!(f.function.as_deref(), Some("pasivd::xmrig::spawn"));
        assert_eq!(e.tags["rig"], "<host>");
        assert_eq!(
            e.extra["cfg"],
            serde_json::json!({ "p": ["<address>"], "n": 3 })
        );
        let json = serde_json::to_string(&e).unwrap();
        for leak in [
            "jane", "bob", "192.168", "0x71C7", "bc1q", "DESKTOP-", "rack.lan",
        ] {
            assert!(!json.contains(leak), "{leak} leaked: {json}");
        }
    }
}
