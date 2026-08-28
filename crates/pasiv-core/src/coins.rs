// SPDX-License-Identifier: GPL-3.0-only
//! Coin roster — the single source of truth (Rust side) for which coins Pasiv
//! can mine and everything coin-specific about each: which vendored miner
//! handles it, its algorithm, default pool, address-validation rule, and the
//! "verify on pool" dashboard link. Adding a coin = one row here (plus a
//! vendored binary only if it needs a *new* miner). The supervisor, governor,
//! and fee engine stay coin-agnostic.
//!
//! Display-only fields (human name, UI accent, input placeholder) live in the
//! webview mirror `src/coins.ts`, keyed by the same lowercase `ticker`, so
//! they never become dead code here. Keep the two in sync — same tolerated
//! pattern as the address validators.

use crate::types::{Coin, MinerId};

pub struct CoinSpec {
    pub coin: Coin,
    /// Lowercase ticker — also the `payouts` map key and the webview's coin id.
    pub ticker: &'static str,
    /// Which vendored binary/adapter mines this coin.
    pub miner: MinerId,
    /// Miner algorithm flag (XMRig `-a`), or None for the miner's default
    /// (RandomX `rx/0`, which XMRig auto-negotiates for Monero).
    pub algo: Option<&'static str>,
    pub pool_host: &'static str,
    pub pool_port: u16,
    pub tls: bool,
    /// Minimum GPU VRAM (MB) this coin's algorithm needs — 0 for CPU coins.
    /// Gated in `commands::resolve_gpu_for_start` / `coin_availability` against
    /// the LARGEST eligible GPU, so a too-small card is refused with the real
    /// reason instead of failing at runtime (KawPow's DAG on a 4 GB card).
    pub min_vram_mb: u64,
    /// Payout-address validity for this coin.
    pub validate: fn(&str) -> bool,
    /// The pool's human dashboard for a given payout address.
    pub dashboard_url: fn(&str) -> String,
    /// CoinGecko id for the USD price (Auto / Max-Profit ranking).
    pub coingecko_id: &'static str,
    /// A HeroMiners `/api/stats` URL exposing live network difficulty + average
    /// block reward + `coinUnits` — the other half of the profit calculation.
    /// (For XMR the mining pool is MoneroOcean, but the stats source is
    /// HeroMiners' Monero pool, so every coin reads through one JSON shape.)
    /// Empty for coins excluded from Auto ranking (see `auto_rankable`).
    pub stats_url: &'static str,
    /// Where "Need a wallet?" sends a user who doesn't own this coin yet — one
    /// recommended wallet per coin, no choice lists (the app-simplicity brief).
    /// The URL is owned here on the Rust side, never passed from the webview, so
    /// the webview can't be tricked into opening an arbitrary page.
    ///
    /// Monero/Zephyr/Salvium/Verus point at Pasiv's own `/mine` guide for that
    /// coin (which names the recommended wallet AND explains what to paste back).
    /// **Pearl is the exception and it is a safety measure, not a preference:**
    /// multiple look-alike "Pearl wallet" sites rank in search with contradictory
    /// claims, some describing a different chain — so PRL links ONLY to the
    /// official Pearl Research Labs GitHub releases, never a search-ranked domain.
    pub wallet_url: &'static str,
}

impl CoinSpec {
    /// Whether Auto / Max-Profit may rank this coin. The ranking (`profit::score`)
    /// is `price × reward ÷ difficulty`, which is only comparable *within one PoW*
    /// — it assumes the machine's hashrate is the same across the candidates so it
    /// cancels out. That holds for the RandomX family (XMR/ZEPH/SAL, all XMRig) but
    /// NOT across algorithms: a Mac does ~1000× more VerusHash/s than RandomX/s, so
    /// ranking VRSC against XMR on network economics alone would be meaningless.
    /// VRSC is therefore a manual pick; Auto manages only the RandomX family.
    pub fn auto_rankable(&self) -> bool {
        self.miner == MinerId::Xmrig && !self.stats_url.is_empty()
    }

    /// Whether *this build* can actually mine this coin.
    ///
    /// The roster is the full catalogue, but Verus mines in-process through the
    /// vendored VerusHash C++, which only builds on macOS (`lib.rs` registers
    /// its supervisor under the same cfg). Offering it on Linux or Windows
    /// would give the user a coin to pick that silently mines nothing, so the
    /// picker, the profit ranking and `start_mining` all filter on this.
    /// Verus is macOS-only (in-process VerusHash C++ builds only there); SRBMiner
    /// is the inverse — GPU/Pearl on Windows/Linux, no macOS build. XMRig runs
    /// everywhere. Explicit arms so a new miner must state its platforms; the
    /// `#[allow]` keeps `-D warnings` happy where the cfg! arms constant-fold
    /// into a `matches!` (which bit the Linux gate before — see git history).
    #[allow(clippy::match_like_matches_macro)]
    pub fn is_available(&self) -> bool {
        match self.miner {
            MinerId::Verus => cfg!(target_os = "macos"),
            MinerId::SrbMiner => cfg!(not(target_os = "macos")),
            MinerId::Xmrig => true,
        }
    }
}

/// The coins this build can mine, in roster order.
pub fn available() -> impl Iterator<Item = &'static CoinSpec> {
    ROSTER.iter().filter(|c| c.is_available())
}

fn xmr_dashboard(addr: &str) -> String {
    // MoneroOcean's SPA needs BOTH the `?addr=` query (seeds the address) AND the
    // `#/wallet/<addr>/overview` hash route (navigates to that wallet) — the query
    // alone landed on the dashboard without opening the wallet. The address
    // therefore appears twice, once in each. Confirmed working against a real
    // wallet.
    format!("https://moneroocean.stream/?addr={addr}#/wallet/{addr}/overview")
}

// HeroMiners (zeph + sal): the `?address=` inside the hash is IGNORED by
// today's SPA (verified live 2026-08-11 — no stats_address XHR fires, "You:
// N/A"); their UI keeps the address client-side after one manual lookup. The
// link still lands on the dashboard route where that lookup lives, so it stays
// — and self-repairs if the pool honors the param again.
fn zeph_dashboard(addr: &str) -> String {
    format!("https://zephyr.herominers.com/#/dashboard?address={addr}")
}

fn sal_dashboard(addr: &str) -> String {
    format!("https://salvium.herominers.com/#/dashboard?address={addr}")
}

fn vrsc_dashboard(addr: &str) -> String {
    // LuckPool's own lookup form navigates to `miner.html?<ADDRESS>` — the bare
    // address IS the query string (verified live 2026-08-11: this form renders
    // the wallet + fires /verus/miner/<addr>). The old `?address=` made the SPA
    // read the query KEY, querying the literal string "address" — a permanently
    // empty dashboard.
    format!("https://luckpool.net/verus/miner.html?{addr}")
}

/// Must track `pool_host` below — a dashboard on a pool we no longer submit to
/// shows the user a permanently empty page. HONEST LIMIT (verified in the
/// pool's own bundle, 2026-08-11): LuckyPool's SPA reads the wallet ONLY from
/// localStorage — no URL shape seeds it (path/hash/query all ignored; the old
/// "verified HTTP 200" was the SPA shell, not the wallet view). The page opens
/// on its Look Up form; one paste and the pool remembers the address. The
/// address stays in the path anyway: harmless today, self-repairing if the
/// pool ever honors it.
fn prl_dashboard(addr: &str) -> String {
    format!("https://pearl.luckypool.io/miner-stats/{addr}")
}

/// HeroMiners Ravencoin dashboard — same SPA shape as zeph/sal (the address is
/// kept client-side after one lookup; the route is what matters). Non-regional
/// host: stats/dashboard are global even though mining submits to the `de.` pool.
fn rvn_dashboard(addr: &str) -> String {
    format!("https://ravencoin.herominers.com/#/dashboard?address={addr}")
}

fn erg_dashboard(addr: &str) -> String {
    format!("https://ergo.herominers.com/#/dashboard?address={addr}")
}

/// The supported coins. XMR + ZEPH + SAL are all XMRig / RandomX (rx/0) — a new
/// row and a payout address is the whole cost of adding one. VRSC is VerusHash,
/// mined IN-PROCESS by the `verus` adapter (macOS-only; no vendored sidecar).
/// Wownero was evaluated and excluded: it deliberately has no stratum pools
/// (RandomWOW blocks pool hash-forwarding; it's solo-mining-only), which is
/// incompatible with Pasiv's pool-payout model.
pub const ROSTER: &[CoinSpec] = &[
    CoinSpec {
        coin: Coin::Xmr,
        ticker: "xmr",
        miner: MinerId::Xmrig,
        algo: None,
        pool_host: "gulf.moneroocean.stream",
        pool_port: 10128,
        tls: false,
        min_vram_mb: 0,
        validate: crate::address::is_valid_xmr_address,
        dashboard_url: xmr_dashboard,
        coingecko_id: "monero",
        stats_url: "https://monero.herominers.com/api/stats",
        wallet_url: "https://pasiv.network/mine/monero",
    },
    CoinSpec {
        coin: Coin::Zeph,
        ticker: "zeph",
        miner: MinerId::Xmrig,
        // RandomX rx/0 (same PoW as XMR); the pool + address select the chain.
        // Verified live: xmrig -o de.zephyr.herominers.com:1123 -a rx/0 connects
        // and receives ZEPH jobs.
        algo: Some("rx/0"),
        pool_host: "de.zephyr.herominers.com",
        pool_port: 1123,
        tls: false,
        min_vram_mb: 0,
        validate: crate::address::is_valid_zeph_address,
        dashboard_url: zeph_dashboard,
        coingecko_id: "zephyr-protocol",
        stats_url: "https://zephyr.herominers.com/api/stats",
        wallet_url: "https://pasiv.network/mine/zephyr",
    },
    CoinSpec {
        coin: Coin::Sal,
        ticker: "sal",
        miner: MinerId::Xmrig,
        // RandomX rx/0 (same PoW as XMR/ZEPH); the pool + Carrot address select
        // the chain. Verified live: xmrig -o de.salvium.herominers.com:1230
        // -a rx/0 connects and receives SAL jobs. (herominers' published :1228
        // is dead; :1230 is the live stratum port on the same host.)
        algo: Some("rx/0"),
        pool_host: "de.salvium.herominers.com",
        pool_port: 1230,
        tls: false,
        min_vram_mb: 0,
        validate: crate::address::is_valid_sal_address,
        dashboard_url: sal_dashboard,
        coingecko_id: "salvium",
        stats_url: "https://salvium.herominers.com/api/stats",
        wallet_url: "https://pasiv.network/mine/salvium",
    },
    CoinSpec {
        coin: Coin::Vrsc,
        ticker: "vrsc",
        // The flagship: VerusHash V2.2, mined IN-PROCESS by the `verus` adapter
        // (native arm64 hash + a Rust stratum client) — no vendored sidecar.
        // Proven end-to-end with a real accepted share on LuckPool (docs/verus/).
        miner: MinerId::Verus,
        // Algo is intrinsic to the adapter (VerusHash); the XMRig `-a` flag is N/A.
        algo: None,
        pool_host: "na.luckpool.net",
        pool_port: 3956,
        tls: false,
        min_vram_mb: 0,
        validate: crate::address::is_valid_vrsc_address,
        dashboard_url: vrsc_dashboard,
        coingecko_id: "verus-coin",
        // Excluded from Auto ranking (different PoW — see CoinSpec::auto_rankable),
        // so no HeroMiners-shaped stats source is needed.
        stats_url: "",
        wallet_url: "https://pasiv.network/mine/verus",
    },
    CoinSpec {
        coin: Coin::Prl,
        ticker: "prl",
        // GPU → Pearl via SRBMiner-Multi (algo `pearlhash`) on LuckyPool's plain
        // stratum. Windows/Linux only (no macOS SRBMiner build) AND needs an
        // eligible NVIDIA GPU — lib.rs only adds the SrbMiner supervisor when
        // hardware::detect() finds a CUDA Turing+/≥3 GB card, so on a GPU-less
        // box start_all reports "PRL can't be mined on this machine yet".
        // SRBMiner takes a 2% pearlhash dev fee (third-party, disclosed like
        // XMRig's 1%); LuckyPool takes its own pool fee. Pasiv's own 4% fee is
        // XMR-only, so it never applies to PRL.
        miner: MinerId::SrbMiner,
        algo: Some("pearlhash"),
        // Was pearl.alphapool.tech:5571, shipped in 0.3.8 on a "verify on the test
        // box" note that never happened. Verified now, and it does not work: the
        // host resolves only to Cloudflare (104.21.90.102 / 172.67.199.216), which
        // proxies HTTP/HTTPS and not arbitrary stratum ports, so TCP 5571 never
        // connects. SRBMiner sat at time_connected=0, last_job_received=0, zero
        // shares, GPU parked at idle clocks (210 MHz, 3 W) — the miner ran and
        // looked healthy while earning nothing.
        //
        // LuckyPool is the fallback that same note named, and is one of the three
        // pools SRBMiner's own start-mining-pearl.bat ships with. Verified live on
        // an RTX 4070 SUPER: connects, jobs flow, 11 accepted / 0 rejected in the
        // first 191 s at 2460 MHz / 219 W / 99% utilisation, and the pool's
        // /api/stats_address confirms the shares landed server-side.
        pool_host: "pearl-eu2.luckypool.io",
        pool_port: 3360,
        tls: false,
        // Pearlhash fits in the 3 GB eligibility floor (hardware::parse_nvidia_smi);
        // this pins that floor per-coin instead of relying on it implicitly.
        min_vram_mb: 3072,
        validate: crate::address::is_valid_prl_address,
        dashboard_url: prl_dashboard,
        coingecko_id: "pearl-2",
        // Different PoW (pearlhash on GPU) — can't be RandomX-ranked; auto_rankable
        // also requires MinerId::Xmrig, so PRL is a manual pick like VRSC.
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
    },
    CoinSpec {
        coin: Coin::Rvn,
        ticker: "rvn",
        // GPU → Ravencoin via SRBMiner-Multi (algo `kawpow`) — the SAME engine and
        // GPU lane as Pearl, so adding it cost a roster row, not a new sidecar. An
        // ASIC-*resistant* algorithm, which is the whole point: a consumer GPU
        // holds real network share on KawPow, so it earns (unlike Kaspa/Alephium,
        // whose ASIC networks would rank a GPU at ~$0). Windows/Linux + eligible
        // NVIDIA GPU only, like PRL.
        miner: MinerId::SrbMiner,
        algo: Some("kawpow"),
        // HeroMiners' low-diff port (64), the right default for a single consumer
        // GPU; 1141 is the high-diff/rig port. Stats + dashboard live on the
        // non-regional host; mining submits to the `de.` regional pool.
        // Hardware-verified on the lab rack 2026-08-17 before v0.4.34 shipped:
        // pool connected, login/address accepted, 3 accepted / 0 rejected,
        // ~15.8 MH/s per RTX 4060 — same proof standard as PRL above.
        pool_host: "de.ravencoin.herominers.com",
        pool_port: 1140,
        tls: false,
        // KawPow's DAG is ~6 GB and grows slowly with epoch. The old comment
        // called the generic 3 GB eligibility gate "the backstop" — it wasn't:
        // a 4 GB card was offered RVN and would have failed at runtime. Now the
        // picker and start path refuse with the real numbers instead.
        min_vram_mb: 6144,
        validate: crate::address::is_valid_rvn_address,
        dashboard_url: rvn_dashboard,
        coingecko_id: "ravencoin",
        // Different PoW (KawPow on GPU) — can't be RandomX-ranked, and auto_rankable
        // also requires MinerId::Xmrig, so RVN is a manual pick like PRL/VRSC. Its
        // live earnings estimate comes through profit::ravencoin_rate(), not this
        // HeroMiners-shaped stats path.
        stats_url: "",
        wallet_url: "https://pasiv.network/mine/ravencoin",
    },
    CoinSpec {
        coin: Coin::Erg,
        ticker: "erg",
        // GPU → Ergo via SRBMiner-Multi (algo `autolykos2`) — the same engine
        // and GPU lane as Pearl/Ravencoin, so a roster row is the whole cost.
        // ASIC-resistant (memory-hard), 1.00% engine dev fee (Readme-verified —
        // half of pearlhash's 2%). The third GPU coin and the established-
        // network hedge: ~$0.11/day on a 4060-class card at the 2026-08-17
        // desk score vs Pearl's ~$0.65 and RVN's ~$0.03 — Pearl stays the top
        // earner, Ergo is the 2019-vintage alternative next to a young token
        // that has emergency-hardforked twice this quarter. Full scoring +
        // three-source cross-check: docs/spikes/ERGO-phase0.md.
        miner: MinerId::SrbMiner,
        algo: Some("autolykos2"),
        // HeroMiners' "Low-End Hardware" port (diff 4G) — right for a single
        // consumer GPU; mining submits to the `de.` regional host. TCP-verified
        // open (353 ms) 2026-08-17. Same operator/API family as our ZEPH/SAL
        // stats sources.
        // Hardware-verified on the lab rack 2026-08-17 (PRL/RVN proof
        // standard): pool connected + wallet accepted, 3 accepted / 0 rejected
        // shares (avg find 67 s at the port's 4G diff), ~70–71 MH/s per
        // RTX 4060, 402 ms latency to the de. host.
        pool_host: "de.ergo.herominers.com",
        pool_port: 1180,
        tls: false,
        // Autolykos2's DATASET is ~2.5 GB, but the measurement says otherwise
        // about the floor: SRBMiner allocated 7.6 GB on the 8 GB test card. We
        // have not run a smaller card, and a wrong 3 GB floor puts a dead
        // toggle on a 4 GB GPU — the exact failure the VRAM gate exists to
        // prevent. Conservative until measured (the AMD-phase0 rule: widen
        // with evidence, never on optimism).
        min_vram_mb: 6144,
        validate: crate::address::is_valid_erg_address,
        dashboard_url: erg_dashboard,
        coingecko_id: "ergo",
        // Different PoW (Autolykos2 on GPU) — never RandomX-ranked; live
        // estimate comes through profit::ergo_rate(), not this stats path.
        stats_url: "",
        wallet_url: "https://pasiv.network/mine/ergo",
    },
];

/// The spec for a coin. Total: falls back to the first row (XMR) for a coin not
/// in the roster, so callers never have to handle `None`.
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
        // EVERY coin now points at Pasiv's own /mine guide. Pearl was the lone
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
        // XMR is the canonical first/default row.
        assert_eq!(ROSTER[0].coin, Coin::Xmr);
        assert_eq!(spec(Coin::Xmr).pool_host, "gulf.moneroocean.stream");
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
        // Verus mines in-process through macOS-only VerusHash, and lib.rs only
        // registers its supervisor there — the two cfgs must agree, or the
        // picker offers a coin that mines nothing.
        for c in ROSTER.iter().filter(|c| c.miner == MinerId::Verus) {
            assert_eq!(c.is_available(), cfg!(target_os = "macos"));
        }

        let avail: Vec<&str> = available().map(|c| c.ticker).collect();
        assert!(avail.contains(&"xmr"), "XMR is available on every platform");
        assert_eq!(avail.contains(&"vrsc"), cfg!(target_os = "macos"));
        // Availability filters, never reorders: XMR stays the default first row.
        assert_eq!(avail[0], "xmr");
    }

    #[test]
    fn auto_ranking_is_randomx_family_only() {
        // Auto / Max-Profit compares coins by network economics assuming the
        // machine's hashrate cancels — only valid within one PoW. VRSC (VerusHash)
        // must be excluded; the XMRig/RandomX coins must be included.
        for c in ROSTER {
            match c.miner {
                MinerId::Verus => assert!(
                    !c.auto_rankable(),
                    "{} (Verus) must not be auto-ranked",
                    c.ticker
                ),
                MinerId::Xmrig => assert!(
                    c.auto_rankable(),
                    "{} (XMRig/RandomX) should be auto-rankable",
                    c.ticker
                ),
                _ => {}
            }
        }
        // A rankable coin must carry a real stats source; a non-rankable one need not.
        assert!(!by_ticker("vrsc").unwrap().auto_rankable());
        assert!(by_ticker("xmr").unwrap().auto_rankable());
    }

    #[test]
    fn every_row_has_a_working_dashboard_and_distinct_ticker() {
        let mut seen = std::collections::HashSet::new();
        for c in ROSTER {
            assert!(seen.insert(c.ticker), "duplicate ticker {}", c.ticker);
            let url = (c.dashboard_url)("ADDR123");
            assert!(
                url.starts_with("https://"),
                "{}: dashboard not https",
                c.ticker
            );
            assert!(
                url.contains("ADDR123"),
                "{}: dashboard drops the address",
                c.ticker
            );
            // Rankable coins need a stats endpoint; VRSC deliberately has none.
            assert_eq!(c.auto_rankable(), !c.stats_url.is_empty());
        }
        // Unknown ticker resolves to nothing; unknown coin falls back to XMR.
        assert!(by_ticker("doge").is_none());
    }

    /// Exact deep-link shapes, pinned against LIVE verification (2026-08-11,
    /// headless render of each URL with the real payout addresses — see the
    /// per-fn comments). The bug class this guards: a URL that returns HTTP 200
    /// while the SPA silently ignores the address — every "verify on pool"
    /// click landing on an empty page. VRSC is the sharp edge: the pool's
    /// canonical form is the BARE address as the whole query string; a
    /// well-meaning "fix" back to `?address=` makes the SPA query the literal
    /// word "address".
    #[test]
    fn dashboard_urls_keep_their_verified_shapes() {
        let url = |t: &str| (by_ticker(t).unwrap().dashboard_url)("ADDR123");
        // MoneroOcean needs the address twice: query seeds it, hash routes to it.
        assert_eq!(
            url("xmr"),
            "https://moneroocean.stream/?addr=ADDR123#/wallet/ADDR123/overview"
        );
        // LuckPool: bare-address query — no `address=` key, ever.
        assert_eq!(url("vrsc"), "https://luckpool.net/verus/miner.html?ADDR123");
        assert!(
            !url("vrsc").contains("address="),
            "vrsc regressed to ?address="
        );
        // Landing pages (SPA keeps the wallet client-side): route must survive.
        assert!(url("zeph").contains("#/dashboard"));
        assert!(url("sal").contains("#/dashboard"));
        assert!(url("prl").starts_with("https://pearl.luckypool.io/miner-stats"));
    }
}
