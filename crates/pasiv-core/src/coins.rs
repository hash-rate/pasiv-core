// SPDX-License-Identifier: GPL-3.0-only
//! Coin roster — the single source of truth (Rust side) for which coins Pasiv
//! can mine and everything coin-specific about each: which miner handles it,
//! the unMineable algorithm it is mined under (the pool host follows from
//! that — `unmineable::Algo::host`), the native payout-address rule, the
//! price/stats sources for the earnings estimate. Every coin mines on
//! unMineable, Pasiv's one payout route; the user is paid in USDT or BTC
//! account-wide, or in the coin itself per lane (`PayoutAsset::native_for`).
//! Adding a coin = one row here (plus an engine only if it needs a *new*
//! miner). The supervisor, governor, and fee engine stay coin-agnostic.
//!
//! Display-only fields (human name, UI accent, input placeholder) live in the
//! webview mirror `src/coins.ts`, keyed by the same lowercase `ticker`, so
//! they never become dead code here. Keep the two in sync — same tolerated
//! pattern as the address validators.
//!
//! History. Until 0.7.0 (2026-10-24) this was the roster of the legacy
//! DIRECT route — each coin's own pool, port and dashboard — which installs
//! from before 0.5.0 kept. That route is gone: Zephyr, Salvium and Verus
//! (the coins only it ever carried) retired with it, on the 1 November 2026
//! date announced in 0.5.0, and their rows are removed; a config that still
//! names one is switched to Monero by the desktop's migration. Ravencoin and
//! Ergo went a week early, with 0.6.11 (2026-10-23): their only engine,
//! SRBMiner-Multi, retired when the signed Pearl engine shipped, so
//! `is_available()` refuses them everywhere; their rows stay as the record of
//! a verified lane until an engine carries them again.

use crate::types::{Coin, MinerId};
use crate::unmineable::Algo;

pub struct CoinSpec {
    pub coin: Coin,
    /// Lowercase ticker — also the `payouts` map key and the webview's coin id.
    pub ticker: &'static str,
    /// Which vendored binary/adapter mines this coin.
    pub miner: MinerId,
    /// Miner algorithm flag (XMRig `-a`, the Pearl engine's algo name), or
    /// None for the miner's default.
    pub algo: Option<&'static str>,
    /// The unMineable algorithm this coin is mined under — and so the pool
    /// host (`Algo::host()`, port `Algo::PORT`). `None` = unMineable does not
    /// carry it, so on the one route the coin cannot be mined at all
    /// (`is_available` is false).
    pub unmineable: Option<Algo>,
    /// Minimum GPU VRAM (MB) this coin's algorithm needs — 0 for CPU coins.
    /// Gated in `commands::resolve_gpu_for_start` / `coin_availability` against
    /// the LARGEST eligible GPU, so a too-small card is refused with the real
    /// reason instead of failing at runtime (the motivating case was KawPow's
    /// ~6 GB DAG on a 4 GB card, back when Ravencoin was live).
    pub min_vram_mb: u64,
    /// NATIVE payout-address validity for this coin — the address a user
    /// gives to be paid in the coin itself. The "verify on pool" link for
    /// such an address is `unmineable::stats_url(PayoutAsset::native_for(coin), addr)`.
    pub validate: fn(&str) -> bool,
    /// CoinGecko id for the USD price (the earnings estimate).
    pub coingecko_id: &'static str,
    /// A HeroMiners `/api/stats` URL exposing live network difficulty + average
    /// block reward + `coinUnits` — the other half of the RandomX earnings
    /// estimate (a public stats source; hashes do not go there). Empty for
    /// coins priced from their own network instead (see `auto_rankable`).
    pub stats_url: &'static str,
    /// Where "Need a wallet?" sends a user who doesn't own this coin yet — one
    /// recommended wallet per coin, no choice lists (the app-simplicity brief).
    /// The URL is owned here on the Rust side, never passed from the webview, so
    /// the webview can't be tricked into opening an arbitrary page.
    ///
    /// Monero points at Pasiv's own `/mine` guide for the coin (which names
    /// the recommended wallet AND explains what to paste back).
    /// **Pearl is the exception and it is a safety measure, not a preference:**
    /// multiple look-alike "Pearl wallet" sites rank in search with contradictory
    /// claims, some describing a different chain — so PRL links ONLY to the
    /// official Pearl Research Labs GitHub releases, never a search-ranked domain.
    pub wallet_url: &'static str,
    /// Dark by default. An experimental coin is absent from the picker, the
    /// profit ranking, the doctor and every published lane strip until it is
    /// switched on — the spike-flag discipline, expressed in the roster rather
    /// than sprinkled through call sites. Promote by flipping this to false in
    /// the same commit that adds its hardware verification.
    pub experimental: bool,
}

impl CoinSpec {
    /// Whether this coin is priced by the RandomX formula (`profit::score`,
    /// `price × reward ÷ difficulty`, which only compares *within one PoW*).
    /// Historically the Auto / Max-Profit gate, back when the RandomX family
    /// had three members; today it is the XMR estimate's path, and a GPU coin
    /// (Pearl) is priced from its own network instead.
    pub fn auto_rankable(&self) -> bool {
        self.miner == MinerId::Xmrig && !self.stats_url.is_empty()
    }

    /// Whether *this build* can actually mine this coin.
    ///
    /// The roster is the full catalogue; a row this build cannot run must not
    /// reach the picker, the ranking or `start_mining`, or the user picks a
    /// coin that silently mines nothing. Two gates: unMineable must carry the
    /// coin (the one route), and the miner must run here. The Pearl engine is
    /// GPU on Windows/Linux (CUDA, no macOS build); XMRig runs everywhere.
    /// SRBMiner is RETIRED (0.6.11, 2026-10: its adapter, Defender-exclusion
    /// flow and download left with the signed Pearl engine), and Verus is
    /// RETIRED (0.7.0, the 1 November 2026 date: it only ever mined on the
    /// legacy direct route, which is gone) — both available on no platform.
    /// Explicit arms so a new miner must state its platforms; the `#[allow]`
    /// keeps `-D warnings` happy where the cfg! arms constant-fold into a
    /// `matches!` (which bit the Linux gate before — see git history).
    #[allow(clippy::match_like_matches_macro)]
    pub fn is_available(&self) -> bool {
        let miner_runs_here = match self.miner {
            MinerId::Verus => false,
            MinerId::PearlNative => cfg!(not(target_os = "macos")),
            MinerId::SrbMiner => false,
            MinerId::Xmrig => true,
        };
        self.unmineable.is_some() && miner_runs_here
    }

    /// The pool this coin's hashes go to — unMineable's host for its
    /// algorithm — as `host:port`, or `None` for a coin the route does not
    /// carry. The webview mirror (`roster.gen.ts`) and the diagnostics bundle
    /// render this.
    pub fn pool(&self) -> Option<String> {
        self.unmineable
            .map(|a| format!("{}:{}", a.host(), Algo::PORT))
    }
}

/// The coins this build can mine, in roster order.
pub fn available() -> impl Iterator<Item = &'static CoinSpec> {
    ROSTER
        .iter()
        .filter(|c| c.is_available() && !c.experimental)
}

/// Every coin this build could run *including* experimental ones — for
/// diagnostics and tests only. Never use this to populate user-facing lists.
pub fn available_including_experimental() -> impl Iterator<Item = &'static CoinSpec> {
    ROSTER.iter().filter(|c| c.is_available())
}

/// The supported coins. XMR is XMRig / RandomX on the CPU; PRL is the signed
/// Pearl engine on the GPU — both on unMineable. Wownero was evaluated and
/// excluded: it deliberately has no stratum pools (RandomWOW blocks pool
/// hash-forwarding; it's solo-mining-only), which is incompatible with
/// Pasiv's pool-payout model. Zephyr, Salvium and Verus rows were removed in
/// 0.7.0 with the direct route (see the module note).
pub const ROSTER: &[CoinSpec] = &[
    CoinSpec {
        coin: Coin::Xmr,
        ticker: "xmr",
        miner: MinerId::Xmrig,
        // Named explicitly (XMRig would default to it): unMineable's RandomX
        // host serves several coins, and the login's asset picks the payout.
        algo: Some("rx/0"),
        unmineable: Some(Algo::RandomX),
        min_vram_mb: 0,
        validate: crate::address::is_valid_xmr_address,
        coingecko_id: "monero",
        // A public stats source for the estimate's difficulty/reward; hashes
        // go to unMineable, never here.
        stats_url: "https://monero.herominers.com/api/stats",
        wallet_url: "https://pasiv.network/mine/monero",
        experimental: false,
    },
    CoinSpec {
        coin: Coin::Prl,
        ticker: "prl",
        // GPU → Pearl via the engine Pasiv builds from source and signs itself
        // (CPPminer, MIT; `MinerId::PearlNative`) on unMineable's PearlPow
        // stratum. Windows/Linux only AND needs an eligible GPU — lib.rs only
        // adds the GPU supervisor when hardware::detect() finds a CUDA
        // Turing+/≥3 GB card, so on a GPU-less box start_all reports "PRL
        // can't be mined on this machine yet". The engine is CUDA-only today,
        // so the AMD RX 6000/7000/9000 beta that SRBMiner's OpenCL path
        // carried is off until an AMD card is measured on it
        // (hardware::amd_eligible). Until 2026-10 this row was SRBMiner-Multi
        // (`pearlhash`), closed, unsigned and Defender-quarantined on sight,
        // which is why every Windows prompt Pasiv showed traced back to this
        // lane; the signed engine runs clean from a scanned folder with zero
        // prompts. The engine runs with its fee switched off (`--no-fee`), so
        // the 2% third-party dev fee SRBMiner kept is gone. Pasiv's own 4%
        // applies to Pearl like every coin (until 0.7.0 a legacy direct-route
        // Pearl rig on LuckyPool paid no Pasiv fee; that route is gone).
        miner: MinerId::PearlNative,
        algo: Some("pearlhash"),
        unmineable: Some(Algo::PearlPow),
        // Pearlhash fits in the 3 GB eligibility floor (hardware::parse_nvidia_smi);
        // this pins that floor per-coin instead of relying on it implicitly.
        min_vram_mb: 3072,
        validate: crate::address::is_valid_prl_address,
        coingecko_id: "pearl-2",
        // Different PoW (pearlhash on GPU) — priced from its own network
        // (share-of-network), never by the RandomX formula.
        stats_url: "",
        // Points at our own guide, which then links the Official Pearl Research
        // Labs release and ONLY that (safety — see the field doc). Verified: that
        // repo is the genuine Pearl network monorepo and its release ships signed
        // macOS/Windows/Linux builds; `prl1…` matches the validator above. Never a
        // search-ranked "pearl wallet" domain. Routing through the guide is
        // deliberate: it carries the look-alike-wallet warning, and Pearl was the
        // only coin whose in-app wallet link left pasiv.network (Verus already
        // uses this pattern).
        wallet_url: "https://pasiv.network/mine/pearl",
        experimental: false,
    },
    // ── Retired rows (SRBMiner-era GPU coins): `is_available()` is false
    // everywhere; kept as the record of lanes that were verified on hardware.
    CoinSpec {
        coin: Coin::Rvn,
        ticker: "rvn",
        // RETIRED with 0.6.11 (2026-10-23), a week ahead of its 1 November 2026
        // date: Ravencoin mined on SRBMiner-Multi (algo `kawpow`), and that
        // engine left with the signed Pearl engine, so `is_available()` is false
        // on every platform. The row stays until the November roster cleanup;
        // the tests pin the retirement. History: the SAME engine and GPU lane as
        // Pearl then was, so adding it cost a roster row, not a new sidecar. An
        // ASIC-*resistant* algorithm, which was the whole point: a consumer GPU
        // holds real network share on KawPow, so it earned (unlike Kaspa/Alephium,
        // whose ASIC networks would rank a GPU at ~$0). Windows/Linux + eligible
        // NVIDIA GPU only, like PRL.
        miner: MinerId::SrbMiner,
        algo: Some("kawpow"),
        // Mined on HeroMiners' low-diff port on the direct route; unMineable
        // does not carry it. Hardware-verified on the lab rack 2026-08-17
        // before v0.4.34 shipped: 3 accepted / 0 rejected, ~15.8 MH/s per
        // RTX 4060 — same proof standard as PRL above.
        unmineable: None,
        // KawPow's DAG is ~6 GB and grows slowly with epoch. The old comment
        // called the generic 3 GB eligibility gate "the backstop" — it wasn't:
        // a 4 GB card was offered RVN and would have failed at runtime. Now the
        // picker and start path refuse with the real numbers instead.
        min_vram_mb: 6144,
        validate: crate::address::is_valid_rvn_address,
        coingecko_id: "ravencoin",
        // Different PoW (KawPow on GPU) — can't be RandomX-ranked, and auto_rankable
        // also requires MinerId::Xmrig, so RVN was a manual pick like PRL/VRSC. Its
        // live earnings estimate came through profit::ravencoin_rate(), not this
        // HeroMiners-shaped stats path.
        stats_url: "",
        wallet_url: "https://pasiv.network/mine/ravencoin",
        experimental: false,
    },
    CoinSpec {
        coin: Coin::Erg,
        ticker: "erg",
        // RETIRED with 0.6.11 (2026-10-23), same story as RVN above: Ergo mined
        // on SRBMiner-Multi (algo `autolykos2`), the engine the signed Pearl
        // engine replaced, so `is_available()` is false everywhere and the row
        // stays only until the November roster cleanup. History: the same engine
        // and GPU lane as Pearl/Ravencoin, so a roster row was the whole cost.
        // ASIC-resistant (memory-hard), 1.00% engine dev fee (Readme-verified —
        // half of pearlhash's 2%; no third-party GPU dev fee survives today).
        // The third GPU coin and the established-
        // network hedge: ~$0.11/day on a 4060-class card at the 2026-08-17
        // desk score vs Pearl's ~$0.65 and RVN's ~$0.03 — Pearl stays the top
        // earner, Ergo is the 2019-vintage alternative next to a young token
        // that has emergency-hardforked twice this quarter. Full scoring +
        // three-source cross-check: the Ergo phase-0 measurement (private notes).
        miner: MinerId::SrbMiner,
        algo: Some("autolykos2"),
        // Mined on HeroMiners' "Low-End Hardware" port on the direct route;
        // unMineable does not carry it. Hardware-verified on the lab rack
        // 2026-08-17 (PRL/RVN proof standard): 3 accepted / 0 rejected shares,
        // ~70–71 MH/s per RTX 4060.
        unmineable: None,
        // Autolykos2's DATASET is ~2.5 GB, but the measurement says otherwise
        // about the floor: SRBMiner allocated 7.6 GB on the 8 GB test card. We
        // have not run a smaller card, and a wrong 3 GB floor puts a dead
        // toggle on a 4 GB GPU — the exact failure the VRAM gate exists to
        // prevent. Conservative until measured (the AMD-phase0 rule: widen
        // with evidence, never on optimism).
        min_vram_mb: 6144,
        validate: crate::address::is_valid_erg_address,
        coingecko_id: "ergo",
        // Different PoW (Autolykos2 on GPU) — never RandomX-ranked; live
        // estimate came through profit::ergo_rate(), not this stats path.
        stats_url: "",
        wallet_url: "https://pasiv.network/mine/ergo",
        experimental: false,
    },
    CoinSpec {
        coin: Coin::Etc,
        ticker: "etc",
        // GPU -> Ethereum Classic via SRBMiner-Multi (algo `etchash`) — the engine
        // that also ran Pearl/Ravencoin/Ergo until 0.6.11, so this was a roster
        // row and not a new sidecar. With SRBMiner retired (2026-10) the row is
        // unreachable on every platform (`is_available()`), on top of being
        // experimental; the lane needs an engine again before anything below
        // is more than a record. (Historical; SRBMiner retired 0.6.11:)
        // VERIFIED ON HARDWARE 2026-09-02 (ETC etchash measurement, private notes):
        // SRBMiner 3.5.3 on an RTX 4060, 14.01 MH/s, first accepted share 3m49s
        // after connect, 1 accepted / 0 rejected, and 2Miners' own account API
        // showed the worker online.
        //
        // Two things make ETC different from the other GPU coins:
        //  * Its dev fee was the LOWEST of SRBMiner's GPU algos — 0.65%, against
        //    pearlhash's 2.00%, autolykos2's 1.00% and kawpow's 0.85% (all gone
        //    with SRBMiner; the signed Pearl engine runs with its fee off).
        //  * Its address is a plain EVM `0x`, so a wallet-sign-in user already
        //    has one and needs no new wallet at all.
        //
        // EXPERIMENTAL: dark until an AMD card verifies the vendor claim that is
        // this lane's whole reason for existing — and, since 2026-10, until the
        // lane has an engine at all (AMD is paused on the Pearl engine too, see
        // `hardware::AMD_ON_ENGINE`).
        miner: MinerId::SrbMiner,
        algo: Some("etchash"),
        // Measured against 2Miners (1.0% fee, PPLNS, no registration; the
        // address is the login) on the direct route. unMineable does not carry
        // it; promoting the lane needs a pool decision as well as an engine.
        unmineable: None,
        // ZERO here ON PURPOSE — the static gate cannot express this coin.
        // Etchash's DAG grows 8 MiB/epoch forever (4.605 GB on 2026-09-02,
        // ~323 MiB/year), so the real gate is computed at start time by
        // `crate::etchash::required_vram_mib` against the live chain height.
        // A constant would be wrong within months. Promoting this lane REQUIRES
        // wiring that call into pre-flight; until then the lane is experimental
        // and this zero is unreachable.
        min_vram_mb: 0,
        validate: crate::address::is_valid_etc_address,
        coingecko_id: "ethereum-classic",
        // Deliberately empty: `auto_rankable()` is XMRig-only, because the
        // ranking compares price x reward / difficulty and only cancels the
        // machine's hashrate WITHIN one algorithm. There is no GPU ranker to
        // register with — see the verdict doc.
        stats_url: "",
        wallet_url: "https://pasiv.network/mine/etc",
        experimental: true,
    },
];

/// The spec for a coin. Total: falls back to the first row (XMR) for a coin not
/// in the roster — including the retired `Zeph`/`Sal`/`Vrsc` variants, which
/// a config may still name — so callers never have to handle `None`.
pub fn spec(coin: Coin) -> &'static CoinSpec {
    ROSTER.iter().find(|c| c.coin == coin).unwrap_or(&ROSTER[0])
}

/// Look a coin up by its lowercase ticker (the `payouts` key / webview id).
pub fn by_ticker(ticker: &str) -> Option<&'static CoinSpec> {
    ROSTER.iter().find(|c| c.ticker == ticker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_coin_has_a_safe_wallet_url() {
        for c in ROSTER {
            assert!(
                c.wallet_url.starts_with("https://"),
                "{}: wallet_url must be https",
                c.ticker
            );
            assert!(
                !c.wallet_url.contains(' '),
                "{}: wallet_url has a space",
                c.ticker
            );
        }
        // EVERY coin points at Pasiv's own /mine guide. Pearl was the lone
        // exception only because it had no guide; pasiv.network/mine/pearl shipped
        // 2026-08-28 and links the official Pearl Research Labs release and nothing
        // else, so the safety property is preserved and routed through a page that
        // also warns about look-alike wallet sites.
        for c in ROSTER {
            assert!(
                c.wallet_url.starts_with("https://pasiv.network/mine/"),
                "{}: wallet_url should be a Pasiv guide",
                c.ticker
            );
        }
        // Pearl remains a safety case: whatever it points at must never be a
        // search-ranked look-alike.
        let prl = by_ticker("prl").unwrap();
        assert_eq!(prl.wallet_url, "https://pasiv.network/mine/pearl");
        for bad in [
            "pearlwallet.org",
            "pearlchain.live",
            "pearlbridge",
            "atomic",
        ] {
            assert!(
                !prl.wallet_url.contains(bad),
                "Pearl wallet must never link {bad}"
            );
        }
    }

    #[test]
    fn roster_is_consistent_and_total() {
        assert!(!ROSTER.is_empty());
        // spec() is total and round-trips every roster coin.
        for c in ROSTER {
            assert_eq!(spec(c.coin).ticker, c.ticker);
            assert_eq!(by_ticker(c.ticker).unwrap().coin, c.coin);
        }
        // XMR is the canonical first/default row, and the retired variants a
        // config may still name resolve to it rather than to nothing.
        assert_eq!(ROSTER[0].coin, Coin::Xmr);
        for retired in [Coin::Zeph, Coin::Sal, Coin::Vrsc] {
            assert_eq!(spec(retired).coin, Coin::Xmr, "{retired:?}");
        }
        for t in ["zeph", "sal", "vrsc"] {
            assert!(by_ticker(t).is_none(), "{t} left the roster in 0.7.0");
        }
    }

    /// One route: every mineable coin names its unMineable algorithm, and its
    /// pool is that algorithm's host — the login's asset decides the payout.
    #[test]
    fn every_available_coin_mines_on_unmineable() {
        assert_eq!(spec(Coin::Xmr).unmineable, Some(Algo::RandomX));
        assert_eq!(
            spec(Coin::Xmr).pool().as_deref(),
            Some("rx.unmineable.com:3333")
        );
        assert_eq!(spec(Coin::Prl).unmineable, Some(Algo::PearlPow));
        assert_eq!(
            spec(Coin::Prl).pool().as_deref(),
            Some("pearlpow.unmineable.com:3333")
        );
        for c in ROSTER {
            if c.is_available() {
                assert!(
                    c.unmineable.is_some(),
                    "{} has no unMineable algo",
                    c.ticker
                );
            }
            if c.unmineable.is_none() {
                assert!(!c.is_available(), "{} cannot be mined anywhere", c.ticker);
                assert!(c.pool().is_none());
            }
            // Every coin unMineable carries has a native payout asset, whose
            // validator is the row's own.
            if c.unmineable.is_some() {
                let asset = crate::unmineable::PayoutAsset::native_for(c.coin)
                    .unwrap_or_else(|| panic!("{} needs a native asset", c.ticker));
                assert_eq!(asset.symbol(), c.ticker.to_uppercase());
            }
        }
    }

    #[test]
    fn availability_tracks_the_platform_that_can_actually_mine() {
        // The RandomX family runs the vendored XMRig sidecar, which ships for
        // all three platforms, so those rows are always available.
        for c in ROSTER.iter().filter(|c| c.miner == MinerId::Xmrig) {
            assert!(
                c.is_available(),
                "{} should be available everywhere",
                c.ticker
            );
        }
        // Verus retired with the direct route (0.7.0): no row names it, and
        // a row that did would be unavailable everywhere.
        assert!(ROSTER.iter().all(|c| c.miner != MinerId::Verus));
        // The Pearl engine is CUDA: Windows/Linux. SRBMiner's rows are retired
        // on every platform — a row that stayed pickable would mine nothing.
        for c in ROSTER.iter().filter(|c| c.miner == MinerId::PearlNative) {
            assert_eq!(c.is_available(), cfg!(not(target_os = "macos")));
        }
        for c in ROSTER.iter().filter(|c| c.miner == MinerId::SrbMiner) {
            assert!(!c.is_available(), "{} retired with SRBMiner", c.ticker);
        }

        let avail: Vec<&str> = available().map(|c| c.ticker).collect();
        assert!(avail.contains(&"xmr"), "XMR is available on every platform");
        assert!(!avail.contains(&"vrsc") && !avail.contains(&"zeph") && !avail.contains(&"sal"));
        // Availability filters, never reorders: XMR stays the default first row.
        assert_eq!(avail[0], "xmr");
    }

    #[test]
    fn randomx_pricing_is_the_xmrig_family_only() {
        // The RandomX formula compares coins by network economics assuming the
        // machine's hashrate cancels — only valid within one PoW. The GPU coin
        // (Pearl) is priced from its own network; XMR by the formula.
        for c in ROSTER {
            match c.miner {
                MinerId::Xmrig => assert!(
                    c.auto_rankable(),
                    "{} (XMRig/RandomX) should be RandomX-priced",
                    c.ticker
                ),
                _ => assert!(!c.auto_rankable(), "{} is not RandomX", c.ticker),
            }
            // A RandomX-priced coin must carry a real stats source; others need not.
            assert_eq!(c.auto_rankable(), !c.stats_url.is_empty());
        }
        assert!(by_ticker("xmr").unwrap().auto_rankable());
        assert!(!by_ticker("prl").unwrap().auto_rankable());
    }

    #[test]
    fn every_row_has_a_distinct_ticker_and_native_dashboard() {
        let mut seen = std::collections::HashSet::new();
        for c in ROSTER {
            assert!(seen.insert(c.ticker), "duplicate ticker {}", c.ticker);
        }
        // Unknown ticker resolves to nothing; unknown coin falls back to XMR.
        assert!(by_ticker("doge").is_none());
        // The "verify on pool" link for a native address is unMineable's own
        // page for (asset, address) — only ever built for a valid address.
        let prl = "prl1p7avmp9fu3gfkx64629q050q9d9snvwd07avmp9fu3gfkx64629q050q9d9";
        let asset = crate::unmineable::PayoutAsset::native_for(Coin::Prl).unwrap();
        assert!((spec(Coin::Prl).validate)(prl));
        assert_eq!(
            crate::unmineable::stats_url(asset, prl).as_deref(),
            Some(format!("https://unmineable.com/address/{prl}?coin=PRL").as_str())
        );
        assert_eq!(crate::unmineable::stats_url(asset, "ADDR123"), None);
    }
}

#[cfg(test)]
mod etc_lane_tests {
    use super::*;

    fn etc() -> &'static CoinSpec {
        ROSTER
            .iter()
            .find(|c| c.ticker == "etc")
            .expect("etc in roster")
    }

    /// THE FLAG TEST. The brief's hardest requirement is that ETC is invisible
    /// when off, and "invisible" has to mean the roster itself, not a check at
    /// each call site that someone will forget.
    ///
    /// ⚠️ HONEST LIMIT: on macOS this test passes for the WRONG REASON. ETC is a
    /// SRBMiner coin and there was no macOS SRBMiner build, so `is_available()`
    /// already excludes it and deleting the `!c.experimental` filter changes
    /// nothing here — verified by mutation on 2026-09-02, where removing the
    /// filter left all tests green on a Mac. Since SRBMiner retired in 0.6.11
    /// the same is true on EVERY platform (see the comment inside), so the
    /// guard is load-bearing nowhere today and MUST be mutation-checked on
    /// Windows/Linux before this lane is promoted on a new engine.
    /// `no_experimental_coin_ever_reaches_available` below is the
    /// platform-independent half.
    #[test]
    fn etc_is_dark_by_default() {
        assert!(etc().experimental);
        assert!(
            !available().any(|c| c.ticker == "etc"),
            "an experimental coin must not reach the picker, ranker or doctor"
        );
        // …and since SRBMiner retired (2026-10) it is reachable NOWHERE, not
        // even for diagnostics: its row names MinerId::SrbMiner, which
        // `is_available()` refuses on every platform. (Until then this asserted
        // the SRBMiner platform gate — present off macOS — and that is what
        // the Linux release leg caught when the gate closed.)
        assert!(
            !available_including_experimental().any(|c| c.ticker == "etc"),
            "ETC retired with SRBMiner on every platform"
        );
    }

    /// The invariant that holds on every platform: whatever `available()`
    /// yields, none of it is experimental. This is what actually encodes the
    /// promise; the ETC-specific test above is a stronger statement that only
    /// one platform can make.
    #[test]
    fn no_experimental_coin_ever_reaches_available() {
        for c in available() {
            assert!(!c.experimental, "{} leaked into available()", c.ticker);
        }
    }

    #[test]
    fn no_shipped_coin_is_accidentally_experimental() {
        for c in ROSTER.iter().filter(|c| c.ticker != "etc") {
            assert!(!c.experimental, "{} must stay shipped", c.ticker);
        }
    }

    #[test]
    fn etc_validates_evm_addresses_and_rejects_other_coins() {
        let v = etc().validate;
        assert!(v("0xdAC17F958D2ee523a2206206994597C13D831ec7"));
        assert!(!v("0xDAC17F958D2ee523a2206206994597C13D831ec7")); // bad checksum
        assert!(!v("9f4QF8AD1nQ3nJahQVkMj8hFSVVzVom77b52JU7EW71Zexg6N8v")); // ERG
    }

    /// The static VRAM gate is deliberately unusable for this coin, and that
    /// must be loud rather than look like an oversight.
    #[test]
    fn etc_does_not_use_the_static_vram_gate() {
        assert_eq!(etc().min_vram_mb, 0, "ETC sizes its DAG at start time");
        // Every OTHER SRBMiner-era GPU coin (retired rows too) does use it, so
        // the zero is specific, not a bug.
        for c in ROSTER
            .iter()
            .filter(|c| c.miner == MinerId::SrbMiner && c.ticker != "etc")
        {
            assert!(
                c.min_vram_mb > 0,
                "{} should still gate statically",
                c.ticker
            );
        }
    }

    /// ETC is not auto-rankable, for the same reason VRSC/PRL/RVN/ERG are not:
    /// cross-algorithm ranking is meaningless. Pinned so a future "just add it
    /// to Auto" does not silently start comparing MH/s against H/s.
    #[test]
    fn etc_is_not_auto_rankable() {
        assert!(!etc().auto_rankable());
        assert!(etc().stats_url.is_empty());
    }
}
