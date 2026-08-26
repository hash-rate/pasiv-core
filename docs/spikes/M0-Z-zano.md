# M0-Z · Zano Engine Spike — Findings

**Timebox:** M0-style go/no-go, no production code. **Date:** 2026-08-04.
**Status:** research complete; one empirical step deferred (Q1 binary acceptance
test — see below).

> Method note: economics are **computed from first principles** in
> `M0-Z-zano-bench.csv`, not copied from a calculator site, so every figure can
> be re-derived from the four inputs stated there. The PRL column uses hashrate
> **measured on our own rig** (3× RTX 4060, `simon-MS-7D90`), not a vendor
> claim. ProgPowZ per-card hashrates are **estimates** and are flagged as such —
> they are the weakest numbers here and the first thing to re-measure.

---

## TL;DR verdict

| Q | Question | Verdict |
|---|---|---|
| Q1 | Miner + sidecar fit | **GO — and better than hoped.** SRBMiner-Multi covers `progpowz` on **AMD + NVIDIA + Intel** at **0.85%** dev fee, and the binary we already vendor (3.4.6) has the progpow family compiled in. Integration is a registry entry, not an adapter. |
| Q2 | PoW/PoS split + honest economics | **NO-GO.** Zano has announced **Zenith**, which *ends PoW mining entirely*. No date, no block height. Separately, miners get only ~50% of emission, and a mid-tier card is **loss-making above ~$0.05/kWh**. |
| Q3 | Pools and payout | **GO.** HeroMiners (already a Pasiv pool brand), plus K1Pool, LuckyPool (multi-region), WoolyPooly. TLS, configurable minimums, standard address paste. |
| Q4 | Auto mode boundary | **Manual-pick only**, exactly as Verus is handled. Do not couple the engine to an Auto extension. |
| Q5 | Governor + UX | **Deferred (not blocking).** No idle GPU to test under ProgPowZ load — all three cards are mining. Registry entry drafted below. |

**Overall: NO-GO on Zano.** Not because the engine is hard — it is the cheapest
engine we will ever add — but because **the coin's own roadmap removes the thing
we would be building for**, on no published timeline.

**But the spike found something worth more than Zano.** The AMD question was
asked as first-class, and the answer is yes: SRBMiner's progpow family is our
AMD path, through a binary we already ship. That capability is real and worth
having — Zano is simply the wrong vehicle for it. See "What to do instead".

---

## Q1 · Miner + sidecar fit — **GO**

`progpow_zano` in SRBMiner-Multi's algorithm table:

| Property | Value | Consequence for us |
|---|---|---|
| Dev fee | **0.85%** | Lower than pearlhash's 2%. Third-party fee, disclosed like XMRig's 1%. |
| Devices | **AMD + NVIDIA + Intel** (`[ - A N I ]`, no CPU) | First AMD-capable engine. |
| Algo flag | `--algorithm progpowz` | Same launch shape as pearlhash. |
| OS | Windows + Linux (as we already vendor) | No new build targets. |

**Why this collapses integration cost:** we already vendor SRBMiner-Multi 3.4.6
for Windows and Linux, fetched and checksum-verified at build
(`tool/sidecars.json`). Zano needs **no new binary, no new adapter, no new
redistribution question** — the redistribution permission we rely on for
pearlhash already covers it. The work is a `CoinSpec` row plus a validator.

Contrast M0-Q, where Q1 was the disqualifier: for Qubic there was no client that
was simultaneously maintained, redistributable and pool-capable. Here the
already-shipped binary covers it.

**Evidence, and the one gap:**

- Official algorithm table lists `progpow_zano` at 0.85% with `A N I` devices.
- SRBMiner changelog references `progpowz` performance work by **3.0.2** —
  comfortably before the **3.4.6** we vendor. Latest upstream is 3.4.8.
- Our shipped binary self-reports `SRBMiner-MULTI 3.4.6` and exposes
  `--gpu-progpow-safe` ("use this if you get a lot of gpu validation errors on
  **progpow algorithms**") — so the progpow family is compiled into the exact
  binary we distribute.
- ⚠️ **Deferred:** a clean `--algorithm progpowz` *acceptance* run. Attempted on
  `simon-MS-7D90`; the miner exits without output when the GPUs are already
  committed to pearlhash, and I would not stop production mining for a spike.
  A `strings` probe was inconclusive and **its own control failed** (`pearlhash`
  also returned zero matches on a binary that is mining pearlhash), so it proves
  nothing either way — recorded here so nobody repeats it.
  **Harness:** on any box with a free GPU,
  `srbminer --algorithm progpowz --pool <host:port> --wallet <addr> --disable-cpu`
  and confirm it reaches DAG build rather than "unknown algorithm".

**Memory floor:** ProgPow is a DAG/datatable algorithm (SRBMiner exposes
`--gpu-table-slow-build` for "DAG/Datatable creation"), so it carries an
Ethash-style VRAM floor that grows over time. Our existing eligibility gate
(CUDA Turing+, ≥3 GB) is **too loose** for ProgPowZ and would show a dead
toggle on small cards. Re-derive the floor from the live DAG size before ship.

## Q2 · PoW/PoS split and honest economics — **NO-GO**

### The disqualifier: Zano is leaving PoW

Zano has announced **Zenith**, a move to *pure* proof-of-stake. In Zano's own
words, **"PoW mining ends"** and **"the PoW share of block rewards ends with
it."** The post gives **no activation date and no block height**, describes the
work as research-stage ("there's a long way between a research result and a live
network"), and **does not describe any transition period for miners**.

That is disqualifying for us regardless of how cheap the engine is:

- We would onboard users onto a coin whose mining has an announced end date and
  no published schedule. When Zenith lands, every Zano rig we created stops
  earning — through no failure of ours, but entirely predictably.
- Our roster bar includes honest economics. Shipping Zano **without** saying
  "mining on this coin is scheduled to end" would fail that test; shipping it
  **with** that disclosure raises the obvious question of why we shipped it.

### The split, as the brief suspected

| Fact | Value |
|---|---|
| Block reward | **1 ZANO**, fixed |
| Block time | 60 s → 1,440 blocks/day |
| Paid to **PoW** | **~720 ZANO/day** (Zano's own figure, ≈262,800/yr) |
| Implied PoW share | **~50% of blocks** |

So the headline "1 ZANO per block" is **double** what a miner should model. A
calculator that used 1,440 × 1 ZANO would overstate mining revenue by 2×. Our
figures below use 720.

### Emission history (for the bar page's Honesty test)

Zano carries a **3.69M ZANO premine** for development, marketing and
partnerships; roughly **440k remained as of July 2026 (~2.9% of supply)**. The
team's prior project, **Boolberry**, ran a 1% dev-tax with no premine, which
Zano's own docs say proved unsustainable — that experience is given as the
reason for the premine. This is disclosed by the project, not hidden, and the
remaining fund is small. It is not disqualifying; it is a thing to state plainly
rather than let a user discover.

### Revenue (see `M0-Z-zano-bench.csv`)

Inputs: ZANO $8.9329, network 551.96 GH/s, PoW emission 720 ZANO/day →
**$0.011652 per MH/s/day**. PRL $0.30, network 33.31 EH/s, **2425.84** PRL per
block at 197.14 s → **$9.576×10⁻¹⁵ per H/s/day**.

> **CORRECTED 2026-08-15.** The PRL block reward here was originally recorded as
> 245.61 — a ~10× understatement (coinUnits slip: 242583793536 atomic ÷ 1e8 =
> 2425.84 PRL). Verified against the live LuckyPool `/api/stats` and against the
> 2026-07-29 measurement of ~2471 PRL/block (`profit/mod.rs:462`); a reward of
> 2471 → 245.6 → 2425.8 is not a real emission change. Only this input was wrong;
> the arithmetic was always sound. **The PRL comparison below is reversed by the
> correction** — see the revised paragraph. The Zano verdict itself is unaffected:
> it rests on Zenith ending PoW and on the PoW/PoS emission split, not on PRL.

| Card | ZANO gross/day | net @$0.05 | net @$0.10 | net @$0.15 |
|---|---|---|---|---|
| RTX 4060 | $0.198 | **+$0.060** | −$0.078 | −$0.216 |
| RTX 3070 | $0.280 | **+$0.124** | −$0.032 | −$0.188 |
| RTX 4070 SUPER | $0.350 | **+$0.110** | −$0.130 | −$0.370 |
| AMD RX 6700 XT | $0.280 | **+$0.070** | −$0.140 | −$0.350 |
| AMD RX 7600 | $0.186 | −$0.012 | −$0.210 | −$0.408 |

**PRL beats ZANO on the one card we can compare honestly** — the opposite of what
this section said before the reward correction above. Our measured RTX 4060 does
55.7 TH/s pearlhash = **$0.534/day**; the same card on ProgPowZ is an *estimated*
**$0.198/day**. PRL is roughly **2.7×** ZANO, and note the asymmetry in evidence:
the PRL figure is measured on our own rig, the ZANO one is an estimate. So the
crossover the brief asked about has **not** happened — Pearl remains the better
GPU coin, which is why the roster work targets it rather than replacing it.

**Zano is still a NO-GO**, and for reasons the correction does not touch:

1. Zenith removes PoW earnings entirely on an unknown date — an unhedgeable
   integration risk regardless of today's rate.
2. Miners get only ~50% of emission, so the PoW/PoS split permanently caps what a
   Zano node can earn relative to a full-emission coin.

The earlier third reason — "every card is loss-making at $0.10/kWh and above" — was
an artefact of the 10× error and is **withdrawn for PRL**: at $0.10/kWh a 4060 on
pearlhash nets **+$0.258/day**. It still holds for ZANO on these estimates.

## Q3 · Pools and payout — **GO**

| Pool | Scheme | Notes |
|---|---|---|
| **HeroMiners** | PPS+ / PROPX | TLS ports, configurable minimum payout. **Already a Pasiv pool brand** (ZEPH, SAL) — same trust story, same UX. |
| K1Pool | PPLNS | Low fee, automatic payouts. |
| LuckyPool | — | Multi-region: `sg.zano.luckypool.io` (Asia), `ca.zano.luckypool.io` (US). Same operator as our Pearl pool. |
| WoolyPooly | — | Documented `--algorithm progpowz --pool pool.woolypooly.com:3146 --wallet <addr>`. |

Regional coverage (EU/US/Asia) is satisfied. All are paste-your-address,
direct-payout pools — no accounts, which is the custody property we require and
the one Qubic failed.

**Address validator — recommendation: standard addresses only at launch.** Zano
has standard addresses, integrated addresses, and an **@alias** system. Aliases
resolve through a chain lookup, which is a network dependency and a failure mode
inside a validator that must work offline. Accept the standard form, reject the
rest with a specific message. (Same reasoning that made us reject XMR integrated
addresses and VerusID `name@` forms.)

**Fee slicer — confirmed compatible, one paragraph as asked.** ProgPowZ is
continuous share-based mining with per-block payouts and no epoch settlement, so
the time-slice mechanism works exactly as it does for pearlhash: swap the pool
`--user` for the fee address for the slice, swap back, and the ledger line is
written on the falling edge. Nothing about Zano needs the fee engine changed.
This is the property Qubic failed (weekly epoch settlement, un-hot-swappable
payout ID), and Zano passes it cleanly.

## Q4 · Auto mode boundary — **manual-pick only**

Auto ranks the RandomX family, where a machine's hashrate is constant across
coins, so USD/day is directly comparable. ProgPowZ vs pearlhash on the *same
GPU* produces different hashrates in different units — 17 MH/s vs 55.7 TH/s —
so the existing ranking cannot compare them without per-device normalisation.

**Recommendation: ship any progpow engine as manual-pick, exactly as Verus is
handled on CPU.** This is already an established, honest pattern in the product,
and `get_pearl_rate` / `get_verus_rate` exist precisely because those coins sit
outside the ranking.

**Effort estimate for the benchmark path, if ever wanted:** a per-device
calibration run (~30–60 s per algorithm per GPU, cached in config, invalidated
on driver/hardware change) plus a normalisation layer in `profit::rank`, plus UI
for "benchmarking…" — call it 3–5 days, and it introduces a first-run delay on
exactly the screen where a new user is deciding whether this app works. **Do not
couple it to an engine ship.**

## Q5 · Governor + UX — **deferred, not blocking**

ProgPow is power- and thermal-heavy. Our rig runs pearlhash at **78 °C** on RTX
4060s under the existing guardrails, which is healthy, but ProgPowZ is a
different load and I have **no idle GPU to test on** — all three cards are in
production. Deferred rather than guessed.

What to check when a card is free: sustained temperature under ProgPowZ vs
pearlhash, whether `--gpu-off-temperature` trips appropriately, and whether
SRBMiner's power-limit options are worth defaulting.

**Draft registry entry** (`src-tauri/src/coins/mod.rs`), ready if the verdict
ever flips:

```rust
CoinSpec {
    coin: Coin::Zano,
    ticker: "zano",
    name: "Zano",
    miner: MinerId::SrbMiner,
    algo: "progpowz",
    pool_host: "zano.herominers.com",
    pool_port: 1230,              // confirm + TLS variant before ship
    validator: is_valid_zano_address,   // standard addresses only; reject @alias
    // ProgPowZ is DAG-based: the existing Turing+/≥3GB gate is TOO LOOSE.
    // Derive the floor from live DAG size or the toggle will be dead on small cards.
}
```

---

## What to do instead — the finding worth keeping

The strategic question was "does this open AMD?" **Yes.** SRBMiner's progpow
family runs on AMD, NVIDIA and Intel, through the binary we already vendor, at
0.85%. That is the cheapest possible route to Pasiv's first AMD-capable engine —
it needs no new sidecar, no new redistribution permission, and no adapter work.

Zano is simply the wrong coin to carry it, because its own roadmap ends mining.

**Recommendation:** keep the AMD ambition, drop the vehicle. SRBMiner's changelog
references **`progpow_telestai`** alongside `progpowz`, so at least one other
progpow coin is covered by the same binary. Re-run this spike's Q2/Q3 against a
progpow coin with **no announced PoW sunset**; Q1 is already answered and would
not need repeating. That is a half-day of work, not three days, because the
expensive question is settled.

---

## Bar-page verdict paragraph (Zano row)

> **Zano — not shipped, and here's why.** Zano mines well: the algorithm runs on
> AMD as well as NVIDIA, and the miner we already bundle supports it at a lower
> fee than our GPU coin does. We are not adding it, because Zano has announced
> that it is ending mining altogether and moving to pure staking. There is no
> published date. Adding it would mean pointing people's hardware at a coin
> whose mining is scheduled to stop, with no way to tell them when — so we would
> rather not start. If that plan changes, the engine is a day's work and we will
> revisit. On the numbers, Pearl also simply earns more: about 2.7× Zano on the
> same card, and that Pearl figure is measured on our own rig where the Zano one
> is an estimate. GPU mining pays best when your power is cheap or already paid
> for.
