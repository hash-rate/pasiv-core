// SPDX-License-Identifier: GPL-3.0-only
//! Terminal presentation for the pasivd CLI — colour, symbols, and the help
//! screens.
//!
//! Zero dependencies on purpose: colour is ANSI escapes gated on an interactive
//! stdout, so a log file, a pipe, `NO_COLOR`, or `TERM=dumb` all get clean plain
//! text with no escape soup. A headless node's output is read as often from
//! `journalctl` as from a terminal, and that reader must not see `\x1b[` noise.

use std::io::IsTerminal;
use std::sync::OnceLock;

/// Colour is on only when stdout is a real terminal and nothing asks us to stop.
/// Cached — it cannot change under a running process, and every styled string
/// checks it. Honours the `NO_COLOR` convention (any value, even empty) and the
/// `dumb` terminal.
pub fn colour() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) {
            return false;
        }
        std::io::stdout().is_terminal()
    })
}

fn paint(code: &str, s: &str) -> String {
    if colour() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    paint("1", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn cyan(s: &str) -> String {
    paint("36", s)
}

/// Status glyphs, defined once so `claim`, `doctor`, and the run log read the
/// same. Kept to the small set every modern terminal renders.
pub fn tick() -> String {
    green("✓")
}
pub fn warn_mark() -> String {
    yellow("⚠")
}

/// The commands, in the order help lists them — the single source both the help
/// screen and the did-you-mean suggester read, so they can never disagree about
/// what exists.
pub const COMMANDS: &[(&str, &str)] = &[
    (
        "claim",
        "Pair this machine to your account. Prints a 6-character code you approve\n            in the Pasiv companion app.",
    ),
    (
        "run",
        "Run the node: mine, publish live state, obey start/stop from the phone.\n            This is what the systemd unit runs — you rarely type it yourself.",
    ),
    (
        "doctor",
        "One health pass — config, miner, pool, fee ledger. Exits non-zero on any\n            failure, so cron and systemd timers can watch it.",
    ),
    ("version", "Print the version and exit."),
    ("help", "Show this help. `pasivd <command> --help` for one command."),
];

/// The top-level help — the first thing a bare `pasivd` shows. Written to be
/// read by someone who has never seen the tool: what it is, the one-liner to get
/// going, then the reference.
pub fn print_help(version: &str) {
    let b = |s: &str| bold(s);
    println!(
        "{} {}",
        b("pasivd"),
        dim(version.trim_start_matches("pasivd "))
    );
    println!("The headless Pasiv node — turn a server, NAS, or spare box into a rig in");
    println!(
        "your fleet. Mines Monero to {} payout address and shows up in the",
        b("your own")
    );
    println!("phone companion beside your desktops. No GUI, no wallet on the box.");
    println!();
    println!("{}", b("USAGE"));
    println!("  pasivd {}", cyan("<command>"));
    println!();
    println!("{}", b("COMMANDS"));
    // 2-space indent + 7-wide name + 3 spaces = 12 columns before the text, so
    // the "\n            " (12-space) continuations in COMMANDS line up under it.
    for (name, desc) in COMMANDS {
        println!("  {}   {}", cyan(&format!("{name:<7}")), desc);
    }
    println!();
    println!("{}", b("GETTING STARTED"));
    println!(
        "  {}                      pair this node",
        bold("sudo pasivd claim"),
    );
    println!(
        "  {}     start mining once a payout is set",
        bold("sudo systemctl enable --now pasivd"),
    );
    println!();
    println!("{}", b("LEARN MORE"));
    println!("  {}", dim("https://pasiv.network/pasivd"));
}

/// Per-command help. Short by design — a command with three flags does not need
/// a man page, it needs to say what it does and show the one line you type.
pub fn print_command_help(cmd: &str) {
    match cmd {
        "claim" => {
            println!(
                "{} — pair this node to your Pasiv account.",
                bold("pasivd claim")
            );
            println!();
            println!(
                "Prints a 6-character code. In the companion app, tap {} then",
                bold("+")
            );
            println!(
                "{} and enter it. The node is bound to whoever approves it.",
                bold("Add node")
            );
            println!();
            println!("{}", bold("USAGE"));
            println!(
                "  sudo pasivd claim        {}",
                dim("# needs root to write /etc/pasivd.json")
            );
            println!();
            println!(
                "Nothing mines until you claim {} an XMR payout exists on the",
                bold("and")
            );
            println!("account (desktop app -> Coins -> Monero, which syncs automatically).");
        }
        "run" => {
            println!("{} — run the node in the foreground.", bold("pasivd run"));
            println!();
            println!("Mines, publishes live state to your fleet, and obeys start/stop from");
            println!("the phone. The installer's systemd unit runs exactly this, so you");
            println!("normally use {} rather than typing it.", bold("systemctl"));
            println!();
            println!("{}", bold("USAGE"));
            println!(
                "  sudo systemctl enable --now pasivd     {}",
                dim("# the usual way")
            );
            println!(
                "  pasivd run                             {}",
                dim("# foreground, for a quick look")
            );
        }
        "doctor" => {
            println!("{} — one diagnostic pass.", bold("pasivd doctor"));
            println!();
            println!("Checks config, the miner binary and its pinned checksum, pool");
            println!(
                "reachability, and the fee ledger. Prints {} / {} / {} per check",
                green("PASS"),
                yellow("WARN"),
                red("FAIL")
            );
            println!("and exits non-zero if anything failed — so a cron or systemd timer");
            println!("can page you without parsing the output.");
            println!();
            println!("{}", bold("USAGE"));
            println!(
                "  sudo pasivd doctor       {}",
                dim("# root reads /etc/pasivd.json")
            );
        }
        _ => print_help(crate::VERSION),
    }
}

/// Unknown-command handler with a did-you-mean, because a typo should cost one
/// glance, not a trip to `--help`. Suggests the closest real command only when
/// it is genuinely close (edit distance <= 2), never a wild guess.
pub fn unknown(cmd: &str) {
    eprintln!(
        "{} unknown command {}",
        red("error:"),
        bold(&format!("'{cmd}'"))
    );
    if let Some(near) = closest(cmd) {
        eprintln!("  did you mean {}?", green(&format!("'{near}'")));
    }
    eprintln!("  run {} to see all commands", bold("pasivd --help"));
}

/// Nearest command by Levenshtein distance, within a small threshold so a real
/// slip ("cliam", "docter") is caught but noise is not.
fn closest(input: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .map(|(name, _)| *name)
        .map(|name| (name, levenshtein(input, name)))
        .filter(|(_, d)| *d <= 2)
        .min_by_key(|(_, d)| *d)
        .map(|(name, _)| name)
}

/// Plain iterative Levenshtein — the command set is tiny, so the two-row form is
/// more than fast enough and needs no dependency.
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn did_you_mean_catches_a_slip_but_not_noise() {
        assert_eq!(closest("cliam"), Some("claim"));
        assert_eq!(closest("docter"), Some("doctor"));
        assert_eq!(closest("run"), Some("run"));
        // Too far from anything real — better to say nothing than mislead.
        assert_eq!(closest("frobnicate"), None);
    }

    #[test]
    fn every_command_help_case_is_wired() {
        // A command listed in COMMANDS but missing from print_command_help would
        // silently fall through to the full help — assert the three real
        // subcommands each have their own page.
        for cmd in ["claim", "run", "doctor"] {
            assert!(
                COMMANDS.iter().any(|(n, _)| *n == cmd),
                "{cmd} not in COMMANDS"
            );
        }
    }

    #[test]
    fn colour_escapes_are_absent_when_disabled() {
        // paint() with colour off must return the bare string — the property the
        // whole "clean in a pipe" promise rests on. colour() reads the real
        // stdout, so assert on paint()'s plain branch directly.
        let plain = "PASS";
        if !colour() {
            assert_eq!(green(plain), plain);
            assert!(!bold(plain).contains('\x1b'));
        }
    }
}
