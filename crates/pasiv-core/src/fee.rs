// SPDX-License-Identifier: GPL-3.0-only
//! The hashrate-fee engine (docs/FEES.md §2). Time-sliced 4%, accrues ONLY in
//! `Mining` (never-list: a paused or idle user is charged exactly zero) — the
//! slice scheduler is driven by the same state machine the UI renders, making
//! overcharge structurally impossible rather than promised.
//!
//! The fee address ships below as a compile-time constant (never-list: changing
//! it requires a new signed release and a changelog entry). Every completed
//! slice is appended to a local, human-readable JSONL ledger.
//!
//! Fail-safe by design: a runner only diverts to the fee address during an
//! actual `Mining` slice, restarts always respawn on the user's address, and a
//! swap-back failure stops the miner rather than let it stick on the fee
//! address (the desktop supervisor stops after 3 consecutive failed
//! swap-backs; `pasivd` re-logs-in each slice edge).

use serde::Serialize;

use crate::types::Coin;

/// Compile-time fee address (never-list: changing it requires a signed
/// release and a changelog entry). Empty = fee engine off.
///
/// Mainnet Monero standard address, set 2026-07-24 — the fee engine is ON in
/// signed releases.
///
/// NOTE: `is_valid_xmr_address` checks prefix, length and alphabet — it does
/// NOT verify the Keccak checksum Monero addresses carry (that needs a base58
/// block decode plus keccak-256; see the module note in `address`). This
/// constant was checked against a real wallet by hand; the test below only
/// proves it is well-formed.
pub const FEE_ADDRESS_XMR: &str =
    "47jfXhesYLvN5M8Qzy5j7PCwd99kZKdwNKwxqonrdEDVVzopGUfNApn1NK98sPE7wgGzsEtvMYM1cZChVpDHasabFH1MZ1f";

/// 20 s per ~8.3 minute window ≈ 4% of Mining time.
pub const SLICE_WINDOW_SECS: u64 = 500;
pub const SLICE_SECS: u64 = 20;

/// The append-only ledger's filename, identical on every surface (desktop and
/// `pasivd`) so a rack of screenless machines audits the same way.
pub const LEDGER_FILE: &str = "fee-ledger.jsonl";

pub fn slices_enabled() -> bool {
    !FEE_ADDRESS_XMR.is_empty()
}

/// Whether a given count of elapsed **Mining** seconds falls inside a fee slice.
/// Over each `SLICE_WINDOW_SECS` window of Mining time, the first `SLICE_SECS`
/// are the fee slice (mine to the fee address); the remainder go to the user.
/// This is what makes the 4% *structural* — it's a pure function of time spent
/// in the `Mining` state, so a paused/idle user is charged exactly zero, and it
/// returns false entirely when no fee address ships. A runner consults this
/// each tick to decide which address the miner points at.
pub fn in_fee_slice(mining_secs: u64) -> bool {
    slices_enabled() && (mining_secs % SLICE_WINDOW_SECS) < SLICE_SECS
}

/// The fraction of gross revenue Pasiv's own fee takes for a given coin: the
/// time-sliced 4% on XMR, and exactly zero on every other coin (the fee is
/// XMR-only). Mirrors the runners' `fee_applies` gate
/// (`slices_enabled() && coin == Coin::Xmr`) so the number Auto ranks on can
/// never disagree with what's actually charged. Auto uses this to rank on the
/// user's *take-home* rather than gross — otherwise it would prefer XMR over a
/// fee-free coin that pays the user more once the fee is counted.
pub fn fee_fraction(coin: Coin) -> f64 {
    if slices_enabled() && coin == Coin::Xmr {
        SLICE_SECS as f64 / SLICE_WINDOW_SECS as f64
    } else {
        0.0
    }
}

/// One completed fee slice, appended to the local ledger.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct FeeEvent {
    pub started_at: u64,
    pub ended_at: u64,
    pub coin: Coin,
    pub address: String,
    pub est_hashes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeeSummary {
    /// False when no fee address ships.
    pub active: bool,
    pub fee_address_xmr: &'static str,
    pub total_slices: u64,
    pub total_fee_seconds: u64,
    pub recent: Vec<FeeEvent>,
}

/// Append-only, one JSON object per line — auditable with any text editor.
/// Callers hand in the ledger path (the desktop resolves its app-data dir;
/// `pasivd` uses its state dir or `PASIVD_FEE_LEDGER`).
pub fn append_event(path: &std::path::Path, ev: &FeeEvent) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", serde_json::to_string(ev).unwrap_or_default())
}

pub fn read_ledger(path: &std::path::Path) -> Vec<FeeEvent> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_ledger(&contents)
}

/// Summarise a ledger file for display.
pub fn summary_at(path: &std::path::Path) -> FeeSummary {
    summary_of(read_ledger(path))
}

/// Summarise already-parsed events (pure — the testable half of `summary_at`).
pub fn summary_of(events: Vec<FeeEvent>) -> FeeSummary {
    let total_fee_seconds = events
        .iter()
        .map(|e| e.ended_at.saturating_sub(e.started_at))
        .sum();
    let recent = events.iter().rev().take(20).cloned().collect();
    FeeSummary {
        active: slices_enabled(),
        fee_address_xmr: FEE_ADDRESS_XMR,
        total_slices: events.len() as u64,
        total_fee_seconds,
        recent,
    }
}

/// Tolerant line parser: a corrupt line never hides the rest of the ledger.
pub fn parse_ledger(contents: &str) -> Vec<FeeEvent> {
    contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_roundtrip_and_corruption_tolerance() {
        let ev = FeeEvent {
            started_at: 1000,
            ended_at: 1005,
            coin: Coin::Xmr,
            address: "4TEST".into(),
            est_hashes: 12_345,
        };
        let line = serde_json::to_string(&ev).unwrap();
        let contents = format!("{line}\nnot-json-garbage\n\n{line}\n");
        let parsed = parse_ledger(&contents);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].est_hashes, 12_345);
        assert_eq!(parsed[1].ended_at - parsed[1].started_at, 5);
    }

    #[test]
    fn fee_address_is_valid_and_ships_the_engine_on() {
        // A fee address ships (a signed-release change), so the engine is ON,
        // and it must be a valid Monero address (checksum-carrying).
        assert!(
            slices_enabled(),
            "a shipped fee address turns the engine on"
        );
        assert!(
            crate::address::is_valid_xmr_address(FEE_ADDRESS_XMR),
            "the fee address must be a well-formed Monero address (shape only — \
             the checksum is not verified here, see the constant's note)"
        );
    }

    #[test]
    fn slice_ratio_is_about_four_percent() {
        let ratio = SLICE_SECS as f64 / SLICE_WINDOW_SECS as f64;
        assert!(
            ratio > 0.038 && ratio < 0.042,
            "slice ratio {ratio} drifted from ~4%"
        );
    }

    #[test]
    fn fee_slice_follows_the_4pct_window_schedule() {
        // With an address shipped: the first SLICE_SECS of every SLICE_WINDOW_SECS
        // of Mining time are the fee slice — exactly 4% — and nothing else.
        assert!(in_fee_slice(0) && in_fee_slice(SLICE_SECS - 1));
        assert!(!in_fee_slice(SLICE_SECS) && !in_fee_slice(SLICE_WINDOW_SECS - 1));
        assert!(in_fee_slice(SLICE_WINDOW_SECS)); // wraps into the next window

        let count = (0..SLICE_WINDOW_SECS).filter(|s| in_fee_slice(*s)).count();
        assert_eq!(count as f64 / SLICE_WINDOW_SECS as f64, 0.04);
    }

    #[test]
    fn fee_fraction_is_xmr_only_and_matches_the_slice_ratio() {
        // The number Auto ranks on must equal what's actually charged: 4% on XMR,
        // zero on every fee-free coin. A drift here would let Auto rank on a fee
        // that isn't taken (or miss one that is).
        assert_eq!(
            fee_fraction(Coin::Xmr),
            SLICE_SECS as f64 / SLICE_WINDOW_SECS as f64
        );
        assert_eq!(fee_fraction(Coin::Zeph), 0.0);
        assert_eq!(fee_fraction(Coin::Sal), 0.0);
        assert_eq!(fee_fraction(Coin::Vrsc), 0.0);
    }

    #[test]
    fn summary_totals_and_recency() {
        let ev = |s: u64| FeeEvent {
            started_at: s,
            ended_at: s + 20,
            coin: Coin::Xmr,
            address: FEE_ADDRESS_XMR.into(),
            est_hashes: 1,
        };
        let s = summary_of((0..25).map(|i| ev(i * 500)).collect());
        assert!(s.active);
        assert_eq!(s.total_slices, 25);
        assert_eq!(s.total_fee_seconds, 25 * 20);
        assert_eq!(s.recent.len(), 20, "recent is capped at 20");
        assert_eq!(s.recent[0].started_at, 24 * 500, "newest first");
    }
}
