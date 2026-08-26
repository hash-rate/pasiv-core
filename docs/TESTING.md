# Testing charter

*Why this exists: on 2026-08-04 three regressions were live at once — sign-in
broken since 0.4.8, the Pearl payout field unreachable, and a blocked GPU
silently stopping all mining — and the suite was green throughout. The problem
was never test count. It was that the tests asserted the wrong things.*

---

## The rule

**A test added with a fix must be proven to fail without the fix.**

Not "should fail". Actually run it red:

```bash
git stash push src/the_file_you_fixed.rs   # or .tsx
cargo test the_new_test        # must FAIL
git stash pop
cargo test the_new_test        # must PASS
```

A guard that passes either way is worse than none: it costs maintenance and buys
confidence it hasn't earned. Every regression test in this repo has been through
that loop — do the same or don't claim it's a guard.

---

## The four ways bugs have actually shipped here

Every regression we have shipped falls into one of these. When adding a test,
work out which class the bug belongs to, because each needs a different shape of
test.

### 1. External contract drift — we asserted against our own imagination

The single most expensive class.

- **SRBMiner stats**: the adapter read `hashrate_total_now` and a top-level
  `shares`. SRBMiner has never emitted either. Unit tests passed for weeks
  because they asserted the same invention. Live: 0 H/s and 0 shares while the
  GPUs hashed, so the state machine never left "warming up" and the fee
  scheduler (gated on `accepted > 0`) never ran.
- **SIWE address casing**: 0.4.8 lowercased the address. EIP-4361 requires the
  EIP-55 checksum, and `@spruceid/siwe-parser` — what MetaMask ships — throws on
  a lowercase one. MetaMask stopped recognising the request as a sign-in at all.
- **Monero "verify on pool" URL**: fixed three separate times.

**What to do:** assert against a *captured real payload*, committed as a
fixture, not a hand-written approximation.

- `src-tauri/tests/fixtures/xmrig-summary-live.json` — captured from a real
  XMRig 6.26.0 that was mining
- `src-tauri/tests/fixtures/srbminer-status-live.json` — captured from a real
  SRBMiner on three GPUs
- `src/supabase.test.ts` — runs our real SIWE message through MetaMask's own
  `detectSIWE` / `isValidSIWEOrigin`

Re-capture the fixture when bumping a miner, and let the test fail if upstream
moved. **Pair each fixture test with a negative one** that feeds the shape we
used to imagine and asserts it reads as zeros — so a rewrite-from-guess fails
loudly instead of silently reporting an idle miner.

### 2. State combinations — the happy path was tested, the matrix was not

- A **blocked GPU stopped all mining**, CPU included, because `do_start`
  rejected the start if *any* coin needed the GPU.
- The **Pearl payout field vanished** whenever the GPU was not `Ready` — on a
  Smart App Control machine, permanently, since SAC cannot be re-enabled.

Neither needed a clever test. Only one that *enumerated* the combinations.

**What to do:** when behaviour depends on an enum × an enum, loop the product
and assert the invariant in every cell.

- `src-tauri/src/commands/mod.rs` → `gpu_state_matrix_never_lets_the_gpu_block_cpu_mining`
  (6 GPU states × 5 coins × 2 gpu-coin choices)
- `src/App.test.tsx` → the Pearl payout field, asserted in all 6 GPU states

Adding a variant to `GpuStatus` or the coin roster should make a matrix test
fail until someone decides what the new cell means. That is the point.

### 3. Platform-specific — it only breaks on the OS nobody tests

A miner that outlived the app, an unasked UAC prompt, Full power
oversubscribing the CPU, the Linux tray icon, Smart App Control. Windows and
Linux produced most of our regressions.

Windows CI **compiles** tests but does not run them (`--no-run`): the test
binary dies on load with 0xC0000139 (STATUS_ENTRYPOINT_NOT_FOUND) because
`pasiv_lib`'s crate-type includes `cdylib`. So the platform with the most
regressions is the one platform whose assertions never execute.

Attempted 2026-08-04 and FAILED: `RUSTFLAGS="-C target-feature=-crt-static"`,
on the theory that the static CRT was the cause. It is not — the failure is
byte-identical without it. The next idea worth trying is splitting the testable
logic into a plain rlib that no cdylib links against. Don't re-try the RUSTFLAGS
route.

**⚠️ Live limitation, not a solved problem.** Ubuntu and macOS run the full
suite, and `release.yml`'s gate runs it before any artifact is built, so
releases are gated. But a bug that only manifests on Windows still has no
automated catcher. Windows-specific changes need manual verification on the lab
tower until the crate split happens.

**What to do meanwhile:** prefer platform-independent logic that CAN be tested
everywhere — `resolve_gpu_for_start()` is the model. It encodes a rule that
only matters on Windows (Smart App Control) but is a pure function over an enum,
so it is fully tested on Linux.

### 4. Tests that encode a wrong assumption

The most dangerous, because they look like coverage.

`publish_github.test.ts` asserted `findInstaller` returns **null** for Linux,
"where the AppImage is both". That was wrong, and the test *defended* it: no
Linux entry in `downloads`, so the published `.deb` was referenced by nothing,
so a `.deb` install could neither self-update (Tauri's Linux updater only
applies AppImages) nor be pointed at its own upgrade. A rig sat on 0.4.8 through
three releases.

**What to do:** write the assertion as a *user-facing consequence*, not a
restatement of the implementation. "Returns null for Linux" restates the code.
"Offers the .deb as the Linux installer" is a claim about the product that can
be judged true or false on its own.

If a test needs changing to let a fix land, stop: either the test was wrong
(say so in the message, as above) or the fix is.

---

## Invariants that must never regress

These encode promises made in public. Each has a test; none may be deleted
without changing the corresponding promise.

| Invariant | Where | Promise |
|---|---|---|
| Remote actions are start/stop only | `remote/tests.rs::only_start_and_stop_are_remote_actions` | MONETISATION §5.2 never-list |
| The uplink carries no payout address | `remote/tests.rs::the_uplink_never_serializes_a_payout_address` | pasiv.network/privacy |
| Fee is 4%, XMR-only, mining-state only | `fee/mod.rs` (5 tests) | MONETISATION §1 |
| The fee address is a compile-time constant | `fee/mod.rs::fee_address_is_valid_and_ships_the_engine_on` | MONETISATION §5.3 |
| The XMRig API token never reaches argv | `miners/xmrig.rs::the_api_token_never_reaches_the_command_line` | it can rewrite the payout address |
| A blocked GPU never stops CPU mining | `commands/mod.rs` matrix | 0.4.13 |
| Every locale has exactly the English keys | `src/i18n.test.ts` | 7 shipped locales |
| A pasivd push row never carries a payout | `supabase/functions/pasivd/logic.test.ts` | pasiv.network/privacy (server-side mirror of the desktop's uplink test) |
| pasivd launches xmrig with `--http-no-restricted` | `pasivd::xmrig_args_enable_the_unrestricted_api_the_fee_swap_needs` | without it the fee-swap 403s and a fresh node mines nothing (0.1.1) |
| A pasivd node reports a lane + only a real est $/day | `pasivd::snapshot_carries_the_lane_and_only_a_real_est_usd_day` | the companion card isn't a blank "Mining" (0.1.2) |

---

## Where the gate runs (and why it didn't)

The most expensive gap was not a missing test. It was that **the tests never
ran for anything we shipped**.

`ci.yml` triggered on `push: branches: [main]` and pull requests. But releases
are tagged from working branches — v0.4.7 through v0.4.13 were all cut from one
sitting 31 commits ahead of `main` — and `release.yml` ran no tests, no clippy
and no fmt at all. So every release for two weeks produced a signed, notarised,
auto-updating binary, delivered to real machines, with **no gate whatsoever**.
The suite was green locally the whole time. Nothing enforced that.

Two changes, and neither may be reverted without replacing it:

1. `release.yml` has a `gate` job — frontend tests, `cargo test` for both crates,
   `clippy -D warnings`, `fmt --check` — and `build` and `pasivd` both `needs:
   gate`. A tag cannot produce an artifact unless the suite passes.
2. `ci.yml`, `ci-windows.yml` and `ci-macos.yml` all run on **every branch**,
   so problems surface when the commit lands rather than when the tag is cut.
   The first pass fixed only `ci.yml` — the platform workflows kept their
   main-only trigger and so still never ran for the branch that ships, which is
   the same hole one level down. Worth remembering: a partial fix to a gating
   problem looks identical to a real one until you check which jobs actually
   ran.

A tag is the moment "it works on my machine" becomes everyone's problem. The
gate belongs there regardless of which branch it is on.

---

## The trust boundary: the edge function

`supabase/functions/pasivd` is the only path a headless node touches the account
— it authenticates devices, serves payouts, and claims commands. It ran with
zero tests until 2026-08-21, protected only by a comment. The security-critical
*decisions* are now pure functions in `logic.ts` (importing `index.ts` would
start `Deno.serve` and need env), unit-tested in `logic.test.ts`: the payout
privacy invariant, owner-scoping (a device can't write another account's row),
input clamping, and the unambiguous pairing code. Run with `deno test
supabase/functions/pasivd/`. The live Supabase-call paths (device auth,
command claim/expire) are still only covered indirectly — a mock/local Supabase
would close that.

## Coverage floors (a ratchet, not a target)

Every package has a line-coverage floor set a few points below its current
number, so coverage can't silently erode. **Raise the floor as coverage climbs;
never lower it to green a red build — add the missing test.**

| Package | Current | Floor | Enforced by |
|---|---|---|---|
| Frontend (TS) | 78.6% lines | 76 / 74 / 66 / 70 (L/S/F/B) | `vitest.config.ts` thresholds, via `npm run test:coverage` |
| Desktop Rust | 56.3% lines | 54 | `cargo llvm-cov --fail-under-lines` (CI runs this instead of `cargo test`) |
| pasivd Rust | 25% lines | 22 | same (low by nature — most of pasivd is the async I/O loop; the pure logic is tested) |
| Mobile Flutter | 47.5% lines | 45 | `flutter test --coverage` + an lcov floor check in mobile CI |

The extract-and-test pattern is how the floor gets earned: pull the risky
decision into a pure function (`resolve_gpu_for_start`, `pasivd::xmrig_args`,
`pasivd::build_snapshot`, the edge fn's `buildRigRow`), test it exhaustively, and
keep the impure caller a thin wrapper. Coverage of I/O glue is not the goal;
coverage of *decisions* is.

## Running it

```bash
npm test                                             # frontend (vitest)
npm run test:coverage                                # + coverage floors
cargo test --manifest-path src-tauri/Cargo.toml      # desktop core
cargo test --manifest-path pasivd/Cargo.toml         # headless node
deno test supabase/functions/pasivd/                 # edge function
```

CI (`ci.yml`) runs all of these on Linux with `clippy -D warnings`, `fmt
--check`, and the coverage floors, plus `cargo test` on macOS and Windows. The
`release.yml` gate runs the same suite before any artifact is built.
