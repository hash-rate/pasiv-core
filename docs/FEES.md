# Pasiv — Fees, and the never-list

*Public, binding document. If this file and the running app ever disagree, the
app is wrong — file a bug. This is the canonical copy; the pasiv.network
whitepaper mirrors it.*

Pasiv makes money **only when you do.** The mechanics below are documented in
full — and, since this repository exists, implemented in the open — so anyone
can see exactly what they pay and confirm it against the app's own fee ledger.

> **Revision 2026-10-27: Share & earn pays 2 of 4 on every code.** Changed,
> in the open, as never-list item 3 requires:
> 1. **Every code now earns 2 of Pasiv's 4 points** — 2 fee slices in every
>    4, i.e. 2% of the referred rig's gross mining, for 12 months from the
>    day the code was confirmed. Pasiv keeps 2%. Until today a regular code
>    earned 1 point (1%) and a creator code 2; **there is no higher creator
>    rate any more**. The miner still pays exactly 4%, never more. This is
>    live on the server today (it reports every code as paying 2 of 4), so
>    installs on older versions pay their friend 2% too, from their next
>    daily re-check. The core still accepts a pinned share of 1 or 2
>    ([`Affiliate::is_valid`](../crates/pasiv-core/src/fee.rs)), so an old pin
>    stays valid until that re-check moves it.
> 2. **From Pasiv 0.7.3, a code needs no sign-in.** Every install can get an
>    auto-issued, readable code (like `amber-otter-42`) tied to the USDT or BTC
>    payout address it already uses on unMineable; signing in still lets you
>    pick a custom code. A native-coin payout address (PRL or XMR, "Advanced")
>    cannot receive a code's slices — such a user needs a USDT or BTC address
>    for their code.
> 3. **Headless `pasivd` nodes do not pay friend codes.** Share & earn is
>    desktop-only; every slice a node mines goes to the treasury, and
>    `pasivd claim` now says so.
> 4. **Payout threshold, stated plainly.** unMineable pays USDT from 1.5 USDT.
>    An affiliate who does not also mine to that address waits until the
>    friends' slices alone cross it.

> **Revision 2026-10-26 (Pasiv 0.7.2, pasivd 0.1.13): the CPU miner is
> Pasiv's own, everywhere.** Changed, in the open, as never-list item 3 requires:
> 1. **XMRig's 1% is gone on every platform.** Windows Defender removes stock
>    XMRig on sight — even a copy Pasiv signs — so Windows used to ask for a
>    one-time permission to exclude Pasiv's miner folder. From 0.7.2 the CPU
>    lane on Windows, macOS, Linux and pasivd runs **pasiv-rx**, Pasiv's own
>    RandomX miner (GPL-3.0, built on the BSD-3 RandomX library; the Windows
>    build is signed by Pasiv; its source ships with every release, see
>    [SIDECAR-PROVENANCE.md](SIDECAR-PROVENANCE.md)). It has no developer
>    donation, so the only third-party fee left is the pool's. Pasiv's own 4%
>    is unchanged: same slice, same treasury, same ledger.
> 2. **Never-list item 2** loses its last sentence, "(Auto mode is opt-in and
>    shows its reasoning.)": Auto mode was removed in 0.7.0, so it described a
>    setting that no longer exists. Nothing else in the item changes.

> **Revision 2026-10-24 (Pasiv 0.7.0, pasivd 0.1.12): one payout route, and
> "Advanced: native coins".** Changed, in the open, as never-list item 3
> requires:
> 1. **The legacy direct route is retired.** Until now an install from before
>    0.5.0 could still mine straight to a coin's own pool (MoneroOcean for
>    Monero, LuckyPool for Pearl) with a Monero fee address and a fee rule
>    that charged **Monero only** — Pearl on that route paid Pasiv nothing.
>    Every rig now mines on unMineable, the route every install since 0.5.0
>    uses. A rig that was paying a Monero or Pearl address directly keeps
>    **the same address**: unMineable now pays it in that coin (below), so
>    nobody's payout moves and nobody has to be at the keyboard.
> 2. **You can be paid in the coin you mine.** Beside USDT and BTC, Wallets →
>    Get paid gains **Advanced: native coins**: a Pearl (PRL) address for the
>    GPU lane and/or a Monero (XMR) address for the CPU lane, each paid daily
>    by unMineable in that coin from its own threshold (1 PRL and 0.03 XMR on
>    2026-10-24 — read live from the pool, never hard-coded; 0.03 XMR is
>    weeks of a desktop CPU, and the app says so before you choose). A lane
>    without a native address keeps paying your USDT/BTC address.
> 3. **The 4% is now the same on every coin, for everyone.** With one route
>    there is one fee rule ([`fee_fraction`](../crates/pasiv-core/src/fee.rs)):
>    4% of mining time, to the BTC treasury below, on Monero and Pearl alike.
>    For a legacy Pearl-direct rig this is a new charge — disclosed here and
>    in the release notes. The old Monero fee address
>    (`47jfXhesYLvN5M8Qzy5j7PCwd99kZKdwNKwxqonrdEDVVzopGUfNApn1NK98sPE7wgGzsEtvMYM1cZChVpDHasabFH1MZ1f`)
>    is retired from the code; ledger lines that name it still read.
> 4. **Zephyr, Salvium and Verus retire** on the 1 November 2026 date set in
>    0.5.0 — they only ever had the direct route. A rig still set to one
>    switches to Monero, paid to its own Monero address if it has one, else it
>    idles with a "set a payout" notice. Never-list items 2 and 7 hold: the
>    pool pays the address you entered; nothing is switched silently — this
>    page, the changelog and the in-app notice say exactly what moved.

> **Revision 2026-10-15 (Pasiv 0.6.10, pasivd 0.1.10): BTC as an optional
> payout asset.** No fee changes. On the unMineable route you may now be paid
> in **BTC** to your own Bitcoin address (`1…`, `3…` or `bc1…`, checksum-verified)
> instead of USDT. USDT stays the default and the fast one; BTC is for holding.
> unMineable pays BTC only once the balance reaches its BTC threshold
> (0.00075 BTC on 2026-10-15 — read live from the pool, never hard-coded),
> which is far higher in dollars than USDT's 1.5, so the app shows that
> threshold and an estimate of the wait at your machine's own rate **before**
> you choose, and asks you to confirm. Pasiv's 4% and the BTC treasury it is
> paid to are unchanged; unMineable's fee and Pasiv's referral are as below.
> Still non-custodial: the pool pays the address you entered (item 7).

> **Revision 2026-09-28 (Pasiv 0.5.9): Share & earn, made honest.** Changed,
> in the open, as never-list item 3 requires:
> 1. **A friend's share is counted over the life of the install, not per
>    session.** The 1-in-4 (or 2-in-4) rule is applied to a running count of
>    fee slices your machine has confirmed — it no longer starts from zero at
>    every launch. Before this a PC that is switched off daily almost never
>    reached the friend's slice on the GPU miner (it was the last of every
>    four, and four GPU windows take 16 h). Slices still sit at the end of
>    their window, so no session ever pays more than 4%.
> 2. **Codes pay only on the unMineable (USDT) payout route.** A friend's
>    address is BTC or USDT on unMineable; a direct Monero payout and a
>    headless `pasivd` node mine straight to Monero pools and cannot pay it, so
>    there every slice goes to Pasiv's treasury. The app now says so and
>    doesn't take a code on the direct route.
> 3. **What a code pays is decided by the server, not the screen.** The desktop
>    asks Pasiv's backend what a code is (address, asset, regular or creator)
>    when you confirm it; the settings screen can no longer set those. Once a
>    day it re-checks the code: a removed code is unpinned, a tier change is
>    applied. Your own code is refused on the server as well as locally.
> 4. **Uses are counted per machine**, so "machines using your code" cannot be
>    inflated by repeat calls. And note the trade-off we accept: a code is a
>    public label that resolves to a payout address — treat it like a public
>    tip address, because anyone can look one up.
>
> **Revision 2026-09-26 (Pasiv 0.5.5): Share & earn.** Changed, in the open,
> as never-list item 3 requires:
> 1. **The fee is still exactly 4% of mining time, and you pay nothing
>    extra.** If you set up Pasiv with a friend's code, part of **Pasiv's own
>    4%** goes to that friend: 1 fee slice in every 4 (1 of the 4 points) for
>    a regular code, 2 in 4 for a creator code, for 12 months from the day you
>    confirmed it. The rest goes to Pasiv's treasury as before.
> 2. **The friend is paid by the pool, never by Pasiv.** Their slices mine
>    straight to their own BTC or USDT address on unMineable, exactly like
>    Pasiv's treasury slices do. Pasiv holds nothing (never-list item 7).
> 3. **You choose it, you see it, you can remove it.** A code is applied only
>    when you enter it and confirm the address it pays; that address is then
>    pinned on your machine (nothing remote can change it — never-list item
>    8). Every such slice is in your fee ledger with the address it paid.
>    Removing the code sends every slice back to the treasury at once.
>    ([`fee_destination`](../crates/pasiv-core/src/fee.rs))
>
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

---

## 1. What Pasiv charges

**4%.** Taken as time-sliced mining, only while you are actively earning.

| | Amount | Taken by | When |
|---|---|---|---|
| **Pasiv fee** | **4%** | Pasiv, as time-sliced mining | **Only while you are actively earning** |

That is the whole of it. No ads, no subscription on the free tier, no cut of
your payouts beyond the 4%. Paused or idle time is charged at zero, and the
app's own ledger shows every slice. **The fee applies to every coin you
mine** — Monero on the CPU, Pearl on the GPU — whatever you choose to be paid
in: USDT, BTC, or the coin itself
([`fee_fraction`](../crates/pasiv-core/src/fee.rs)). There is one payout
route, unMineable; the legacy direct route and its Monero-only rule retired
in 0.7.0.

### Other fees, which are not ours

Other parties take a cut of mining too. None of it reaches Pasiv (the one
exception, the referral, comes out of unMineable's fee, not yours) — but you
will see these in your payouts, so they are listed here rather than left for
you to discover.

| | Amount | Taken by | |
|---|---|---|---|
| unMineable pool fee | 0.75% | unMineable (the pool, on every payout asset) | 1%, lowered to 0.75% by Pasiv's referral code in your login |
| Pasiv's referral | 0.25% | Paid **to Pasiv by unMineable, out of unMineable's own fee** | Not taken from your earnings; disclosed because Pasiv benefits |
| Share & earn (optional) | 2 of Pasiv's 4 points (2%) | Paid **to the friend whose code you entered, out of Pasiv's 4%** | Not an extra fee — it's part of the 4%; every code alike since 2026-10-27 (was 1 point, 2 for creator codes); only if you entered a code; ends after 12 months; desktop only |
| ~~SRBMiner dev fee~~ | ~~2%~~ | — | Gone since 0.6.11: Pasiv's own build of the Pearl engine runs with its fee off, so nothing on the GPU lane goes to a third-party developer |
| ~~XMRig dev donation~~ | ~~1%~~ | — | Gone from 0.7.2 (pasivd 0.1.13): the CPU miner is Pasiv's own (pasiv-rx) on every platform and has no developer donation, so nothing on the CPU lane goes to a third-party developer |

**On XMRig's 1% (history).** Until 0.7.2 Pasiv ran the *stock, official*
XMRig binary, which carries a 1% developer donation floor, and deliberately left
it in: removing it was worth about fifty cents per node per year and would have
meant maintaining our own miner. Defender made us build that miner anyway
(pasiv-rx), so from 0.7.2 the CPU lane runs it everywhere and the 1% is gone.

---

## 2. How the 4% works

The fee is **time-sliced hashrate**, identical in mechanism to XMRig's dev fee
— the model this audience already trusts — but auditable, and now open source.

- For **4% of active mining time**, the miner submits shares to **Pasiv's fee
  address** instead of the user's. The other 96% goes to the user's payout
  address, untouched. The fee address is Pasiv's treasury,
  `bc1qv8nlkvhjelgp5d5lk79qjs6sdeq08w8vjxgz90`, paid in BTC by the same pool,
  on the same algorithm the user mines — publicly checkable at
  <https://unmineable.com/address/bc1qv8nlkvhjelgp5d5lk79qjs6sdeq08w8vjxgz90?coin=BTC> ([`FEE_ADDRESS_TREASURY`](../crates/pasiv-core/src/fee.rs)).
- **The slice shape depends on how the miner can switch.** The CPU miner
  (XMRig) re-logs-in live, so its slice is 20 s in every 500 s. The GPU miner
  (the Pearl engine, as SRBMiner before it) can only switch by restarting, measured at ~19 s from restart to
  hashing, so its slice is 10 minutes in every 4 h 10 min: the same 4%, with
  warm-up costing you under 0.3% instead of most of a short slice
  ([`slice_shape`](../crates/pasiv-core/src/fee.rs)).
- The slice accrues **only in the `Mining` state.** `Idle`, `Paused`,
  `Starting`, and `Error` contribute **zero** fee time. This is the fairness
  guarantee, enforced in code: the fee counter is driven by the same state
  machine that drives the UI
  ([`state`](../crates/pasiv-core/src/state.rs)), so it is structurally
  impossible to charge a paused user.
- Each slice sits at the **end of its window** — the last 20 seconds of
  every 500 seconds of Mining time for the CPU miner, the last 10 minutes of
  every 4 h 10 min for the GPU miner. Mining time starts from zero each time
  the app starts, so a slice at the start would be charged in full by every
  short session; at the end, **no session ever pays more than 4%** (changed
  2026-09-27, Pasiv 0.5.7 — before that a GPU rig restarted by an update paid
  its 10-minute slice up front)
  ([`in_fee_slice`](../crates/pasiv-core/src/fee.rs),
  [`in_fee_slice_for`](../crates/pasiv-core/src/fee.rs)): a pure function of
  time spent mining, which is what makes the percentage structural rather
  than promised.
- **A friend's slices** (Share & earn) are the last 2 of every group of
  4 fee slices (2%; since 2026-10-27 for every code — before that 1 for a
  regular code) — counted over the life of the install, persisted on disk, so
  a machine that mines in short sessions still hands its friend exactly their
  share over time (2026-09-28). `pasivd` has no Share & earn and pays no
  friend code; all its slices go to the treasury.

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

Every fee slice, on every surface, is written to a local append-only
`fee-ledger.jsonl` (one JSON object per line: start, end, coin, fee address,
estimated hashes) — in the desktop app's data
folder, and in `pasivd`'s state directory on a headless node. Beside it:

- **In the desktop app:** the Wallets tab states the fees in one line
  (Pasiv 4% · unMineable pool 0.75%), and Settings → Diagnostics reports the
  ledger — the number of slices and the total fee time. `pasivd doctor`
  reports the same ledger on a headless node.
- **The exact fee address**, a compile-time constant in
  [`fee.rs`](../crates/pasiv-core/src/fee.rs) — the BTC treasury — so anyone
  can confirm on the mining pool that the numbers match: the treasury's
  public page on unMineable (linked in §2). Ledger lines written before 0.7.0
  by the retired direct route name the old Monero fee address instead; they
  still count and display, and MoneroOcean's page for that address is the
  record of what they paid. (Monero is a private chain: a block explorer
  shows nothing for any address, so verification is pool-side, not on-chain.)

Fairness is not a claim here; it's a receipt.

---

## 4. The never-list (binding)

Pasiv will **not**, in any build:

1. Show ads, or bundle "partner" coins the user didn't choose.
2. Silently switch the user's pool, coin, or payout address — and never mine
   on a machine with no payout address of the user's set up. Once one is set
   up, Pasiv starts with the computer and mines by default; an explicit Stop
   (app, tray or phone) is final across every restart until the user starts
   again. *Revised 2026-10-07 for 0.6.7: previously "never mine at all
   without an explicit start". Revised 2026-10-26 for 0.7.2: the sentence
   about Auto mode went with Auto mode (removed in 0.7.0).*
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
