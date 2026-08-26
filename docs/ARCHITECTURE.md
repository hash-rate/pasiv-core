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

- **`fee`** — the 4% fee engine. The compile-time fee address, the structural
  time-slice schedule (`(mining_secs % 500) < 20` — a pure function of Mining
  time, so a paused or stopped miner is uncharge­able by construction), the
  append-only JSONL ledger every slice is written to, and the
  `SliceScheduler` enforcement state machine with its stop-don't-park
  failsafe. `docs/FEES.md` is the binding contract this module implements.
- **`coins`** — the roster: for every supported coin, exactly which miner
  runs it, which pool it submits to, its address rule, its dashboard link.
  Adding a coin is one row here.
- **`address`** — payout validators (paste-time shape checks; the pool's
  `mining.authorize` remains the authoritative check).
- **`state`** — the miner state machine. The fee engine is driven by it,
  which is what makes "never charge a non-mining user" structural.
- **`profit`** — the Auto/Max-Profit ranking math, computed on the user's
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
claim a node against the cloud API, mine, enforce the fee slices with the
same level-triggered reconcile the crate's conformance test encodes, write
the same ledger. Its cloud endpoint, key, and pool are defaults overridable
by environment (`PASIVD_API_URL` / `PASIVD_ANON_KEY` / `PASIVD_POOL`), so an
auditor or a fork can point it anywhere without patching source.

Modules: `main.rs` (config, claim, run loop), `doctor.rs` (the PASS/WARN/FAIL
diagnostic pass), `xmrig.rs` (fetch/verify/spawn + thin HTTP wrappers over
`pasiv_core::xmrig`).

## The boundary

The proprietary apps (desktop GUI, phone companions, the cloud functions,
release signing, the website) consume this crate as a **rev-pinned git
dependency**. The pin is the trust mechanism: a push to this repository
changes nothing a user runs until the app repository deliberately bumps the
pin — a visible diff there, gated by its own tests, including a literal pin
of the fee address and slice constants that fails any bump which moves them.
The `pasivd-linux-x64` attached to every release is built from this
repository at that same pinned rev, tested first, then minisign-signed with
the same key the desktop updater trusts.

What stays closed, said plainly: the product — UI, companions, cloud, release
signing. Open core, not open everything. What can never change silently: the
items in `docs/FEES.md`.
