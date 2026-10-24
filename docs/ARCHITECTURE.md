# Architecture

What lives in this repository, how it fits together, and where the boundary
with the proprietary apps runs. The one-line version: **this is the code that
touches money**, published so nobody has to take it on faith.

## The workspace

```
crates/pasiv-core     the library — every money-path decision
pasivd/               a complete headless mining daemon built on it
vendor/verushash-rs   vendored VerusHash C++ (MIT, arm64/clang-patched)
docs/                 FEES.md (the binding never-list) + these notes
```

## pasiv-core, module by module

- **`fee`** — the 4% fee engine. The compile-time fee address (the BTC
  treasury on unMineable; 4% on every coin since 0.7.0 retired the legacy
  direct route and its Monero-only rule), the structural time-slice schedules (`(mining_secs % 500) < 20` for a
  live-switching miner, 10 min in every 4 h 10 min for one that must restart —
  a pure function of Mining time, so a paused or stopped miner is
  uncharge­able by construction), the
  append-only JSONL ledger every slice is written to, and the
  `SliceScheduler` enforcement state machine with its stop-don't-park
  failsafe. `docs/FEES.md` is the binding contract this module implements.
- **`unmineable`** — the one payout route (default from 0.5.0, only route
  from 0.7.0): the pool login (`USDT:<address>.<worker>#<referral>`, `BTC:…`,
  or natively `PRL:…` / `XMR:…` per lane — `PayoutAsset::native_for`), the
  address rules, and the pool hosts. The pool pays the user's own address.
- **`coins`** — the roster: for every supported coin, which miner runs it,
  the unMineable algorithm (and so the pool) it is mined under, its native
  payout-address rule. Adding a coin is one row here. Ravencoin and Ergo
  retired with Pasiv 0.6.11 (2026-10-23); Zephyr, Salvium and Verus with
  0.7.0 (the 1 November 2026 date), together with the direct route.
- **`address`** — payout validators (paste-time shape checks; the pool's
  `mining.authorize` remains the authoritative check).
- **`state`** — the miner state machine. The fee engine is driven by it,
  which is what makes "never charge a non-mining user" structural.
- **`profit`** — the earnings-estimate math, computed on the user's
  take-home (net of the fee), never on gross.
- **`earnings`** — the $/day estimate. Every surface — desktop, phone card,
  headless node — computes it with this one function.
- **`hardware`** — CPU/GPU detection and the thread-count decision (it
  decides what "Full power" launches, which makes it money-adjacent).
- **`xmrig`** — the contract for driving the bundled XMRig: local-API URLs,
  the runtime config (unrestricted, loopback, token in a 0600 file — never
  argv), and the response parsers. Both consumers drive it through this one
  module; they drifted when each had a copy, and a drifted flag once shipped
  a daemon that could not mine.
- **`verus`** *(macOS)* — the complete in-process Verus engine: VerusHash
  V2.2, the stratum client, the worker pool, share submission. The one coin
  mined without a sidecar; see `docs/VERUS.md` for the proven byte layout.
- **`types`** — the shared miner/coin types.

## pasivd

A complete, runnable consumer of the crate — the end-to-end money path anyone
can compile and run: fetch-and-verify XMRig (double sha256, atomic unpack),
claim a node against the cloud API, mine on unMineable (paid in USDT/BTC
when the owner's account has such an address, otherwise natively in XMR to
the owner's Monero address), enforce the fee slices with the same
level-triggered reconcile the crate's conformance test encodes, write the
same ledger. Its cloud endpoint and key are defaults overridable by
environment (`PASIVD_API_URL` / `PASIVD_ANON_KEY`), so an auditor or a fork
can point it at another backend without patching source; the pool is
unMineable's and is not overridable.

Modules: `main.rs` (config, claim, run loop), `doctor.rs` (the PASS/WARN/FAIL
diagnostic pass), `xmrig.rs` (fetch/verify/spawn + thin HTTP wrappers over
`pasiv_core::xmrig`).

Resilience (pasivd 0.1.9): the daemon keeps mining whatever the cloud does. A
failed or revoked startup poll logs loudly and mining starts on the payout the
node last heard (cached atomically in its state directory — the owner's own
address); the cloud is re-polled every 60 s until it answers, then hourly, and
a revoked node warns hourly and keeps hashing. Only a node with no payout
anywhere waits. Every cloud call has a 15 s request / 5 s connect timeout and
runs on its own task, so a hung request can delay a push but never a miner
respawn or the end of a fee slice; the miner loop's only awaits are loopback
calls to xmrig with a 3 s timeout. Config and payout-cache writes are
temp-file + fsync + rename, 0600, and skipped when unchanged; the xmrig
binary is replaced only after its replacement is downloaded and verified; and
an update's rollback health is judged by hashing, not by a push succeeding.
The shared crate's hardware probes are bounded too: any shell-out
(`nvidia-smi`, `lspci`, `powershell`, `sysctl`, `reg`) is killed after 5 s and
GPU detection answers with its last good result rather than hanging setup.

## The boundary

The proprietary apps (desktop GUI, phone companions, the cloud functions,
release signing, the website) consume this crate as a **rev-pinned git
dependency**. The pin is the trust mechanism: a push to this repository
changes nothing a user runs until the app repository deliberately bumps the
pin — a visible diff there, gated by its own tests, including a literal pin
of the Monero fee address and the CPU slice constants that fails any bump
which moves them.
The `pasivd-linux-x64` attached to every release is built from this
repository at that same pinned rev, tested first, then minisign-signed with
the same key the desktop updater trusts.

What stays closed, said plainly: the product — UI, companions, cloud, release
signing. Open core, not open everything. What can never change silently: the
items in `docs/FEES.md`.
