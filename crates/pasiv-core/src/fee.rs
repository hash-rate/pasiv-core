// SPDX-License-Identifier: GPL-3.0-only
//! The hashrate-fee engine (docs/FEES.md §2). Time-sliced 4%, accrues ONLY in
//! `Mining` (never-list: a paused or idle user is charged exactly zero) — the
//! slice scheduler is driven by the same state machine the UI renders, making
//! overcharge structurally impossible rather than promised.
//!
//! The fee address ships below as a compile-time constant — the BTC treasury
//! on unMineable, Pasiv's one payout route (never-list: changing it requires a
//! new signed release and a changelog entry). The 4% applies to every coin,
//! whatever the user is paid in (USDT, BTC, or the coin itself). The slice is
//! 20 s in every 500 s for a miner that switches live (XMRig) and 10 min in
//! every 4 h 10 min for one that must restart (the Pearl engine; SRBMiner
//! before it) — 4% either way. Every completed slice is appended to a local,
//! human-readable JSONL ledger.
//!
//! History: until 0.7.0 (2026-10-24) a second, legacy "direct" route mined
//! straight to coin pools with a Monero fee address and an XMR-only fee rule.
//! That route, its address and its rule are retired; the ledger reader below
//! still parses the lines it wrote.
//!
//! Fail-safe by design: a runner only diverts to the fee address during an
//! actual `Mining` slice, restarts always respawn on the user's address, and a
//! swap-back failure stops the miner rather than let it stick on the fee
//! address (the desktop supervisor stops after 3 consecutive failed
//! swap-backs; `pasivd` re-logs-in each slice edge).

use serde::{Deserialize, Serialize};

use crate::types::Coin;

/// 20 s per ~8.3 minute window = 4% of Mining time — the live-switch (XMRig)
/// slice; see `slice_shape` for the restart-based (GPU engine) one.
pub const SLICE_WINDOW_SECS: u64 = 500;
pub const SLICE_SECS: u64 = 20;

/// The append-only ledger's filename, identical on every surface (desktop and
/// `pasivd`) so a rack of screenless machines audits the same way.
pub const LEDGER_FILE: &str = "fee-ledger.jsonl";

/// Whether the fee engine is on: a treasury address ships. (A build with an
/// empty treasury would charge nothing — that is the fail-safe direction.)
pub fn slices_enabled() -> bool {
    !FEE_ADDRESS_TREASURY.is_empty()
}

/// The slice sits at the END of each window, never the start. Mining time
/// starts from zero on every launch, so a slice at the start was charged in
/// full by every short session — a GPU rig restarted by an update paid its
/// whole 10-minute slice up front (≈17% of a 1 h session). At the end, any
/// session pays at most 4%, and over a long run exactly 4% (2026-09-27).
fn slice_at_window_end(mining_secs: u64, window: u64, slice: u64) -> bool {
    mining_secs % window >= window - slice
}

/// The fraction of gross revenue Pasiv's own fee takes for a coin: the
/// time-sliced 4%, the same for EVERY coin on the one route (decided
/// 2026-09-25 for unMineable, the only route since 0.7.0; until then the
/// legacy direct route charged Monero only, which is why this takes a coin).
/// It mirrors the runners' `fee_applies` gate so the number an earnings
/// estimate nets out can never disagree with what is actually charged.
pub fn fee_fraction(coin: Coin) -> f64 {
    let _ = coin; // every coin, by design — the parameter is the audit trail
    fee_fraction_unmineable()
}

// ── unMineable — the one route (default from 0.5.0, only route from 0.7.0) ─
//
// The 4% applies to EVERY coin (decided 2026-09-25, disclosed in
// docs/FEES.md): a fee slice mines to Pasiv's treasury instead of the user's
// address, on the same pool and algorithm, whether the user is paid in USDT,
// BTC or the coin itself.

/// Pasiv's treasury — a Bitcoin SegWit address (bech32 checksum verified by
/// the test below). unMineable converts the fee slices' hashrate and pays it
/// in BTC. A compile-time constant: changing it takes a signed release and a
/// changelog entry (never-list #3).
pub const FEE_ADDRESS_TREASURY: &str = "bc1qv8nlkvhjelgp5d5lk79qjs6sdeq08w8vjxgz90";

/// The asset the treasury is paid in on unMineable.
pub const FEE_TREASURY_ASSET: &str = "BTC";

/// Pasiv's unMineable referral code, appended to every USER login
/// (`#drp0-8auk`). It lowers the user's unMineable fee from 1% to 0.75% and
/// pays Pasiv 0.25% of the user's rewards out of unMineable's own cut —
/// disclosed alongside the 4% (docs/FEES.md). Issued for the BTC treasury.
pub const UNMINEABLE_REFERRAL: &str = "drp0-8auk";

/// How a miner is switched to the fee address, which decides the slice shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchKind {
    /// XMRig re-logs-in live over its HTTP API — no downtime, so short slices.
    HotSwap,
    /// The GPU lane has no live switch (the Pearl engine today, SRBMiner
    /// before it): a slice edge is a restart, measured at ~19 s from kill to
    /// hashing on RTX 4060s (2026-09-25, SRBMiner). Two
    /// edges per window, so the window is long enough that warm-up costs the
    /// user ~0.25% instead of the ~7% a 20 s slice would.
    Restart,
}

/// (window, slice) seconds — both exactly 4% of Mining time.
pub fn slice_shape(kind: SwitchKind) -> (u64, u64) {
    match kind {
        SwitchKind::HotSwap => (SLICE_WINDOW_SECS, SLICE_SECS),
        SwitchKind::Restart => (15_000, 600), // 10 min every 4 h 10 min
    }
}

/// Whether a given count of elapsed **Mining** seconds falls inside a fee
/// slice, for a given switch kind (all coins). Over each window of Mining
/// time the LAST `slice` seconds are the fee slice (mine to the treasury);
/// the rest go to the user. This is what makes the 4% *structural* — a pure
/// function of time spent in the `Mining` state, so a paused/idle user is
/// charged exactly zero — and it returns false entirely when no treasury
/// ships. A runner consults it each tick to decide which address the miner
/// points at.
pub fn in_fee_slice_for(kind: SwitchKind, mining_secs: u64) -> bool {
    let (window, slice) = slice_shape(kind);
    slices_enabled() && slice_at_window_end(mining_secs, window, slice)
}

/// The fee login on unMineable: the treasury, paid in BTC, with the worker
/// kept so slices are visible per rig on the pool's public stats.
pub fn unmineable_fee_login(worker: &str) -> String {
    format!(
        "{}:{}.{}",
        FEE_TREASURY_ASSET,
        FEE_ADDRESS_TREASURY,
        crate::unmineable::worker_name(worker)
    )
}

// ── Share & earn: affiliates (decided 2026-09-26, disclosed in docs/FEES.md) ─
//
// The fee stays exactly 4% of mining time and the miner pays nothing extra.
// When the miner entered a friend's code, a SHARE of Pasiv's own fee slices
// mines to the friend's address instead of the treasury: since 2026-10-27
// every code gets 2 slices in every 4 (2 of the 4 points, 2% of the rig's
// gross; Pasiv keeps 2%), for 12 months from the day the code was confirmed.
// Until then a regular code got 1 in 4 and a creator code 2 in 4; the server
// decides the share and now reports 2 for every code. The pool pays the affiliate
// directly; Pasiv never holds anything. The affiliate address is chosen by
// the MINER (by entering a code and confirming what it pays), is pinned on
// their machine, and every slice lands in their fee ledger with the address
// it paid.

/// What an affiliate is paid in on unMineable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AffiliateAsset {
    Btc,
    Usdt,
}

/// An affiliate a miner has confirmed. Lives in the miner's own config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Affiliate {
    pub code: String,
    pub asset: AffiliateAsset,
    pub address: String,
    /// Points of the 4% that go to the affiliate. Every code is 2 since
    /// 2026-10-27; 1 (the old regular-code share) is still accepted, so a pin
    /// made before then stays valid until the daily re-check moves it to 2.
    /// Anything else is treated as invalid.
    pub share_of_4: u8,
    /// Unix seconds; from then on every slice goes back to the treasury.
    pub until_unix: u64,
}

/// How long a confirmed code pays its affiliate: 12 months.
pub const AFFILIATE_TERM_SECS: u64 = 365 * 24 * 3600;

impl Affiliate {
    /// Address shape matches the asset and the share is 1 or 2.
    pub fn is_valid(&self) -> bool {
        let addr_ok = match self.asset {
            AffiliateAsset::Btc => crate::address::is_valid_btc_segwit_address(&self.address),
            AffiliateAsset::Usdt => {
                crate::address::is_valid_bsc_address(&self.address)
                    || crate::address::is_valid_tron_address(&self.address)
            }
        };
        addr_ok && matches!(self.share_of_4, 1 | 2)
    }

    fn symbol(&self) -> &'static str {
        match self.asset {
            AffiliateAsset::Btc => "BTC",
            AffiliateAsset::Usdt => "USDT",
        }
    }
}

/// Where one fee slice goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeDestination {
    Treasury,
    Affiliate,
}

/// The destination of the slice that `mining_secs` falls in. Slices are
/// numbered by mining time (window index), so over any 4 consecutive windows
/// exactly `share_of_4` go to the affiliate — whatever the slice shape.
/// Expired, invalid or absent affiliate → the treasury, as before.
pub fn fee_destination(
    kind: SwitchKind,
    mining_secs: u64,
    affiliate: Option<&Affiliate>,
    now_unix: u64,
) -> FeeDestination {
    let Some(a) = affiliate else {
        return FeeDestination::Treasury;
    };
    if !a.is_valid() || now_unix >= a.until_unix {
        return FeeDestination::Treasury;
    }
    // The affiliate's windows are the LAST of each 4, so a short session
    // (mining time restarts at every launch) favours the treasury rather than
    // handing the affiliate the only slice it pays — exact over any 4 windows.
    let (window, _) = slice_shape(kind);
    if (mining_secs / window) % 4 >= 4 - u64::from(a.share_of_4) {
        FeeDestination::Affiliate
    } else {
        FeeDestination::Treasury
    }
}

/// The pool login for a fee slice going to `dest`, and the address the ledger
/// must record for it.
pub fn fee_login_for(
    dest: FeeDestination,
    affiliate: Option<&Affiliate>,
    worker: &str,
) -> (String, String) {
    match (dest, affiliate) {
        (FeeDestination::Affiliate, Some(a)) if a.is_valid() => (
            format!(
                "{}:{}.{}",
                a.symbol(),
                a.address.trim(),
                crate::unmineable::worker_name(worker)
            ),
            a.address.trim().to_string(),
        ),
        _ => (
            unmineable_fee_login(worker),
            FEE_ADDRESS_TREASURY.to_string(),
        ),
    }
}

/// Fee fraction on unMineable: 4% of every coin (zero only if no treasury
/// shipped — then nothing is charged at all).
pub fn fee_fraction_unmineable() -> f64 {
    if slices_enabled() {
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
    /// The treasury every slice pays today. Lines written by builds before
    /// 0.7.0 may name the retired direct-route Monero fee address instead;
    /// they still parse and count (`recent` shows the address each paid).
    pub fee_address: &'static str,
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
        fee_address: FEE_ADDRESS_TREASURY,
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
#[derive(Debug)]
pub struct SliceScheduler {
    on_fee: bool,
    slice_started_at: Option<u64>,
    failed_returns: u32,
    /// Where this scheduler's fee slices actually mine — what the ledger
    /// records: the coin being mined and the treasury (or, per slice, the
    /// affiliate — `set_fee_address`).
    fee_coin: Coin,
    fee_address: String,
}

impl SliceScheduler {
    /// A scheduler for a lane mining `coin` on unMineable: the ledger records
    /// that coin and the treasury the slice really paid (the ledger must say
    /// where hashes went).
    pub fn for_unmineable(coin: Coin) -> Self {
        Self {
            on_fee: false,
            slice_started_at: None,
            failed_returns: 0,
            fee_coin: coin,
            fee_address: FEE_ADDRESS_TREASURY.to_string(),
        }
    }

    /// Point the ledger at the address the CURRENT slice mines to — the
    /// treasury or, for a share-and-earn slice, the affiliate. Set it before
    /// the slice's falling edge; `confirmed` records it then.
    pub fn set_fee_address(&mut self, address: &str) {
        self.fee_address = address.to_string();
    }

    /// The side the miner must be pointed at right now: the fee address only
    /// during a slice while ACTIVELY mining, the user's address every other
    /// moment — with the slice shape for how this miner switches (live
    /// re-login vs restart). `actively_mining` is the consumer's notion of
    /// "hashes are being produced" (the desktop's `Mining` state; pasivd's
    /// nonzero hashrate) — which is what makes a paused, warming, or errored
    /// miner structurally unchargeable.
    pub fn desired_for(
        &self,
        kind: SwitchKind,
        actively_mining: bool,
        mining_secs: u64,
    ) -> PayoutSide {
        if actively_mining && in_fee_slice_for(kind, mining_secs) {
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
                    coin: self.fee_coin,
                    address: self.fee_address.clone(),
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

    fn aff(share: u8, asset: AffiliateAsset, address: &str) -> Affiliate {
        Affiliate {
            code: "friend".into(),
            asset,
            address: address.into(),
            share_of_4: share,
            until_unix: 2_000_000_000,
        }
    }
    const BSC: &str = "0x8a3f1c2b9e4d7a6051fe2c9b3d4a5e6f7b8c9d0e";

    /// Over a long simulated run the fee stays exactly 4% of mining time and
    /// the affiliate gets exactly its points of it — for both slice shapes.
    #[test]
    fn affiliate_share_is_exact_and_the_total_never_moves() {
        for kind in [SwitchKind::HotSwap, SwitchKind::Restart] {
            let (window, _) = slice_shape(kind);
            let horizon = window * 400; // many windows, a multiple of 4
            for (share, want_aff) in [(0u8, 0.0), (1, 0.01), (2, 0.02)] {
                let a = aff(share.max(1), AffiliateAsset::Usdt, BSC);
                let a = (share > 0).then_some(&a);
                let (mut fee, mut to_aff) = (0u64, 0u64);
                for t in 0..horizon {
                    if in_fee_slice_for(kind, t) {
                        fee += 1;
                        if fee_destination(kind, t, a, 0) == FeeDestination::Affiliate {
                            to_aff += 1;
                        }
                    }
                }
                let total = fee as f64 / horizon as f64;
                let aff_share = to_aff as f64 / horizon as f64;
                assert!((total - 0.04).abs() < 1e-9, "{kind:?} total {total}");
                assert!(
                    (aff_share - want_aff).abs() < 1e-9,
                    "{kind:?} share {share}: {aff_share}"
                );
            }
        }
    }

    /// Mining time restarts at 0 on every launch. No session, however short,
    /// may pay more than 4% (the slice closes each window), and a short
    /// session can never hand an affiliate more than its points of the fee.
    #[test]
    fn short_sessions_never_pay_more_than_4pct_or_overpay_the_affiliate() {
        let a = aff(2, AffiliateAsset::Usdt, BSC);
        for kind in [SwitchKind::HotSwap, SwitchKind::Restart] {
            let (window, _) = slice_shape(kind);
            for len in [60, window / 2, window, window + 1, 3 * window + 7] {
                let fee = (0..len).filter(|t| in_fee_slice_for(kind, *t)).count() as f64;
                assert!(
                    fee / len as f64 <= 0.04 + 1e-9,
                    "{kind:?} {len}s paid {}",
                    fee / len as f64
                );
                let to_aff = (0..len)
                    .filter(|t| in_fee_slice_for(kind, *t))
                    .filter(|t| fee_destination(kind, *t, Some(&a), 0) == FeeDestination::Affiliate)
                    .count() as f64;
                assert!(
                    to_aff <= fee * 0.5 + 1e-9 || fee == 0.0,
                    "{kind:?} {len}s: affiliate {to_aff} of {fee}"
                );
            }
        }
    }

    #[test]
    fn no_affiliate_expired_or_invalid_means_the_treasury_as_before() {
        for t in 0..20_000 {
            assert_eq!(
                fee_destination(SwitchKind::HotSwap, t, None, 0),
                FeeDestination::Treasury
            );
        }
        let mut a = aff(2, AffiliateAsset::Usdt, BSC);
        a.until_unix = 1_000;
        assert_eq!(
            fee_destination(SwitchKind::HotSwap, 1_000, Some(&a), 1_000),
            FeeDestination::Treasury
        );
        assert_eq!(
            fee_destination(SwitchKind::HotSwap, 1_000, Some(&a), 999),
            FeeDestination::Affiliate
        );
        let bad = aff(3, AffiliateAsset::Usdt, BSC); // 3 of 4 is not a tier
        assert_eq!(
            fee_destination(SwitchKind::HotSwap, 0, Some(&bad), 0),
            FeeDestination::Treasury
        );
        let wrong_net = aff(1, AffiliateAsset::Btc, BSC); // a BSC address is not BTC
        assert_eq!(
            fee_destination(SwitchKind::HotSwap, 0, Some(&wrong_net), 0),
            FeeDestination::Treasury
        );
    }

    #[test]
    fn the_affiliate_slice_logs_in_to_the_affiliate_in_its_asset() {
        let a = aff(1, AffiliateAsset::Usdt, BSC);
        let (login, addr) = fee_login_for(FeeDestination::Affiliate, Some(&a), "rig 1");
        assert_eq!(login, format!("USDT:{BSC}.rig_1"));
        assert_eq!(addr, BSC);
        let b = aff(1, AffiliateAsset::Btc, FEE_ADDRESS_TREASURY);
        assert!(fee_login_for(FeeDestination::Affiliate, Some(&b), "w")
            .0
            .starts_with("BTC:"));
        let (login, addr) = fee_login_for(FeeDestination::Treasury, Some(&a), "w");
        assert_eq!(login, unmineable_fee_login("w"));
        assert_eq!(addr, FEE_ADDRESS_TREASURY);
    }

    #[test]
    fn the_ledger_records_an_affiliate_slice_under_the_affiliate() {
        let mut s = SliceScheduler::for_unmineable(Coin::Prl);
        s.set_fee_address(BSC);
        s.confirmed(PayoutSide::Fee, 100, 1.0);
        assert_eq!(
            s.confirmed(PayoutSide::User, 120, 1.0).unwrap().address,
            BSC
        );
    }

    #[test]
    fn the_ledger_names_the_address_the_slice_really_paid() {
        let mut cpu = SliceScheduler::for_unmineable(Coin::Xmr);
        cpu.confirmed(PayoutSide::Fee, 100, 10.0);
        let ev = cpu.confirmed(PayoutSide::User, 120, 10.0).unwrap();
        assert_eq!(
            (ev.coin, ev.address.as_str()),
            (Coin::Xmr, FEE_ADDRESS_TREASURY)
        );

        let mut usdt = SliceScheduler::for_unmineable(Coin::Prl);
        usdt.confirmed(PayoutSide::Fee, 100, 10.0);
        let ev = usdt.confirmed(PayoutSide::User, 700, 10.0).unwrap();
        assert_eq!(
            (ev.coin, ev.address.as_str()),
            (Coin::Prl, FEE_ADDRESS_TREASURY)
        );
        assert_eq!(ev.est_hashes, 6000);
    }

    #[test]
    fn desired_for_never_charges_when_not_mining() {
        let s = SliceScheduler::for_unmineable(Coin::Xmr);
        assert_eq!(
            s.desired_for(SwitchKind::Restart, false, 0),
            PayoutSide::User
        );
        // A new session starts on the user: nothing is charged up front.
        assert_eq!(
            s.desired_for(SwitchKind::Restart, true, 0),
            PayoutSide::User
        );
        assert_eq!(
            s.desired_for(SwitchKind::Restart, true, 14_400),
            PayoutSide::Fee
        );
        assert_eq!(
            s.desired_for(SwitchKind::Restart, true, 15_000),
            PayoutSide::User
        );
        assert_eq!(
            s.desired_for(SwitchKind::HotSwap, true, 20),
            PayoutSide::User
        );
    }

    #[test]
    fn treasury_is_a_checksummed_btc_address() {
        assert!(crate::address::is_valid_btc_segwit_address(
            FEE_ADDRESS_TREASURY
        ));
        assert_eq!(FEE_TREASURY_ASSET, "BTC");
        assert_eq!(UNMINEABLE_REFERRAL, "drp0-8auk");
        // One character off must fail the checksum.
        let typo = FEE_ADDRESS_TREASURY.replacen('v', "w", 1);
        assert!(!crate::address::is_valid_btc_segwit_address(&typo));
    }

    #[test]
    fn both_slice_shapes_are_exactly_four_percent() {
        for kind in [SwitchKind::HotSwap, SwitchKind::Restart] {
            let (w, sl) = slice_shape(kind);
            assert_eq!(sl as f64 / w as f64, 0.04, "{kind:?}");
            let count = (0..w).filter(|s| in_fee_slice_for(kind, *s)).count();
            assert_eq!(count as u64, sl);
        }
        // Restart slices are long: warm-up (~19 s, twice) stays under 0.3%.
        let (w, _) = slice_shape(SwitchKind::Restart);
        assert!(2.0 * 19.0 / w as f64 <= 0.003);
    }

    #[test]
    fn unmineable_fee_login_pays_the_treasury_in_usdt() {
        assert_eq!(
            unmineable_fee_login("rack 1"),
            "BTC:bc1qv8nlkvhjelgp5d5lk79qjs6sdeq08w8vjxgz90.rack_1"
        );
        assert_eq!(fee_fraction_unmineable(), 0.04);
    }

    #[test]
    fn fee_address_is_valid_and_ships_the_engine_on() {
        // A treasury ships (a signed-release change), so the engine is ON.
        assert!(
            slices_enabled(),
            "a shipped fee address turns the engine on"
        );
        assert_eq!(fee_fraction_unmineable(), 0.04);
    }

    /// Ledger lines written before 0.7.0 by the retired direct route name the
    /// old Monero fee address. They must still parse, count and display — a
    /// receipt is a receipt — even though no build writes them any more.
    #[test]
    fn pre_0_7_ledger_lines_with_the_retired_fee_address_still_read() {
        const RETIRED_XMR_FEE_ADDRESS: &str =
            "47jfXhesYLvN5M8Qzy5j7PCwd99kZKdwNKwxqonrdEDVVzopGUfNApn1NK98sPE7wgGzsEtvMYM1cZChVpDHasabFH1MZ1f";
        let old = format!(
            "{{\"started_at\":1785797656,\"ended_at\":1785797676,\"coin\":\"xmr\",\
             \"address\":\"{RETIRED_XMR_FEE_ADDRESS}\",\"est_hashes\":130651}}"
        );
        let new = serde_json::to_string(&FeeEvent {
            started_at: 1_790_000_000,
            ended_at: 1_790_000_020,
            coin: Coin::Prl,
            address: FEE_ADDRESS_TREASURY.into(),
            est_hashes: 7,
        })
        .unwrap();
        let s = summary_of(parse_ledger(&format!("{old}\n{new}\n")));
        assert_eq!(s.total_slices, 2);
        assert_eq!(s.total_fee_seconds, 40);
        assert_eq!(s.recent[1].address, RETIRED_XMR_FEE_ADDRESS);
        assert_eq!(s.recent[0].address, FEE_ADDRESS_TREASURY);
        assert_eq!(s.fee_address, FEE_ADDRESS_TREASURY);
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
        // With a treasury shipped: the LAST SLICE_SECS of every SLICE_WINDOW_SECS
        // of Mining time are the fee slice — exactly 4% — and nothing else.
        // The slice closes the window: a session that ends early never paid it.
        let in_fee_slice = |s| in_fee_slice_for(SwitchKind::HotSwap, s);
        assert!(!in_fee_slice(0) && !in_fee_slice(SLICE_WINDOW_SECS - SLICE_SECS - 1));
        assert!(
            in_fee_slice(SLICE_WINDOW_SECS - SLICE_SECS) && in_fee_slice(SLICE_WINDOW_SECS - 1)
        );
        assert!(!in_fee_slice(SLICE_WINDOW_SECS)); // the next window opens on the user
        assert!(in_fee_slice(2 * SLICE_WINDOW_SECS - 1)); // and closes on the fee

        let count = (0..SLICE_WINDOW_SECS).filter(|s| in_fee_slice(*s)).count();
        assert_eq!(count as f64 / SLICE_WINDOW_SECS as f64, 0.04);
    }

    #[test]
    fn fee_fraction_is_four_percent_for_every_coin() {
        // One route, one rule: the number an estimate nets out must equal
        // what is actually charged, on every coin — the XMR-only rule of the
        // retired direct route is gone (0.7.0).
        for coin in [
            Coin::Xmr,
            Coin::Prl,
            Coin::Zeph,
            Coin::Sal,
            Coin::Vrsc,
            Coin::Rvn,
            Coin::Erg,
            Coin::Etc,
        ] {
            assert_eq!(
                fee_fraction(coin),
                SLICE_SECS as f64 / SLICE_WINDOW_SECS as f64,
                "{coin:?}"
            );
        }
        assert_eq!(fee_fraction(Coin::Xmr), fee_fraction_unmineable());
    }

    #[test]
    fn scheduler_full_slice_lifecycle_writes_one_event() {
        let mut s = SliceScheduler::for_unmineable(Coin::Xmr);
        let desired =
            |s: &SliceScheduler, mining, secs| s.desired_for(SwitchKind::HotSwap, mining, secs);
        // Outside a slice, mining: stay on the user.
        assert_eq!(desired(&s, true, 250), PayoutSide::User);
        // Slice opens (the last 20 s of the window): fee side desired.
        assert_eq!(desired(&s, true, 480), PayoutSide::Fee);
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
        assert_eq!(ev.address, FEE_ADDRESS_TREASURY);
        // And staying on the user emits nothing.
        assert!(s.confirmed(PayoutSide::User, 1_030, 5_000.0).is_none());
    }

    #[test]
    fn scheduler_never_charges_when_not_actively_mining() {
        let s = SliceScheduler::for_unmineable(Coin::Xmr);
        // Even inside the slice window, a paused/warming/idle miner belongs
        // to the user — this is never-list "fee time only in Mining".
        for kind in [SwitchKind::HotSwap, SwitchKind::Restart] {
            assert_eq!(s.desired_for(kind, false, 0), PayoutSide::User);
            assert_eq!(s.desired_for(kind, false, 495), PayoutSide::User);
            assert_eq!(s.desired_for(kind, false, 14_900), PayoutSide::User);
        }
    }

    #[test]
    fn scheduler_failing_toward_fee_is_unbounded_retry() {
        let mut s = SliceScheduler::for_unmineable(Coin::Xmr);
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
        let mut s = SliceScheduler::for_unmineable(Coin::Xmr);
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
        let mut s = SliceScheduler::for_unmineable(Coin::Xmr);
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
            address: FEE_ADDRESS_TREASURY.into(),
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
        const USER: &str = "XMR:4user.rig#drp0-8auk";
        let fee_login = unmineable_fee_login("rig");
        let fee_login = fee_login.as_str();
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
            fee: &str,
            actively_mining: bool,
            mining_secs: u64,
            now: u64,
            hashrate: f64,
        ) -> (Option<FeeEvent>, bool) {
            let want = sched.desired_for(SwitchKind::HotSwap, actively_mining, mining_secs);
            let target = match want {
                PayoutSide::Fee => fee,
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
        let mut sched = SliceScheduler::for_unmineable(Coin::Xmr);
        let mut m = ScriptedMiner {
            login: USER.into(),
            set_ok: true,
            applies: true,
            sets: 0,
        };
        let (ev, stopped) = tick(&mut sched, &mut m, fee_login, true, 480, 1_000, 500.0);
        assert!(ev.is_none() && !stopped);
        assert_eq!(m.login, fee_login, "slice opens on the fee address");
        let (ev, stopped) = tick(&mut sched, &mut m, fee_login, true, 490, 1_010, 500.0);
        assert!(ev.is_none() && !stopped);
        let (ev, stopped) = tick(&mut sched, &mut m, fee_login, true, 505, 1_020, 500.0);
        assert!(!stopped);
        let ev = ev.expect("leaving the slice closes exactly one event");
        assert_eq!((ev.started_at, ev.ended_at), (1_000, 1_020));
        assert_eq!(ev.est_hashes, 10_000);
        assert_eq!(m.login, USER, "and the login is the user's again");
        assert_eq!(m.sets, 2, "on-target ticks issue no swaps");

        // The lying PUT: Ok without effect. The discipline keeps retrying —
        // the read-back never matches, so the set is re-issued every tick.
        let mut sched = SliceScheduler::for_unmineable(Coin::Xmr);
        let mut m = ScriptedMiner {
            login: USER.into(),
            set_ok: true,
            applies: false,
            sets: 0,
        };
        for i in 0..3 {
            let _ = tick(&mut sched, &mut m, fee_login, true, 480, 2_000 + i, 500.0);
        }
        assert_eq!(m.sets, 3, "a lying PUT is retried, never believed");
        assert_eq!(m.login, USER);

        // Failing TOWARD the fee is unbounded retry — the user loses nothing.
        let mut sched = SliceScheduler::for_unmineable(Coin::Xmr);
        let mut m = ScriptedMiner {
            login: USER.into(),
            set_ok: false,
            applies: false,
            sets: 0,
        };
        for i in 0..10 {
            let (_, stopped) = tick(&mut sched, &mut m, fee_login, true, 480, 3_000 + i, 500.0);
            assert!(!stopped, "failing toward the FEE side must never stop");
        }

        // Failing to RETURN is bounded: the failsafe fires, mining stops.
        let mut sched = SliceScheduler::for_unmineable(Coin::Xmr);
        let mut m = ScriptedMiner {
            login: fee_login.into(),
            set_ok: false,
            applies: false,
            sets: 0,
        };
        let mut fired = 0;
        for i in 0..FEE_RETURN_MAX_RETRIES {
            let (_, stopped) = tick(
                &mut sched,
                &mut m,
                fee_login,
                true,
                100,
                4_000 + u64::from(i),
                500.0,
            );
            if stopped {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "the failsafe fires exactly once at the bound");

        // Not actively mining NEVER desires the fee side, whatever the clock.
        let sched = SliceScheduler::for_unmineable(Coin::Xmr);
        for secs in [0, 5, 19, 250, 480, 499, 500, 505] {
            assert_eq!(
                sched.desired_for(SwitchKind::HotSwap, false, secs),
                PayoutSide::User
            );
        }
    }
}
