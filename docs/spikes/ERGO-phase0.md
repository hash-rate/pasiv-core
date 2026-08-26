# Ergo (Autolykos2) · Phase 0 Verification Gate — Report

**Date:** 2026-08-17. **Status:** desk score complete — **gate PASSES; build
approved** (hardware first-share on the rack remains the ship gate, per the
PRL/RVN standard). Context: the earnings-per-node lever went software-only
(AMD lane closed without a purchase — see `AMD-phase0.md`), and Ergo is the
one candidate that can be hardware-verified on the lab's existing NVIDIA
cards.

> Method note: miner facts are read from SRBMiner-Multi's own `Readme` via the
> GitHub API (the `AMD-phase0.md` method); network economics are cross-checked
> across three independent sources (HeroMiners stats API, the official Ergo
> platform API, WhatToMine) and agree to <2%.

## Headline

| | |
|---|---|
| SRBMiner-Multi 3.5.3 mines it? | **Yes** — `autolykos2`, dev fee **1.00%** (half of pearlhash's 2.00%), devices `[ - A N I ]` (AMD, NVIDIA, Intel — no CPU). |
| ASIC-resistant (the Kaspa/Alephium test)? | **Yes** — Autolykos2 is memory-hard by design; consumer GPUs hold real network share. |
| New sidecar needed? | **No** — one roster row on the same pinned engine, exactly like RVN. |
| Pool? | HeroMiners Ergo: `de.ergo.herominers.com:1180` ("Low-End Hardware", diff 4G) — **TCP-verified open, 353 ms** (and the non-regional host, 334 ms). Same operator/API shape as our ZEPH/SAL/XMR stats sources. |
| Earns? | ≈ **$0.11/day on a 4060-class card** at today's numbers — ~3–4× RVN's take on the same card, ~1/6 of Pearl's. Ships as the clear #2 GPU coin and the Pearl hedge, not a Pearl replacement. |
| VRAM floor | Autolykos2 working set ≈ 2.4–2.5 GB and grows very slowly → provisional `min_vram_mb: 3072` (same floor as PRL); confirmed/adjusted at the rack measurement. |

## The numbers (2026-08-17, all primary)

- **Dev fee / devices** — SRBMiner Readme line 28: `[1.00%] [ - A N I ] autolykos2`. (For the record: `etchash` is `[0.65%] [ - A N I ]` — still excluded here on DAG size and ASIC contest, see the plan's deferred list.)
- **Price** — CoinGecko `ergo`: **$0.20267**.
- **Difficulty** — HeroMiners `/api/stats`: 59,452,111,716,352. Autolykos convention `hashrate = difficulty / block_time` → 5.945e13 / 120 ≈ **495 GH/s**.
- **Cross-check** — WhatToMine (coin id 340): nethash **503.8 GH/s**, block_reward **3.0105**, block_time 118 s — agrees with the derived figure to <2% (the same lock-in standard the RVN row used).
- **Block reward** — official Ergo platform API, blocks 1,853,048–49: `minerReward: 3000000000` nanoERG = **3 ERG flat** (EIP-27 re-emission era; document as a constant like RVN's 1250, revisit on schedule changes).
- **Per-card estimate** — public 4060-class Autolykos2 rate ≈ 120 MH/s (TO BE MEASURED on the rack; the constant we ship derives from the measurement, not this figure):
  `share = 1.2e8 / 5.04e11 = 2.38e-4` × ~730 blocks/day × 3 ERG ≈ 0.52 ERG/day × $0.203 ≈ **$0.106/day**.

## Gate scoring

The plan's gate: *ERG $/day on a 4060 is material (≥ RVN's) or the ≤6 GB-card
audience gains a second option worth ≥ cents/day.* Both arms pass: $0.106/day
vs RVN's ~$0.03/day on the same card, and the 3–6 GB cards RVN's 6144 MB floor
refuses get a second coin at PRL's own 3072 floor. The honest framing for the
UI and site copy: **Pearl remains the top GPU earner today; Ergo is the
established-network alternative** (Ergo has mined continuously since 2019 —
Pearl is a young token that has emergency-hardforked twice this quarter, and a
hedge with 6× less revenue but 100× more history is a rational user choice we
should offer, never push).

## What ships (the RVN template)

Roster row (`coins/mod.rs`): SrbMiner / `autolykos2` /
`de.ergo.herominers.com:1180` / `min_vram_mb: 3072` (provisional) / CoinGecko
`ergo` / `stats_url: ""` (manual pick — different PoW, never RandomX-ranked).
Address validator: mainnet P2PK — base58, leading `9`, 51 chars (Rust + TS
mirror). `profit::ergo_rate()` via `share_of_network_score` with
`hashps = difficulty / 120`, reward const `3.0`, HeroMiners stats source, plus
a deterministic test pinning the difficulty→hashrate convention against the
WhatToMine cross-check above. Matrix test gains `Some(Coin::Erg)` and its VRAM
floor. Wallet guide: `pasiv.network/mine/ergo` (site page, generated locales).
Dashboard: HeroMiners shape, pinned by the verified-shapes test (UA-sniffing
caveat applies — verify headless).

**Ship gate (non-negotiable, the AlphaPool lesson):** first accepted shares
from a rack 4060 running the exact shipped config, hashrate + VRAM recorded in
the roster comment, before any release.

## Addendum — rack hardware verification (2026-08-17, same day)

Run on a rack RTX 4060 (8 GB) with the exact shipped config
(`--algorithm autolykos2 --pool de.ergo.herominers.com:1180 --wallet 9erdMGwU…
--disable-cpu --gpu-id 0`), throwaway checksum-valid P2PK address minted
in-test (pool checks shape + checksum, not key ownership):

- **PASS: pool connected, wallet accepted, 3 accepted / 0 rejected shares**
  (avg find time 67 s at the port's 4G difficulty), latency 402 ms.
- **Measured hashrate: ~70–71 MH/s per RTX 4060** — the desk score's 120 MH/s
  public figure was optimistic. Corrected estimate:
  `(7.1e7 / 5.04e11) × 730 × 3 × $0.203 ≈ **$0.063/day per 4060**` — still
  ~2× RVN's take on the same card, ~1/10 of Pearl's. The gate's ≥-RVN arm
  still passes; the ranking (Pearl » Ergo » RVN) is unchanged.
- **VRAM: SRBMiner allocated 7.6 GB on the 8 GB card** — far above the
  ~2.5 GB Autolykos2 dataset (engine buffers appear sized to the card). With
  no smaller card measured, `min_vram_mb` ships **6144, not 3072**: a wrong
  3 GB floor puts a dead toggle on a 4 GB GPU, the exact failure the gate
  exists to prevent. This retracts the "fills the 3–6 GB gap" selling point
  until a 4 GB card is measured (same evidence rule as the AMD gate).
- Rack restored after: Pearl on the GPUs, XMR on the CPU, both verified live.
