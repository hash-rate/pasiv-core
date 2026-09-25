# Pasiv — Fees, and the never-list

*Public, binding document. If this file and the running app ever disagree, the
app is wrong — file a bug. This is the canonical copy; the pasiv.network
whitepaper mirrors it.*

Pasiv makes money **only when you do.** The mechanics below are documented in
full — and, since this repository exists, implemented in the open — so anyone
can see exactly what they pay and confirm it against the app's own fee ledger.

> **Revision 2026-09-25 (Pasiv 0.5.0).** Changed, in the open, as never-list
> item 3 requires:
> 1. **Payouts in USDT by default, via unMineable.** New installs mine on
>    unMineable's pool, which converts what you mine to USDT and pays **your
>    own address** once a day from 1.5 USDT. Pasiv still never holds funds.
>    Installs already paying a coin address directly keep that route until
>    their owner switches — nobody is moved.
> 2. **The 4% now applies to every coin on the unMineable route** (it was
>    Monero-only). On the direct route it stays Monero-only.
> 3. **unMineable's own 0.75% fee is listed below,** and so is Pasiv's
>    referral: unMineable pays Pasiv 0.25% of your mining **out of its own
>    fee** — it does not come out of your earnings.
> 4. **Signed-in users are identified in analytics by an opaque id**
>    (never-list item 4 revised). Anonymous users stay anonymous.
>
> <!-- PENDING before publishing: Pearl-on-unMineable confirmed by the 48 h rack
> earnings test (2026-09-25). If Pearl stays direct, item 2 reads "Monero". -->

---

## 1. What Pasiv charges

**4%.** Taken as time-sliced mining, only while you are actively earning.

| | Amount | Taken by | When |
|---|---|---|---|
| **Pasiv fee** | **4%** | Pasiv, as time-sliced mining | **Only while you are actively earning** |

That is the whole of it. No ads, no subscription on the free tier, no cut of
your payouts beyond the 4%. Paused or idle time is charged at zero, and the
app's own ledger shows every slice. **On the unMineable route (the default from
0.5.0) the fee applies to every coin you mine**
([`fee_fraction_unmineable`](../crates/pasiv-core/src/fee.rs)); on the direct
route, which installs from before 0.5.0 keep until they switch, it applies to
**Monero only** ([`fee_fraction`](../crates/pasiv-core/src/fee.rs)).

### Other fees, which are not ours

Two other parties take a cut of mining. Neither reaches Pasiv — but you will
see them in your payouts, so they are listed here rather than left for you to
discover.

| | Amount | Taken by | |
|---|---|---|---|
| unMineable pool fee | 0.75% | unMineable (the default route) | 1%, lowered to 0.75% by Pasiv's referral code in your login |
| Pasiv's referral | 0.25% | Paid **to Pasiv by unMineable, out of unMineable's own fee** | Not taken from your earnings; disclosed because Pasiv benefits |
| Pool fee (direct route) | ~1% | Your chosen pool (MoneroOcean, LuckyPool, …) | Per that pool's own published terms |
| SRBMiner dev fee | 2% | The SRBMiner-Multi developers | Built into the GPU mining engine (Pearl) |
| XMRig dev donation | 1% | The XMRig developers | Built into the mining engine Pasiv drives |

**On XMRig's 1%.** Pasiv runs the *stock, official* XMRig binary — fetched from
XMRig's own releases and SHA-256-verified — which carries a 1% developer
donation floor, launched with `--donate-level 1`, the minimum a stock binary
accepts. Compiling the donation out is GPL-permitted and was proven to work;
we deliberately don't. Removing it would return roughly fifty cents per node
per year while costing the checksum-verified official supply chain, the
binary's years of antivirus reputation, and a fork to maintain forever. The
honest framing stands: **the engine is XMRig, its developers keep 1%, and we
do not touch it.**

---

## 2. How the 4% works

The fee is **time-sliced hashrate**, identical in mechanism to XMRig's dev fee
— the model this audience already trusts — but auditable, and now open source.

- For **4% of active mining time**, the miner submits shares to **Pasiv's fee
  address** instead of the user's. The other 96% goes to the user's payout
  address, untouched. On the unMineable route the fee address is Pasiv's
  treasury, `0x10B65cCcDB6a865F0e9f1F77B30cd7718a6BfeeF`, paid in USDT on BNB
  Smart Chain by the same pool ([`FEE_ADDRESS_TREASURY`](../crates/pasiv-core/src/fee.rs)).
- **The slice shape depends on how the miner can switch.** The CPU miner
  (XMRig) re-logs-in live, so its slice is 20 s in every 500 s. The GPU miner
  (SRBMiner) can only switch by restarting, measured at ~19 s from restart to
  hashing, so its slice is 10 minutes in every 4 h 10 min: the same 4%, with
  warm-up costing you under 0.3% instead of most of a short slice
  ([`slice_shape`](../crates/pasiv-core/src/fee.rs)).
- The slice accrues **only in the `Mining` state.** `Idle`, `Paused`,
  `Starting`, and `Error` contribute **zero** fee time. This is the fairness
  guarantee, enforced in code: the fee counter is driven by the same state
  machine that drives the UI
  ([`state`](../crates/pasiv-core/src/state.rs)), so it is structurally
  impossible to charge a paused user.
- Slices are **short and frequent** — the first 20 seconds of every 500
  seconds of Mining time ([`in_fee_slice`](../crates/pasiv-core/src/fee.rs)):
  a pure function of time spent mining, which is what makes the percentage
  structural rather than promised.

**Where to read it:**
- The schedule, the compile-time **fee address**, and the ledger format:
  [`crates/pasiv-core/src/fee.rs`](../crates/pasiv-core/src/fee.rs). Changing
  the address or the percentage requires a new signed release plus a changelog
  entry (never-list §3).
- A complete, runnable enforcement loop: [`pasivd`](../pasivd/src/main.rs) —
  the headless daemon swaps the pool login onto the fee address for a slice
  and back via XMRig's config hot-reload, verifies *where hashes actually
  went* before writing the ledger line, and **stops mining entirely rather
  than continue on the fee address** if it repeatedly fails to swap back. The
  proprietary desktop app implements the same policy against the same
  constants (it stops after 3 consecutive failed swap-backs; restarts always
  respawn on the user's address).
- Every slice writes a `FeeEvent { started_at, ended_at, coin, address,
  est_hashes }` to a local append-only `fee-ledger.jsonl` — one JSON object
  per line, readable with any text editor. **`pasivd` writes the same ledger**
  (`/var/lib/pasivd`, or `~/.local/share/pasivd` when unprivileged;
  `PASIVD_FEE_LEDGER` overrides): a fee is only auditable if it is auditable
  everywhere it is charged. The line is written on the falling edge of a
  slice, once XMRig has confirmed which address it is actually mining to, so
  the ledger records where hashes went rather than where we intended them to
  go.

---

## 3. The fee ledger (the trust surface)

In the desktop app (Pro → Fees), always visible:

- Running total contributed, all-time.
- The exact **fee address** (Monero, the only coin with a Pasiv fee), with a
  "check it on the pool ↗" link — so anyone can confirm on the mining pool
  that the numbers match. (Monero is a private chain: a block explorer shows
  nothing for any address, so verification is pool-side, not on-chain.)
- A timestamped list of recent fee slices.
- The plain-language line: **"Pasiv takes 4%, only while you're mining. Your
  pool takes ~1% and the bundled XMRig engine keeps 1% — neither of those is
  ours. Nothing else leaves this machine."**

Fairness is not a claim here; it's a receipt.

---

## 4. The never-list (binding)

Pasiv will **not**, in any build:

1. Show ads, or bundle "partner" coins the user didn't choose.
2. Silently switch the user's pool, coin, or payout address — and never mine
   at all without an explicit start from the user. (Auto mode is opt-in and
   shows its reasoning.)
3. Change the Pasiv fee percentage or fee address **without a versioned
   changelog entry and a new signed release.** (Both are compile-time
   constants in this repository.)
4. Collect telemetry that is undisclosed, that you cannot switch off, or that
   identifies anyone who hasn't signed in. Product analytics are anonymous for
   everyone without an account. A user who **signs in** is identified by an
   opaque id — the first 32 hex of sha256 of their account id — so their own
   desktop, phone and site events join up; never by a wallet or payout
   address, which are never sent to analytics from anywhere. Every event is
   listed in `docs/ANALYTICS.md`, and one toggle in Settings stops collection
   entirely (an opted-out install makes no request at all). *(Revised
   2026-09-25; until then Pasiv never called `identify()` at all.)*
5. Charge fee time in any state other than `Mining`.
6. Hide how the fee works — the exact mechanism is this file plus the code
   beside it, and every slice is shown in the app's fee ledger.
7. Hold, route, or touch user funds. Payouts are non-custodial: the pool pays
   the address the user entered — on the default route unMineable converts to
   USDT and pays it daily once past 1.5 USDT, holding the balance until then
   exactly as any pool does — and Pasiv earns its keep through its fee, the
   disclosed referral, signing, updates and support — never by holding your
   coins, your account, or your data.
8. Accept a remote instruction beyond **start, stop, and update** — and an
   update installs only a release Pasiv signed. Nothing sent from the phone
   or the cloud can change which coin a machine mines, its pool, or its
   payout address: there is no way to express those commands, they expire
   server-side after two minutes, and the limit is enforced by the database
   schema, not just by app code.
9. Trade your hardware for hashrate. No overclocking, no undervolting, no
   raising thermal or power limits, ever — not as a default, not as an option.
   Pasiv runs on machines people own and use.

Break any of these and it's a bug, not a business model.

---

## 5. Getting the most out of a machine

Pasiv's fee is a share of what you earn, so a node running below its potential
costs us exactly as much as it costs you. Tuning is therefore the app's job, not
yours — bounded by never-list item 9 above.

**Applied for you, automatically.** RandomX is bottlenecked on memory latency,
and two settings recover most of what a default setup loses: **huge pages** and
a **CPU MSR preset** (the same registers [XMRig's own
`randomx_boost.sh`](https://github.com/xmrig/xmrig/blob/master/scripts/randomx_boost.sh)
writes). Together they are typically worth **5–15%**. Both need root once, and
neither has a downside you would notice, so the `pasivd` installer applies them
itself — a privileged step that runs before the daemon drops into its sandbox.

**Asked once, when it costs you something.** Windows large pages need an account
privilege and a sign-out to take effect, so the desktop app asks rather than
assumes, and tells you the gain first.

**Refused, when the price is wrong.** The Windows MSR mod is worth another
~10–15% and needs the WinRing0 kernel driver, which sits on Microsoft's
vulnerable-driver blocklist. Pasiv does not ship it. The never-list outranks the
hashrate.

**Named, when it cannot be had.** With Secure Boot on, the kernel refuses the raw
MSR writes this boost needs, and no amount of setup changes that. `pasivd doctor`
says so plainly — including that turning Secure Boot off is the only thing that
would unlock it — rather than sending you after a fix that cannot work.

Run `pasivd doctor` to see the state of each of these on any node; every
un-applied gain is reported with its size, because a machine mining happily at
85% of its ceiling otherwise looks identical to a healthy one.
