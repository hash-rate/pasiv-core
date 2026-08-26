# Verus (VRSC) — native arm64 VerusHash spike

> Status (2026-07-24): **END-TO-END PROVEN — accepted share on live LuckPool.**
> VerusHash V2.2 compiles arm64-native in pure Rust, produces **bit-exact-correct**
> hashes (reference vectors pass), runs on **hardware AES** (~12× the portable path),
> and — as of this session — it is a **wired-in, in-process miner** (`VerusAdapter`,
> `Coin::Vrsc`) that landed a real accepted share on `na.luckpool.net:3956` both as a
> standalone proof (~445 kH/s single-thread) AND through the real `Miner` trait path
> (~2.5 MH/s / 4 workers, accepted in ~33s — see the ignored `verus_adapter_mines_a_live_share`
> test). The full PBaaS stratum protocol (handshake → notify → 1487B header → merged
> canonicalisation → pool-nonce-in-solution-tail → submit) is solved and documented in §10.
> This takes Pasiv past "a wrapper for a single mining binary": a second, genuinely
> different algorithm the Mac is actually good at — **in-process Rust, no vendored miner
> binary**. Remaining before it's user-facing: a real VRSC address from Simon for in-app credit.

## Why Verus is the flagship

On RandomX (Monero) a Mac is mediocre. VerusHash 2.2 is built around AES, which Apple
Silicon accelerates in hardware — so this is the one coin where "your Mac is *great*
at this" is literally true. There is **no open, licensed, fast arm64 VerusHash** to
vendor (UnminerMac is closed/Elastic-licensed; the ccminer-arm forks are unlicensed,
1-star, stale). So the path is to make the reference implementation build fast on ARM.

## The base

- Crate: **`verushash-rs` 0.1.1** — wraps the reference VerusHash C/C++ (haraka +
  verus_clhash), exposes `verus_hash_v2_2(&[u8]) -> [u8; 32]` (also v1/v2/v2.1),
  builds the C via the `cc` crate. Ships test vectors in `src/testunit.rs`.
- License note: VerusHash C is Apache-2.0 (per `verus_clhash.h`); sse2neon is MIT.
  **Confirm the `verushash-rs` crate's own license before vendoring.**

The crate does **not** build on macOS-arm64 as published — but the VerusHash C is
*already written for ARM* (`verus_clhash.cpp` has `#if __aarch64__ #include
"SSE2NEON.h"` and an `IsCPUVerusOptimized()` that checks ARM `HWCAP_AES`/`HWCAP_PMULL`).
It fails only on three portability nits, all fixed below.

## The patch (reproducible)

Copy the crate locally, use it as a `path` dependency, then:

1. **Add `sse2neon.h`** (MIT, from DLTcollab/sse2neon `master`) to `native/crypto/`.
2. **`haraka.h` + `haraka_portable.h`** — the include `#include "immintrin.h"` (x86)
   becomes, guarded:
   ```c
   #if defined(__arm__) || defined(__aarch64__)
   #include "sse2neon.h"
   #else
   #include "immintrin.h"
   #endif
   ```
3. **`verus_clhash.h`** — (a) the top x86 include block (`<cpuid.h>` / `<x86intrin.h>`
   / `<intrin.h>`) gets an ARM branch that includes `"sse2neon.h"` instead; (b)
   `IsCPUVerusOptimized()` uses Linux-only `getauxval(AT_HWCAP)` on ARM — add an
   `#if defined(__APPLE__)` branch that just sets `__cpuverusoptimized = true`
   (every Apple Silicon core has AES + PMULL).
4. **`verus_clhash.cpp`** — (a) fix the include path `"crypto/SSE2NEON.h"` →
   `"sse2neon.h"`; (b) the `#if defined(__aarch64__) //intrinsics not defined in
   SSE2NEON.h` supplement block (~lines 59–147) is now stale — **modern sse2neon
   defines all of these correctly**, so disable it (`#if 0`). Crucially, Verus's
   hand-rolled `_mm_clmulepi64_si128` *ignored* the `imm` selector; sse2neon honours
   it with standard x86 semantics, which the x86-origin code actually expects.

All four edits are small and localized; `sse2neon.h` is dropped in verbatim (not
patched). The whole change is < 30 lines across the four files plus the vendored header.

## Proof

**Correctness** — `cargo test --release` on the patched crate runs its shipped vectors:
```
test_verus_hash_v1      ... ok
test_verus_hash_v2      ... ok
test_verus_hash_v2_1    ... ok
test_verus_hash_v2_2    ... ok        # "hello world" -> 6cae82cbef9b80afe08e2ceab0073f5db66b3f2f9c3ebca9e8f4e36f7cef4baf
test_verus_hash_v2_2_1000x ... ok
result: 10 passed; 0 failed
```
Bit-exact-correct on arm64.

**Speed (M4 Max, single thread, one-shot `verus_hash_v2_2`)**
| Path | Rate | Note |
|---|---|---|
| Hardware AES (optimized) | **~0.19 MH/s / core** | sse2neon → `vaeseq_u8` + `vmull_p64` |
| Portable (AES off) | ~0.02 MH/s / core | ~**12× slower** — confirms the AES path is real |

The one-shot figure is conservative: it regenerates VerusHash's 2 MB verusclhash key on
*every* call, whereas a miner generates the key once per block and amortizes it across
the nonce range. The true competitive mining rate needs the amortized/streaming
benchmark (next step) before comparing to hellminer.

## Next steps (to a shipping Verus engine)

1. **Amortized mining benchmark** — expose the streaming `CVerusHashV2` API
   (reset/write/finalize with a persistent key) to Rust and measure the real
   per-block hashrate; compare to hellminer to size the earnings story.
2. **Stratum client** — LuckPool CPU: `stratum+tcp://na.luckpool.net:3956#xnsub`,
   user `<VRSC_address>.<worker>`, pass `x`. (`#xnsub` = extranonce subscribe.)
3. **A native Earner/Miner** — unlike XMRig this is **in-process** (a Rust worker
   pool calling `verus_hash_v2_2` + the stratum client), so it doesn't fit the
   sidecar `Miner` trait as-is; it's a new adapter shape. This is the one real
   architectural addition.
4. **VRSC address validator** — base58check, 'R' prefix, ~34 chars (sha256d checksum).
5. **Registry row** — `Coin::Vrsc` once the engine exists (it's not XMRig-family).

## 10. LuckPool stratum protocol (captured live, 2026-07-24)

The hash is proven + vendored (`src-tauri/vendor/verushash-rs/`, MIT, in-repo tests
pass). The remaining miner is Zcash-lineage stratum (Verus is a Zcash fork). Captured
straight off `na.luckpool.net:3956` with a test worker:

```
>> mining.subscribe          << result [null, "<extranonce1>"]   (extranonce1 = 4B)
>> mining.extranonce.subscribe << true
>> mining.authorize [Raddr.worker, x] << true
<< mining.set_target ["<256-bit target hex>"]
<< mining.notify [ job_id, version(4B LE "04000100"=65540), prevhash(32B),
                   merkleroot(32B), finalsaplinghash(32B), time(4B), bits(4B),
                   clean_jobs(bool), solution_template(125B PBaaS prefix) ]
```

**Header hashed by VerusHash** = version‖prevhash‖merkleroot‖finalsaplinghash‖time‖
bits‖nonce(32B)‖**nSolution**, where nSolution = compactsize(`fd4005` = 1344) ‖
solution(1344B). Total hashed length = 140 + 3 + 1344 = **1487 bytes**.

### ✅ ACCEPTED SHARE — protocol solved end-to-end (2026-07-24)

A standalone Rust miner (scratchpad `verus-miner`, links the vendored `verushash-rs`)
landed a **real accepted share** on live `na.luckpool.net:3956`:
```
[REAL-SHARE] hash(be)=0000000978e9  target=000000400000
[SUBMIT RESPONSE] result=true error=null
```
~445 kH/s single-thread on the M4. The exact byte layout that the pool accepts
(reverse-engineered from `Oink70/ccminer-verus` `scanhash_verus` + `equi_stratum_submit`,
which are the ground truth for what LuckPool validates):

- **Solution version 8, `solution[5] > 0` ⇒ PBaaS "merged" path** (LuckPool's live mode).
  The mining nonce does **NOT** live in the header nNonce — it lives in the **last 15
  bytes of the 1344B solution** (`solution[1329..1344]`), the `nonceSpace`:
  | offset in solution | 4B | 4B | 1B | 2B | 4B |
  |---|---|---|---|---|---|
  | `1329` | **pool nonce (extranonce1)** | round | thrd id | pad | **counting nonce** |
  The `[20,"pool nonce missing"]` reject means extranonce1 isn't at `solution[1329..1333]`.
- **Merged canonicalisation before hashing** (pool does the same): zero the header's
  prevhash+merkleroot+finalsaplinghash (`[4..100]`), nBits+nNonce (`[104..140]`), and
  `solution[8..72]`. Iterate the counting nonce; VerusHash V2.2 the full 1487B; compare
  reversed (LE→BE) hash to target.
- **Submit** = `mining.submit [worker, job_id, time, noncestr, solhex]` where
  `noncestr` = header nNonce`[4..32]` (28B, extranonce prefix stripped — pool re-prepends)
  and `solhex` = `fd4005` ‖ the 1344B solution (**with** the un-canonicalised
  `solution[8..72]` template bytes restored, and the `nonceSpace` embedded). 1347B total.
- Reject-code ladder observed while converging: `[20]` invalid solution size → `[20]`
  pool nonce missing → **`[23]` low difficulty share** (format valid, just above target)
  → **`result:true`** (real below-target share). `[23]` on a deliberate above-target test
  submit is the "format is correct" signal.

### ✅ SHIPPED — in-process Earner adapter (2026-07-24)

The proven layout is now a real, wired-in miner: **`src-tauri/src/miners/verus.rs`**
(`VerusAdapter`). It implements the same `Miner` trait as XMRig, so the supervisor /
governor / fleet drive it unchanged — the only difference is it's **in-process** (no
vendored sidecar): a CPU worker pool calling `verus_hash_v2_2` + a small blocking
stratum client (subscribe / extranonce.subscribe / authorize / set_target / notify /
submit + reconnect-with-backoff + vardiff via `set_target`). `start()` returns
`MinerHandle { child: None }`; the supervisor already tolerates that.

- **Nonce partitioning** across workers via the `nonceSpace` thread-id byte
  (`solution[1337]`) + a per-worker stride on the counting nonce.
- **Governor pause/resume** implemented (workers idle, connection stays warm).
- `set_payout` intentionally unsupported — the VRSC payout *is* the stratum login,
  and the fee engine is Monero-only, so it never asks Verus to divert.
- `Coin::Vrsc` roster row (`na.luckpool.net:3956`), VRSC validator
  (`is_valid_vrsc_address` — 'R' prefix, 34 base58 chars), webview mirror + picker chip.
- **Excluded from Auto / Max-Profit ranking** (`CoinSpec::auto_rankable`): VerusHash is a
  different PoW, so `price×reward÷difficulty` (which assumes the machine's hashrate
  cancels) can't compare it to the RandomX family. VRSC is a manual pick.

**Proven end-to-end through the trait path** — an ignored integration test
(`verus_adapter_mines_a_live_share`) ran the *adapter* (not the reference binary) against
live LuckPool: ~2.5 MH/s across 4 workers, **accepted share in ~33s**, clean stop. Run it:
`cargo test -p pasiv verus_adapter_mines_a_live_share --release -- --ignored --nocapture`.

**Remaining before it's a user-facing feature:**
1. A **real VRSC address from Simon** for in-app credit (the proof used the public test
   worker `RPPPm6dVbpx3L3yDRK1ktZ1VnDbBTtNMoy`).
2. Optional polish: pipe stratum connection/log lines to the UI log (in-process miners
   have no stdout stream today), and amortise the verusclhash key per job for more H/s.
