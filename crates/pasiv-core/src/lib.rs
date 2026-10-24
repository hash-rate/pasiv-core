// SPDX-License-Identifier: GPL-3.0-only
//! # pasiv-core — the code that touches money
//!
//! The open core of [Pasiv](https://pasiv.network), extracted so the parts of
//! a desktop miner a user has to trust with their earnings are auditable:
//!
//! - [`fee`] — the 4% fee engine: the compile-time fee address (the BTC
//!   treasury on unMineable), the structural time-slice schedules that make
//!   the percentage a pure function of Mining time, and the append-only
//!   ledger format every slice is written to. 4% on every coin.
//! - [`unmineable`] — Pasiv's one payout route (the default from 0.5.0, the
//!   only one from 0.7.0): the pool login, the account-wide USDT/BTC address
//!   rules, the per-lane native payout (PRL for the GPU lane, XMR for the CPU
//!   lane), and the pool hosts. The pool pays the user's own address; Pasiv
//!   never holds funds.
//! - [`coins`] — the coin roster: for every supported coin, which miner runs
//!   it, the unMineable algorithm (and so the pool) it is mined under, and
//!   its native payout-address rule.
//! - [`address`] — the payout validators (paste-time shape checks; the pool's
//!   `mining.authorize` is always the authoritative check).
//! - [`state`] — the mining state machine. The fee engine is driven by it,
//!   which is what makes "never charge a paused user" structural rather than
//!   promised.
//! - [`profit`] — the earnings-estimate math, computed on the user's
//!   take-home (net of the fee), never on gross.
//! - [`earnings`] — the $/day estimation formula, shared by every surface.
//! - [`hardware`] — CPU/GPU detection and the thread-count decision.
//! - [`types`] — the shared miner/coin types.
//! - [`verus`] (macOS) — the complete in-process Verus mining engine:
//!   VerusHash V2.2, stratum client, worker pool, share submission. Verus
//!   retired as a Pasiv coin in 0.7.0; the engine stays as reference.
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
