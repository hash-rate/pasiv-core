# pasiv-core

**The open core of [Pasiv](https://pasiv.network) — the code that touches
money.** GPL-3.0-only.

Pasiv is a one-button, non-custodial desktop miner. Its category is full of
software that deserves suspicion, so the parts of Pasiv a user has to trust
with their earnings are open source in this repository — readable, buildable,
and pinned by tests:

| What | Where | Why it's here |
|---|---|---|
| The 4% fee engine | [`crates/pasiv-core/src/fee.rs`](crates/pasiv-core/src/fee.rs) | The compile-time fee address, the structural time-slice schedule (`(mining_secs % 500) < 20` — exactly 4%, only in the `Mining` state), and the append-only ledger every slice is written to |
| The coin/pool roster | [`crates/pasiv-core/src/coins.rs`](crates/pasiv-core/src/coins.rs) | Exactly where hashes are submitted for every supported coin — pools, ports, algorithms |
| Payout validators | [`crates/pasiv-core/src/address.rs`](crates/pasiv-core/src/address.rs) | The paste-time rules for every coin's payout address |
| The mining state machine | [`crates/pasiv-core/src/state.rs`](crates/pasiv-core/src/state.rs) | Drives both the UI and the fee counter — "never charge a paused user" is structural, not promised |
| Auto/Max-Profit ranking math | [`crates/pasiv-core/src/profit.rs`](crates/pasiv-core/src/profit.rs) | Ranks coins on the user's **take-home** (net of the fee), never on gross |
| The $/day formula | [`crates/pasiv-core/src/earnings.rs`](crates/pasiv-core/src/earnings.rs) | One function, every surface — desktop, daemon, phone totals |
| **`pasivd`, the headless daemon** | [`pasivd/`](pasivd/) | A complete, runnable end-to-end money path: fetches and SHA-256-verifies the official XMRig, mines to *your* address, takes the same 4% slice to the same fee address, writes the same ledger, and stops mining rather than ever stick on the fee address |

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
  against the same cloud and pools the shipped binary does — and both the API
  endpoint and the pool are overridable by environment (`PASIVD_API_URL`,
  `PASIVD_ANON_KEY`, `PASIVD_POOL`), so nothing forces a fork through Pasiv's
  backend.
- **Read the fee engine** — it's 200 lines — and check the shipped behaviour
  against it: the fee address in the app's Fees panel is the constant in
  `fee.rs`, and your local `fee-ledger.jsonl` uses the format in the same
  file.
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
fee address or the slice schedule requires a commit here *and* a new signed
release there, each leaving a public diff. `pasivd` binaries attached to
[Pasiv releases](https://github.com/hash-rate/pasiv-releases/releases) are
built from this source.
