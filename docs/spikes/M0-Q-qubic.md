# M0-Q · Qubic Engine Spike — Findings

**Timebox:** M0-style go/no-go, no production code. **Status:** research complete;
one empirical step (Q3 live epoch benchmark) deferred — see below. **Date:** 2026-07-31.

> Method note: every client claim below is checked against the **repositories and
> GitHub's license API**, not marketing docs. Licenses were confirmed via
> `gh api repos/<owner>/<repo>/license` (authoritative), not by reading READMEs.
> No third-party mining binaries were downloaded or executed in this spike, so
> per-binary SHA-256 hashes and on-metal output samples are the deferred piece
> (Q3 harness handed over below).

---

## TL;DR verdict

| Q | Question | Verdict |
|---|---|---|
| Q1 | Sidecar viability (disqualifier) | **NO-GO** — no maintained + redistributable + pool-capable client. Unblocks only with explicit redistribution permission from a client author. |
| Q2 | Payout + fee-model fit | **NO-GO** — weekly epoch settlement, pools that require accounts, and a launch-arg payout ID that the 1% time-slicer can't hot-swap. No clean custody-free fee path. |
| Q3 | Earnings reality | **INCONCLUSIVE (weak)** — thin market ($58M cap, ~$1–2M/day volume, ~$4×10⁻⁷/QUBIC) makes it a volatile diversification play at best. Needs a live one-epoch run (harness provided). |
| Q4 | Reputational check | **EVIDENCE ONLY (Simon's call)** — Qubic ran a real 51% takeover of **Monero** in **Aug 2025** (6-block reorg). Our XMR-core, Monero-adjacent audience *is* the attacked party. |
| Q5 | Grant leverage | **CONSTRAINED** — the Grants program mandates all code open-source and **no reliance on closed-source**, which collides with both our open-core paid shell and the closed miner binaries. |

**Overall: the engine is rejected on current evidence.** Q1 and Q2 each fail on
hard, independent grounds (redistribution; fee-model/custody + weekly settlement).
Q4 adds a strategic reason to *not* ship it even if Q1/Q2 were solved. The single
thing that could reopen Q1 — a redistribution grant from a client author — is a
business/legal ask for Simon, not an engineering unblock, and it leaves Q2 and Q4
untouched.

---

## Q1 · Sidecar viability — **NO-GO (conditional on redistribution permission)**

Maintained headless Windows clients **do** exist — the disqualifier is not
"headless", it's **redistributable**. Our open-core stance requires a vendored
sidecar we can legally ship. Authoritative license check:

| Client | Repo | Devices | Headless | License (via GitHub API) | Redistributable? | Last push | Pool? |
|---|---|---|---|---|---|---|---|
| **rqiner** | `Qubic-Solutions/rqiner-builds` | CPU (Rust) | ✅ `-t -i -l -b` | **none** (404) | ❌ default all-rights-reserved | 2025-01-08 | ✅ (Qubic-Solutions PoolHub, `-i <id>`) |
| **rqiner-hybrid** | `Qubic-Solutions/rqiner-hybrid-builds` | CPU + CUDA | ✅ | **none** (404) | ❌ | 2024-08-01 (stale) | ✅ |
| **qli-Client** | `qubic-li/client` | CPU + NVIDIA CUDA | ✅ CLI / `appsettings.json` | **none** (404) | ❌ | 2026-06-19 | ✅ (qubic.li) |
| **Apool miner** | `apool-io/apoolminer` | CPU (AVX2) + NVIDIA RTX20–50 | ✅ `--algo qubic …` | **none** (404) | ❌ | 2026-06-12 | ✅ (Apool) |
| **Qiner** (official reference) | `qubic/Qiner` | CPU (AVX2/512) | ✅ CLI | **Anti-Military License** (source-available) | ✅ (AML permits redistribution) | 2026-07-24 | ❌ **solo/direct-to-node only** |

The tension, plainly:

- The **performant, pool-integrated, GPU-capable** clients (rqiner, qli, Apool) are
  **binary-only with no license file** → by default copyright, all rights reserved,
  **not legally redistributable**. We cannot bundle them the way we bundle XMRig
  (GPLv3) or SRBMiner (closed-source **but** with the author's redistribution
  permission).
- The **one clearly-redistributable** client (Qiner, AML) is a **dormant CPU-only
  reference implementation with no pool support** — and Qubic mining for a single
  consumer machine is *pool-mandatory* (solo solution rates round to zero). So it
  is non-viable as a shipping sidecar.

There is **no client that is simultaneously {maintained, redistributable,
pool-capable}**. Per the spike's kill criterion, the disqualifier fires.

**macOS / Apple Silicon:** no client. rqiner advertises `aarch64` but ships no
confirmed darwin build; qli/Apool/Qiner are Windows/Linux only. → **A Qubic engine
would be Windows/Linux-only.** (Feeds site/app copy P1-4: Qubic is not offered to
Mac users — same shape as the Verus-is-Mac-only split, inverted.)

**Output parseability (Miner-trait risk, independent of licensing):** none of the
clients document a parseable stdout stream of `hashrate` + `accepted-solution`
events. qli logs to files; Apool exposes a local `/gpu` HTTP status endpoint;
rqiner's stdout is undocumented. Our `Miner` trait needs both signals — most
likely they'd come from the **pool's HTTP API keyed by identity** (as Pasiv
already does for PRL on LuckyPool), not from the sidecar's stdout. Confirmable only
by running the binaries (deferred).

**Unblock path:** explicit written redistribution permission from **Qubic-Solutions
(rqiner)** or **qubic.li (qli-Client)** — the exact precedent as SRBMiner (closed
but permitted, shipped with a disclosure line). This is a Simon-level outreach, not
an engineering task. Absent it, Q1 = NO-GO.

## Q2 · Payout + fee-model fit — **NO-GO**

**Settlement is weekly and epoch-locked.** Each epoch ends **Wednesday 12:00 UTC**;
the prior epoch's revenue is calculated and paid **within 24 h** after. So a user
mines for up to a **full week before the first payout** — a hard UX regression from
XMR/PRL's continuous pool balances, and it makes the "is this thing working?"
first-run question unanswerable for days.

**Raw-identity, no-registration payout is not universal.** Apool (largest PPLNS
pool, **10% fee**) requires **sign-up + an allocated sub-account** — you cannot just
paste a 60-char identity, which **breaks our non-custodial pattern**. rqiner's
integrated pool (Qubic-Solutions PoolHub) does take a raw identity via `-i <id>`,
so a registration-free path exists on *some* pools but not the biggest one. Any
engine would have to pin to a raw-identity pool and accept its (smaller) hashrate
share and terms.

**The 1% time-sliced fee model does not map to Qubic.** Two independent breakers:

1. **No runtime payout hot-swap.** Our fee slicer works because XMRig exposes an
   authenticated API to change the payout address *in place* every slice, keeping
   the RandomX dataset warm. rqiner takes the payout identity as a **launch
   argument** (`-i`), with no runtime control channel. Diverting 1% of time to
   Pasiv's identity would require **restarting the miner every slice** — losing warm
   state and, under Qubic's phase-based workload, likely straddling training/idle
   transitions. Impractical.
2. **Weekly, share-attributed settlement + variance.** Even if you could swap IDs,
   the pool credits shares to whatever identity is set at submission time and
   settles **weekly**. A 1% time-slice on a single consumer machine produces a small
   share count that, netted against pool minimums and weekly rounding, is fragile at
   small fleet sizes. On a PPLNS pool the *expected* value is non-zero (shares are
   frequent, unlike full solutions), but it is only realizable through the
   restart-per-slice mechanism above, which kills it in practice.

**Alternative fee mechanisms — all edge toward custody (flagged for Simon):** the
only ways to charge a fee without per-slice restarts are (a) accrue fee on a
Pasiv-side ledger and settle from a **Pasiv-operated pool account** — Pasiv holds
funds = custody; or (b) run a Pasiv sub-account on Apool and reconcile — same. **No
custody-free fee path was found** for Qubic's launch-arg + weekly-epoch model.
This is the item the spike said to escalate: *if no custody-free alternative
exists, flag for Simon* — it does not.

**Wallet UX:** the Qubic **MetaMask Snap** (`qubic/qubic-mm-snap`, "Qubic Connect",
plus `ardata-tech/qubic-wallet`) is **audited by Sayfer** (2-week pentest, 2 vulns
found and remediated), published under the official `qubic` org. Identities it
derives are standard 60-char Qubic IDs and receive pool payouts normally. This part
is fine — but it doesn't rescue the fee-model or settlement problems.

## Q3 · Earnings reality — **INCONCLUSIVE (weak); live epoch run deferred**

I could not run a full epoch in this spike (it is a **~week of wall-clock** under
weekly settlement, and requires executing unlicensed third-party binaries on the
bench). Published reference points and the market context:

- **Throughput (published):** RTX 4090 ≈ **2,100 it/s**; Ryzen 7950X ≈ **850 it/s**.
  Qubic is broadly CPU-favoured ("CPU is king") though GPUs contribute.
- **Token/market reality (the decisive part):** QUBIC market cap **≈ $58.5M**
  (rank ~#330), price **≈ $4.15×10⁻⁷**, 24 h volume only **~$1–2M**, circulating
  supply **~140.9 trillion**. This is a **thin, highly volatile** market — realizing
  mined QUBIC into AUD at any scale invites slippage, and the spike's **−50% price
  scenario is entirely ordinary** for a coin this size. (Network-wide "$3M/week to
  miners" figures include the Monero-redirect revenue — see Q4 — not organic QUBIC
  demand.)

Even with a favourable it/s benchmark, the **liquidity + volatility** profile means
Qubic is a **diversification play, not an earnings win** over XMR (deep, liquid) on
the same CPU or PRL (~$1.3/day, verified) on the same GPU. To "beat or meaningfully
diversify the incumbent on the same silicon" it would have to clear those
incumbents *net of* a −50% haircut and thin-market slippage — unlikely on this
market cap.

**Handover:** `M0-Q-qubic-bench.csv` (schema + published reference rows) and the
harness in **Appendix A** run the real thing when/if Q1+Q2 unblock: one full epoch,
CPU (AVX2 + AVX512) + one NVIDIA card, logging solutions, QUBIC credited, uptime,
and **phase-transition behaviour** (does the client idle at 0% or spin the fan
during training phases?). Convert with the CSV's `aud_per_day` and
`aud_per_day_halfprice` columns.

## Q4 · Reputational check — **EVIDENCE ONLY (Simon decides)**

> Correction to the brief: the incident was **August 2025**, not 2024, and it was
> not merely "51%-adjacent" — it was an **executed** majority takeover.

Sourced timeline:

- **May 2025:** Qubic < 2% of Monero hashrate. Its "Useful PoW" model points miners
  at Monero, converts the XMR rewards to USDT, and **buys and burns QUBIC**.
- **Late July 2025:** share climbs past **~25%**.
- **Aug 2025:** Sergey Ivancheglo (Come-from-Beyond, ex-IOTA) publicly targets 51%
  for Aug 2–31. Qubic **executes a 6-block reorganization** of the Monero chain and,
  in one 122-block window, mines **63 blocks (~51.6%)**; peak share reported at
  **52.72%**.
- **Response:** a coordinated Monero-community **boycott** drives Qubic's share back
  to ~10–15%; miners publicly framed it as a **publicity stunt**; exchanges raised
  confirmation counts.

Sources: [CoinDesk](https://www.coindesk.com/business/2025/08/12/qubic-claims-majority-control-of-monero-hashrate-raising-51-attack-fears),
[The Block](https://www.theblock.co/post/364496/qubics-monero-hashrate-controversy),
[Halborn](https://www.halborn.com/blog/post/explained-the-monero-51-percent-attack-august-2025),
[CryptoSlate](https://cryptoslate.com/monero-community-pushes-back-as-qubics-51-hash-rate-bid-falters/),
[DL News](https://www.dlnews.com/articles/defi/monero-miners-rebuff-qubic-51-percent-hashrate-attack/),
[Qubic's own blog](https://qubic.org/blog-detail/historic-takeover-complete-qubic-miners-now-secure-monero-network).

**Framing for the decision (no recommendation):** Pasiv's core engine is XMR and
its audience skews Monero-adjacent. The party Qubic attacked in Aug 2025 is
*exactly* that community, and the attack was real (chain reorg), not theoretical.
Bundling a Qubic engine is a legible signal to that audience. Simon calls this.

## Q5 · Grant leverage — **CONSTRAINED**

- **Qubic Incubation Program:** funded if **≥ 55%** of a 5-day community Discord
  poll approves; milestone-based disbursement from a 200B-QUBIC Ecosystem Fund.
- **The blocking clause:** the **Grants program requires all produced code be
  open-source and to *not rely on closed-source software***, and Qubic's own code is
  under the **Anti-Military License**. This collides on two fronts:
  1. **Our open-core split** — a *paid shell* over an open core is at odds with
     "all produced code open-source."
  2. **The only viable clients are closed** — an engine that "relies on" rqiner /
     qli / Apool binaries directly violates "no reliance on closed-source", and the
     AML's military-use restriction is itself not OSI-open-source, which complicates
     what license our engine code could even carry.

So the grant is not free money for the engine as-scoped: qualifying would force the
Qubic engine (and arguably its miner dependency) fully open under AML-compatible
terms — i.e. into the **open core, not the paid shell** — which removes the
monetization the engine build was meant to support. A pitch outline exists in
**Appendix B**, but it can't be squared with the open-core model without giving the
engine away.

---

## Deliverable 4 · Kill reason (one paragraph, so we never re-litigate)

> **Qubic engine — rejected (M0-Q, 2026-07-31).** No maintained Qubic client is
> legally redistributable: the performant, pool-capable, GPU-capable clients
> (rqiner, qli-Client, Apool) ship as binaries with **no license file** (all rights
> reserved; confirmed via GitHub's license API), and the only redistributable client
> (Qiner, Anti-Military License) is a dormant CPU-only *solo* reference with no pool
> support — unusable for consumer mining. Independently, the payout model is a poor
> fit: **weekly epoch settlement** (Wed 12:00 UTC) means up to a week to first
> payout, the largest pool (Apool) **requires an account** rather than a pasted
> identity, and the payout ID is a **launch argument with no runtime hot-swap**, so
> our 1% time-sliced fee can't be applied without restarting the miner every slice —
> and the only alternatives require Pasiv to **custody funds**. On top of that, the
> token market is thin and volatile ($58M cap, ~$1–2M/day volume) so earnings are a
> diversification gamble, not a win over XMR/PRL; and Qubic executed a real **51%
> takeover of Monero in Aug 2025**, i.e. against the exact community Pasiv courts.
> **Reopen only if** a client author grants explicit redistribution permission
> (SRBMiner precedent) *and* Simon accepts the weekly-settlement UX and the Monero
> reputational read; the fee model still needs a custody-free redesign before it
> could ship.

## Deliverable 3 · Conditional adapter sketch (interface only)

*Provided for completeness; do not build unless Q1's redistribution unblock lands.*
It slots into the existing `Miner` trait, but note the two shape-mismatches called
out inline — no runtime payout swap, and stats sourced from the pool API, not stdout.

```rust
// src-tauri/src/miners/qubic.rs  (SKETCH — not production)
pub struct QubicAdapter { app: AppHandle, client: reqwest::Client }

#[async_trait]
impl Miner for QubicAdapter {
    fn id(&self) -> MinerId { MinerId::Qubic }      // new variant
    fn coin(&self) -> Coin { Coin::Qubic }          // new variant
    fn device_class(&self) -> DeviceClass { DeviceClass::Cpu } // + a Gpu twin for CUDA

    async fn start(&self, cfg: &MinerConfig) -> Result<MinerHandle> {
        // rqiner: payout identity is a LAUNCH ARG (-i), not runtime-settable.
        // No config file / API to change it later → set_payout() below is Unsupported,
        // which means the fee slicer cannot run without a full restart. This is the
        // Q2 breaker, surfaced at the trait level.
        let args = ["-t", &threads, "-i", &cfg.payout_address, "-l", "pasiv"];
        // …spawn a REDISTRIBUTION-PERMITTED binary (does not exist today)…
    }

    async fn stats(&self) -> Result<MinerStats> {
        // NOT from stdout (undocumented). Poll the pool's HTTP API keyed by the
        // identity, like PRL/LuckyPool: hashrate + accepted shares. Weekly epoch
        // means "accepted" accumulates against a Wednesday boundary, not a session.
        self.pool_stats_by_identity(&cfg.payout_address).await
    }

    async fn set_payout(&self, _addr: &str) -> Result<()> {
        Err(MinerError::Unsupported("set_payout")) // ← fee slicer disabled for Qubic
    }
    // pause/resume: only if the chosen client answers SIGTERM cleanly (unverified).
}
```

**Risks list (if ever built):** (1) no redistributable performant binary — the whole
premise; (2) fee model needs a custody-free redesign or it doesn't earn Pasiv
anything; (3) weekly settlement breaks first-run trust and the earnings readout;
(4) stats depend on pool-API shape, per-pool; (5) phase-based workload may show 0%
during training phases — needs UI copy or it reads as "broken"; (6) Windows/Linux
only; (7) Monero-community blowback; (8) thin-market slippage on payout conversion.

---

## Appendix A · Benchmark harness (deferred live run)

One epoch minimum. Pseudocode for the operator once a redistributable client exists:

1. Pin client + version + `sha256` of each binary into `M0-Q-qubic-bench.csv`.
2. Run CPU (an AVX2 box and an AVX512 box) and one NVIDIA card, each to a
   **raw-identity pool** (Qubic-Solutions PoolHub), separate identities.
3. Every 5 min log to the CSV: `it/s`, GPU/CPU temp+watts, pool-reported accepted
   shares, and phase (training/idle) — watch for 0%-with-fans-spinning.
4. At the Wednesday epoch close + 24 h, record QUBIC credited per identity.
5. Fill `aud_per_day` (spot) and `aud_per_day_halfprice` (−50%); compare to the same
   box's XMR (CPU) and PRL (GPU) numbers.

## Appendix B · Grant pitch outline (outline only — see Q5 blocker)

- **Title:** "Pasiv — consumer desktop miners for Qubic."
- **Ask covers:** engine build, fee-model adaptation, support load for weekly
  settlement + phase workload.
- **Threshold:** 55% community vote (5-day Discord poll).
- **Open-source conflict (unresolved):** qualifying forces the engine — and possibly
  its miner dependency — fully open under AML-compatible terms, i.e. into the open
  core, removing the paid-shell monetization. Do not submit without resolving this
  with Simon first.

## Sources

Clients/licenses: [qubic/Qiner](https://github.com/qubic/Qiner),
[Qubic-Solutions/rqiner-builds](https://github.com/Qubic-Solutions/rqiner-builds),
[qubic-li/client](https://github.com/qubic-li/client),
[apool-io/apoolminer](https://github.com/apool-io/apoolminer),
[Qubic mining software docs](https://docs.qubic.org/learn/sw/). Pools/settlement:
[Qubic pool docs](https://docs.qubic.org/learn/pool/),
[Apool payout time](https://apool.gitbook.io/help/qubic-faq/payout-time),
[Apool PPLNS](https://apool.gitbook.io/help/qubic-faq/pplns-model). Wallet:
[Sayfer Snap audit](https://sayfer.io/audits/metamask-snap-audit-report-for-qubic/),
[qubic/qubic-mm-snap](https://github.com/qubic/qubic-mm-snap). Market:
[CoinMarketCap](https://coinmarketcap.com/currencies/qubic/). Grant:
[Incubation Program](https://qubic.org/blog-detail/introducing-the-qubic-incubation-program),
[Grants Program](https://qubic.org/blog-detail/introducing-the-qubic-grants-program).
Incident sources inline in Q4.
