# pasiv-core

**The open core of [Pasiv](https://pasiv.network) — the code that touches
money.** GPL-3.0-only.

Pasiv is a one-button, non-custodial desktop miner. Its category is full of
software that deserves suspicion, so the parts of Pasiv a user has to trust
with their earnings are open source in this repository — readable, buildable,
and pinned by tests:

| What | Where | Why it's here |
|---|---|---|
| The 4% fee engine | [`crates/pasiv-core/src/fee.rs`](crates/pasiv-core/src/fee.rs) | The compile-time fee address (the BTC treasury), the structural time-slice schedules (20 s in 500 s for a live-switching miner, 10 min in 4 h 10 min for one that must restart — exactly 4% either way, on every coin, only in the `Mining` state), and the append-only ledger every slice is written to |
| The payout route | [`crates/pasiv-core/src/unmineable.rs`](crates/pasiv-core/src/unmineable.rs) | Pasiv's one route (the default from 0.5.0, the only one from 0.7.0): the unMineable login (`USDT:<your address>.<worker>#<referral>`, `BTC:…`, or natively `PRL:…` / `XMR:…` per lane), the address rules (checksums verified where the chain has one), and the pool hosts — the pool pays *your* address; Pasiv never holds funds |
| The coin roster | [`crates/pasiv-core/src/coins.rs`](crates/pasiv-core/src/coins.rs) | Every coin Pasiv mines, the unMineable algorithm (and so the pool) it is mined under, and its native payout-address rule (Ravencoin and Ergo retired with Pasiv 0.6.11 on 2026-10-23; Zephyr, Salvium and Verus with 0.7.0, on the 1 November 2026 date, together with the legacy direct route) |
| Payout validators | [`crates/pasiv-core/src/address.rs`](crates/pasiv-core/src/address.rs) | The paste-time rules for every payout address — USDT on BSC/TRON, and every coin's own |
| The mining state machine | [`crates/pasiv-core/src/state.rs`](crates/pasiv-core/src/state.rs) | Drives both the UI and the fee counter — "never charge a paused user" is structural, not promised |
| Earnings-estimate math | [`crates/pasiv-core/src/profit.rs`](crates/pasiv-core/src/profit.rs) | Prices a coin on the user's **take-home** (net of the fee), never on gross |
| The $/day formula | [`crates/pasiv-core/src/earnings.rs`](crates/pasiv-core/src/earnings.rs) | One function, every surface — desktop, daemon, phone totals |
| **`pasivd`, the headless daemon** | [`pasivd/`](pasivd/) | A complete, runnable end-to-end money path: fetches and SHA-256-verifies the official XMRig, mines Monero on the CPU on unMineable paid to *your* address — in USDT or BTC when your account has one, otherwise natively in XMR — takes the same 4% slice to the same treasury as the desktop, writes the same ledger, and stops mining rather than ever stick on the fee address |

The binding product commitments — including the never-list — are in
[`docs/FEES.md`](docs/FEES.md).

## What is *not* open, and why

Pasiv is an **open-core** project, and says so plainly. The desktop GUI
application, the iOS/Android companion apps, the Pasiv Cloud backend, the
website, and the release/signing infrastructure are proprietary — they are the
product that funds the work. What's open is everything a user must trust:
where hashes go, what the fee is, how it's charged, and how it's recorded.

## How to verify the claims

- **Build the daemon yourself:** `cd pasivd && cargo build --release`. It runs
  against the same cloud and pool the shipped binary does — and the API
  endpoint is overridable by environment (`PASIVD_API_URL`,
  `PASIVD_ANON_KEY`), so nothing forces a fork through Pasiv's backend.
- **Read the fee engine** and check the shipped behaviour against it: the
  fee address is the one constant in `fee.rs` (the BTC treasury), and your
  local `fee-ledger.jsonl` uses the format in the same file.
- **Run the tests:** `cargo test --workspace`. The slice schedule, the 4%
  ratio, the ledger format, the address rules, and the ranking math are all
  pinned.

## Licensing

- This repository: **GPL-3.0-only** (see [COPYING](COPYING)).
- Pasiv's own applications use this code under a separate proprietary licence
  from the copyright holder — standard dual licensing. The GPL binds
  licensees; it does not bind the owner.
- Because of that, contributions require a copyright grant — see
  [CONTRIBUTING.md](CONTRIBUTING.md) before opening a PR.

## Relationship to the shipped products

The proprietary apps consume this crate as a pinned git dependency, so the
open constants and the shipped behaviour cannot drift silently: changing the
fee addresses or the slice schedules requires a commit here *and* a new signed
release there, each leaving a public diff. `pasivd` binaries attached to
[Pasiv releases](https://github.com/hash-rate/pasiv-releases/releases) are
built from this source.

**Why not on crates.io?** Deliberate, two reasons. The Verus engine depends on
the vendored `verushash-rs` (its C++ is patched for arm64/clang), and a path
dependency cannot be published — we would have to publish a renamed fork of a
third-party crate, which helps nobody. More importantly, the git-rev pin *is*
the transparency mechanism: the apps name the exact commit they build against,
and bumping it leaves a visible diff on both sides. A crates.io version would
add a resolution layer without adding any verifiability. Revisit if the
verushash patches are ever upstreamed.
