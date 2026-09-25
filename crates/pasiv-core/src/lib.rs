// SPDX-License-Identifier: GPL-3.0-only
//! # pasiv-core — the code that touches money
//!
//! The open core of [Pasiv](https://pasiv.network), extracted so the parts of
//! a desktop miner a user has to trust with their earnings are auditable:
//!
//! - [`fee`] — the 4% fee engine: the compile-time fee addresses (the BTC
//!   treasury on the unMineable route, the Monero address on the direct
//!   route), the structural time-slice schedules that make the percentage a
//!   pure function of Mining time, and the append-only ledger format every
//!   slice is written to.
//! - [`unmineable`] — the USDT payout route, the default from 0.5.0: the pool
//!   login, the BSC/TRON address rules, and the pool hosts. The pool pays the
//!   user's own address; Pasiv never holds funds.
//! - [`coins`] — the coin/pool roster for the direct route: exactly where
//!   hashes are submitted for every supported coin, and each coin's
//!   payout-address rule.
//! - [`address`] — the payout validators (paste-time shape checks; the pool's
//!   `mining.authorize` is always the authoritative check).
//! - [`state`] — the mining state machine. The fee engine is driven by it,
//!   which is what makes "never charge a paused user" structural rather than
//!   promised.
//! - [`profit`] — the Auto/Max-Profit ranking math (direct route only),
//!   computed on the user's take-home (net of the fee), never on gross.
//! - [`earnings`] — the $/day estimation formula, shared by every surface.
//! - [`hardware`] — CPU/GPU detection and the thread-count decision.
//! - [`types`] — the shared miner/coin types.
//! - [`verus`] (macOS) — the complete in-process Verus mining engine:
//!   VerusHash V2.2, stratum client, worker pool, share submission.
//! - [`xmrig`] — the XMRig local-API contract (runtime config, URLs, parsers)
//!   both consumers drive the bundled miner through.
//!
//! The binding product commitments live in `docs/FEES.md` (the never-list).
//! The `pasivd` workspace member is the complete, runnable headless daemon
//! built on this crate — the end-to-end money path, compilable by anyone.

pub mod address;
pub mod coins;
pub mod earnings;
pub mod etchash;
pub mod fee;
pub mod hardware;
pub mod profit;
pub mod state;
pub mod types;
pub mod unmineable;
#[cfg(target_os = "macos")]
pub mod verus;
pub mod xmrig;
