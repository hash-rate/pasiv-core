# Testing

What is tested in THIS repository, how to run it, and which invariants the
suite is built to hold. (The proprietary desktop and phone apps have their own
suites in their own repositories; this file describes only the open core.)

## Run everything

```sh
cargo test --workspace          # pasiv-core + pasivd
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI (`.github/workflows/ci.yml`) runs the same three, plus coverage floors
(`cargo llvm-cov`) that ratchet up:

- `pasiv-core` — floor 90 (ubuntu) and 80 (macOS, where the Verus engine
  compiles; it is `cfg`'d out everywhere else, so the macOS number is the only
  honest one for it).
- `pasivd` — floor 26. The uncovered mass is the async run loop and network
  commands; the pure decisions are what the floor protects.

A `musl` job builds the exact static `pasivd-linux-x64` the release pipeline
attaches, asserts it is really static, and prints its sha256. An
`installer-mirror` job asserts `pasivd/install.sh` is byte-identical to the
script served at `pasiv.network/pasivd.sh` — and fails CLOSED if the site is
unreachable.

## The invariants the suite pins

**The fee engine** (`crates/pasiv-core/src/fee.rs`): the structural 4% —
`in_fee_slice` is a pure function of Mining time; the `SliceScheduler` never
charges a non-mining user, emits exactly one ledger event per slice on the
falling edge, retries unbounded toward the FEE side (the user loses nothing)
and stops the miner after bounded failures toward the USER side (mining never
continues on the fee address). `reconcile_discipline_conformance` encodes the
exact drive loop both consumers must implement: read back the miner's actual
login, confirm on match, correct on mismatch.

**The XMRig contract** (`crates/pasiv-core/src/xmrig.rs`): the runtime config
is unrestricted (a restricted API 403s the fee swap — a bug that shipped once),
loopback-only, written 0600 before the token lands in it, and the token never
travels in argv.

**Addresses** (`address.rs`): every payout validator, including the shapes
that must be *rejected* (integrated XMR addresses, legacy Salvium, paymail).

**The state machine** (`state.rs`): the transition table, including that
`Mining` — the only state in which fee time accrues — is entered by `Hashing`
alone, and that the fee failsafe exits it into an Idle that says so.

**pasivd** (`pasivd/src/main.rs` tests): the fee target inside a slice is the
shared crate's address; the slice is the *first 20 seconds* of each window
(the offset, not just the ratio); the device config and the miner runtime
config are owner-only; the xmrig command line never carries the API token.

## Conventions

- Extract the risky decision into a pure function and test that; keep the
  impure caller thin. Everything above follows this shape.
- Mutation-test new guards: reintroduce the bug, watch the new test fail and
  the old ones stay green. A guard you have not seen fail is not a guard.
- Network tests are `#[ignore]`d and run manually; CI is hermetic apart from
  the deliberate installer-mirror check.
