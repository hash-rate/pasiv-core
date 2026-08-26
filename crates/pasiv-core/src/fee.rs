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

// ── The slice scheduler: the enforcement state machine ─────────────────────
//
// Phase B of the open-core split: this is the SAME state machine the
// proprietary desktop supervisor and the open `pasivd` daemon both drive, so
// "the fee is enforced by open code" holds everywhere the fee is charged.
// It is deliberately pure — the consumers do the I/O (hot-swapping the pool
// login, reading it back, killing the miner) and report outcomes here; this
// type owns WHAT must happen: which side the miner belongs on, when a ledger
// line is due, and when repeated failure to return to the user's address
// must stop mining entirely.

/// Which address the miner's pool login points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayoutSide {
    User,
    Fee,
}

/// Consecutive failures to swap the payout BACK to the user tolerated before
/// mining must stop outright. Mining to the fee address is capped at 4% by
/// the slice schedule only if the swap-back works; when it can't, failing
/// toward "not mining" is the only direction that never costs the user.
pub const FEE_RETURN_MAX_RETRIES: u32 = 3;

/// The verdict after a failed swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapFailure {
    /// Transient — leave state unchanged and retry next tick.
    Retry,
    /// The bounded retries to RETURN to the user are exhausted: stop the
    /// miner. A respawn always comes up on the user's address.
    StopMining { attempts: u32 },
}

/// Pure fee-slice enforcement state. One instance per miner run; create a
/// fresh one on every (re)spawn — a respawned miner is always on the user's
/// address.
#[derive(Debug, Default)]
pub struct SliceScheduler {
    on_fee: bool,
    slice_started_at: Option<u64>,
    failed_returns: u32,
}

impl SliceScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The side the miner must be pointed at right now: the fee address only
    /// during a slice while ACTIVELY mining, the user's address every other
    /// moment. `actively_mining` is the consumer's notion of "hashes are being
    /// produced" (the desktop's `Mining` state; pasivd's nonzero hashrate) —
    /// which is what makes a paused, warming, or errored miner structurally
    /// unchargeable.
    pub fn desired(&self, actively_mining: bool, mining_secs: u64) -> PayoutSide {
        if actively_mining && in_fee_slice(mining_secs) {
            PayoutSide::Fee
        } else {
            PayoutSide::User
        }
    }

    /// The side this scheduler currently believes the miner is on.
    pub fn current(&self) -> PayoutSide {
        if self.on_fee {
            PayoutSide::Fee
        } else {
            PayoutSide::User
        }
    }

    /// The miner is CONFIRMED on `side` — a successful hot-swap, or (better,
    /// pasivd's level-triggered model) the login actually read back from the
    /// miner. Resets the failure counter, tracks slice edges, and returns the
    /// completed `FeeEvent` exactly once per slice, on the falling edge — so
    /// the ledger records where hashes really went, with the same est_hashes
    /// convention both surfaces have always used (closing hashrate × slice
    /// seconds).
    pub fn confirmed(
        &mut self,
        side: PayoutSide,
        now_unix: u64,
        last_hashrate: f64,
    ) -> Option<FeeEvent> {
        self.failed_returns = 0;
        let was_fee = self.on_fee;
        self.on_fee = side == PayoutSide::Fee;
        match (was_fee, self.on_fee) {
            (false, true) => {
                self.slice_started_at = Some(now_unix);
                None
            }
            (true, false) => {
                let start = self.slice_started_at.take()?;
                let secs = now_unix.saturating_sub(start);
                Some(FeeEvent {
                    started_at: start,
                    ended_at: now_unix,
                    coin: Coin::Xmr,
                    address: FEE_ADDRESS_XMR.to_string(),
                    est_hashes: (last_hashrate * secs as f64) as u64,
                })
            }
            _ => None,
        }
    }

    /// A swap toward `wanted` failed (or the miner's actual login could not be
    /// corrected). Failing toward the FEE address is self-healing — the next
    /// tick simply retries, and the user loses nothing. Failing to RETURN to
    /// the user is the only direction that costs money, so it is bounded:
    /// after `FEE_RETURN_MAX_RETRIES` consecutive failures the verdict is
    /// StopMining, and the consumer must stop the miner (respawns always come
    /// up on the user's address).
    pub fn swap_failed(&mut self, wanted: PayoutSide) -> SwapFailure {
        if wanted == PayoutSide::User {
            self.failed_returns += 1;
            if self.failed_returns >= FEE_RETURN_MAX_RETRIES {
                let attempts = self.failed_returns;
                self.failed_returns = 0;
                self.on_fee = false; // the consumer stops the miner now
                self.slice_started_at = None;
                return SwapFailure::StopMining { attempts };
            }
        }
        SwapFailure::Retry
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
    fn scheduler_full_slice_lifecycle_writes_one_event() {
        let mut s = SliceScheduler::new();
        // Outside a slice, mining: stay on the user.
        assert_eq!(s.desired(true, 250), PayoutSide::User);
        // Slice opens (mining_secs wraps into the window): fee side desired.
        assert_eq!(s.desired(true, 500), PayoutSide::Fee);
        // Rising edge: no event, slice opens.
        assert!(s.confirmed(PayoutSide::Fee, 1_000, 5_000.0).is_none());
        assert_eq!(s.current(), PayoutSide::Fee);
        // Holding on fee across ticks must not re-open or emit.
        assert!(s.confirmed(PayoutSide::Fee, 1_010, 5_000.0).is_none());
        // Falling edge: exactly one event, correct span + est_hashes.
        let ev = s
            .confirmed(PayoutSide::User, 1_020, 5_000.0)
            .expect("event");
        assert_eq!((ev.started_at, ev.ended_at), (1_000, 1_020));
        assert_eq!(ev.est_hashes, (5_000.0 * 20.0) as u64);
        assert_eq!(ev.coin, Coin::Xmr);
        assert_eq!(ev.address, FEE_ADDRESS_XMR);
        // And staying on the user emits nothing.
        assert!(s.confirmed(PayoutSide::User, 1_030, 5_000.0).is_none());
    }

    #[test]
    fn scheduler_never_charges_when_not_actively_mining() {
        let s = SliceScheduler::new();
        // Even inside the slice window, a paused/warming/idle miner belongs
        // to the user — this is never-list "fee time only in Mining".
        assert_eq!(s.desired(false, 0), PayoutSide::User);
        assert_eq!(s.desired(false, 505), PayoutSide::User);
    }

    #[test]
    fn scheduler_failing_toward_fee_is_unbounded_retry() {
        let mut s = SliceScheduler::new();
        // Failing to GET ONTO the fee address costs the user nothing; retry
        // forever, never stop the miner, never count it against the return
        // budget.
        for _ in 0..100 {
            assert_eq!(s.swap_failed(PayoutSide::Fee), SwapFailure::Retry);
        }
        assert_eq!(
            s.swap_failed(PayoutSide::User),
            SwapFailure::Retry,
            "budget untouched"
        );
    }

    #[test]
    fn scheduler_bounded_return_failures_stop_the_miner() {
        let mut s = SliceScheduler::new();
        assert!(s.confirmed(PayoutSide::Fee, 1_000, 100.0).is_none());
        // Two failures: still retrying.
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
        // Third consecutive failure: stop — never keep mining to the fee.
        assert_eq!(
            s.swap_failed(PayoutSide::User),
            SwapFailure::StopMining { attempts: 3 }
        );
        // After the stop verdict the scheduler is reset for the respawn.
        assert_eq!(s.current(), PayoutSide::User);
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
    }

    #[test]
    fn scheduler_success_resets_the_failure_budget() {
        let mut s = SliceScheduler::new();
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
        // One confirmed swap clears the count — only CONSECUTIVE failures stop.
        let _ = s.confirmed(PayoutSide::User, 2_000, 0.0);
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
        assert_eq!(s.swap_failed(PayoutSide::User), SwapFailure::Retry);
        assert_eq!(
            s.swap_failed(PayoutSide::User),
            SwapFailure::StopMining { attempts: 3 }
        );
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

    /// THE RECONCILE DISCIPLINE, as a conformance table. Both consumers must
    /// drive the scheduler this way (desktop: the app supervisor's stats
    /// loop; daemon: pasivd run loop):
    ///
    ///   1. read back the login the miner is ACTUALLY using
    ///   2. matches the desired side: confirmed()
    ///   3. differs (or unreadable): set — Ok means confirmed(), Err means
    ///      swap_failed()
    ///
    /// This test drives that exact shape against a scripted miner and pins the
    /// outcomes the money depends on. If the discipline itself ever changes,
    /// change it HERE first — then in both consumers.
    #[test]
    fn reconcile_discipline_conformance() {
        const USER: &str = "4user";
        struct ScriptedMiner {
            login: String,
            set_ok: bool,
            applies: bool, // false = the lying PUT (Ok without effect)
            sets: u32,
        }
        impl ScriptedMiner {
            fn read(&self) -> Option<&str> {
                Some(self.login.as_str())
            }
            fn set(&mut self, target: &str) -> bool {
                self.sets += 1;
                if self.set_ok && self.applies {
                    self.login = target.to_string();
                }
                self.set_ok
            }
        }
        // One tick of the canonical discipline. Returns the ledger event, if
        // the tick closed a slice, and whether the failsafe fired.
        fn tick(
            sched: &mut SliceScheduler,
            miner: &mut ScriptedMiner,
            actively_mining: bool,
            mining_secs: u64,
            now: u64,
            hashrate: f64,
        ) -> (Option<FeeEvent>, bool) {
            let want = sched.desired(actively_mining, mining_secs);
            let target = match want {
                PayoutSide::Fee => FEE_ADDRESS_XMR,
                PayoutSide::User => USER,
            };
            match miner.read() {
                Some(actual) if actual == target => (sched.confirmed(want, now, hashrate), false),
                _ => {
                    if miner.set(target) {
                        (sched.confirmed(want, now, hashrate), false)
                    } else {
                        (
                            None,
                            matches!(sched.swap_failed(want), SwapFailure::StopMining { .. }),
                        )
                    }
                }
            }
        }

        // A full honest slice: swap in, mine the slice, swap out, ONE event.
        let mut sched = SliceScheduler::new();
        let mut m = ScriptedMiner {
            login: USER.into(),
            set_ok: true,
            applies: true,
            sets: 0,
        };
        let (ev, stopped) = tick(&mut sched, &mut m, true, 0, 1_000, 500.0);
        assert!(ev.is_none() && !stopped);
        assert_eq!(m.login, FEE_ADDRESS_XMR, "slice opens on the fee address");
        let (ev, stopped) = tick(&mut sched, &mut m, true, 10, 1_010, 500.0);
        assert!(ev.is_none() && !stopped);
        let (ev, stopped) = tick(&mut sched, &mut m, true, 25, 1_020, 500.0);
        assert!(!stopped);
        let ev = ev.expect("leaving the slice closes exactly one event");
        assert_eq!((ev.started_at, ev.ended_at), (1_000, 1_020));
        assert_eq!(ev.est_hashes, 10_000);
        assert_eq!(m.login, USER, "and the login is the user's again");
        assert_eq!(m.sets, 2, "on-target ticks issue no swaps");

        // The lying PUT: Ok without effect. The discipline keeps retrying —
        // the read-back never matches, so the set is re-issued every tick.
        let mut sched = SliceScheduler::new();
        let mut m = ScriptedMiner {
            login: USER.into(),
            set_ok: true,
            applies: false,
            sets: 0,
        };
        for i in 0..3 {
            let _ = tick(&mut sched, &mut m, true, 0, 2_000 + i, 500.0);
        }
        assert_eq!(m.sets, 3, "a lying PUT is retried, never believed");
        assert_eq!(m.login, USER);

        // Failing TOWARD the fee is unbounded retry — the user loses nothing.
        let mut sched = SliceScheduler::new();
        let mut m = ScriptedMiner {
            login: USER.into(),
            set_ok: false,
            applies: false,
            sets: 0,
        };
        for i in 0..10 {
            let (_, stopped) = tick(&mut sched, &mut m, true, 0, 3_000 + i, 500.0);
            assert!(!stopped, "failing toward the FEE side must never stop");
        }

        // Failing to RETURN is bounded: the failsafe fires, mining stops.
        let mut sched = SliceScheduler::new();
        let mut m = ScriptedMiner {
            login: FEE_ADDRESS_XMR.into(),
            set_ok: false,
            applies: false,
            sets: 0,
        };
        let mut fired = 0;
        for i in 0..FEE_RETURN_MAX_RETRIES {
            let (_, stopped) = tick(&mut sched, &mut m, true, 100, 4_000 + u64::from(i), 500.0);
            if stopped {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "the failsafe fires exactly once at the bound");

        // Not actively mining NEVER desires the fee side, whatever the clock.
        let sched = SliceScheduler::new();
        for secs in [0, 5, 19, 250, 500, 505] {
            assert_eq!(sched.desired(false, secs), PayoutSide::User);
        }
    }
}
