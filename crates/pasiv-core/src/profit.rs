// SPDX-License-Identifier: GPL-3.0-only
//! Auto / Max-Profit ranking math — the pure half of the desktop's profit
//! module. The live fetchers (CoinGecko prices, pool network stats) live with
//! each runner; everything here is a deterministic function of their inputs,
//! which is exactly the part worth auditing: *how* Pasiv ranks coins, and that
//! it ranks on the user's take-home, never on gross.
//!
//! Every auto-rankable coin shares one algorithm (RandomX rx/0), so for a
//! fixed hashrate the ranking reduces to `price × block_reward ÷
//! network_difficulty` (block time cancels out). Coins on other PoWs (Verus,
//! Pearl, Ravencoin, Ergo) are priced by their share of that coin's own
//! network instead, and are never auto-switched against the RandomX family.

use serde::Serialize;

use crate::types::Coin;

/// Switch only when the leader beats the current coin by this factor —
/// hysteresis so a near-tie doesn't cost a warmup (a switch restarts the
/// miner: cold RandomX dataset, ~30–60 s at 0 H/s).
pub const SWITCH_MARGIN: f64 = 1.08;

/// The Verus mainnet block interval. Fixed by consensus.
pub const VERUS_BLOCK_SECS: f64 = 60.0;

/// Ravencoin's block reward in RVN. Fixed by consensus between halvings: 5000
/// at launch → 2500 (Jan 2022) → **1250 (the Jan 2026 halving, in effect)**.
/// The next halving is ~2030, so a constant is honest until then; if RVN's
/// live estimate is ever off by a clean 2×, this is the first thing to check.
pub const RVN_BLOCK_REWARD: f64 = 1250.0;

/// Ravencoin's target block time (seconds) — 1 minute, a consensus constant.
pub const RVN_BLOCK_SECS: f64 = 60.0;

/// Ergo's block reward in ERG — 3 flat in the EIP-27 re-emission era.
/// Revisit only on an emission-schedule change.
pub const ERG_BLOCK_REWARD: f64 = 3.0;

/// Ergo's target block time (seconds) — 2 minutes, a consensus constant.
pub const ERG_BLOCK_SECS: f64 = 120.0;

/// A coin's live revenue estimate, for the UI's "what and why".
#[derive(Debug, Clone, Serialize)]
pub struct CoinScore {
    pub ticker: String,
    /// USD/day at 1 kH/s, **net of Pasiv's own fee** — the user's take-home, so
    /// the ranking Auto acts on and the ranking the UI shows are the same number
    /// the user actually earns. Pasiv's fee is XMR-only, so this only differs
    /// from gross for XMR (see [`crate::fee::fee_fraction`]). Comparable across
    /// coins on the same algorithm.
    pub usd_per_day_kh: f64,
}

/// Revenue proxy in USD/day at 1 kH/s. `None` if any input is non-positive
/// (missing price, zero difficulty, …) — such a coin is simply not ranked.
pub fn score(price_usd: f64, reward_atomic: f64, coin_units: f64, difficulty: f64) -> Option<f64> {
    if price_usd <= 0.0 || reward_atomic <= 0.0 || coin_units <= 0.0 || difficulty <= 0.0 {
        return None;
    }
    let reward = reward_atomic / coin_units; // atomic units → whole coins
    Some(86_400.0 * 1_000.0 * price_usd * reward / difficulty)
}

/// A gross score reduced to the user's take-home for the given coin. This is
/// the one line that makes the ranking honest: Auto must never prefer a coin
/// because of revenue the user doesn't receive. Charged fee and ranked fee
/// come from the same [`crate::fee::fee_fraction`], so they cannot disagree.
pub fn net_score(gross: f64, coin: Coin) -> f64 {
    gross * (1.0 - crate::fee::fee_fraction(coin))
}

/// USD/day at 1 kH/s for a coin priced by its **share of network hashrate** —
/// the model for coins that can't be RandomX-ranked because they run a
/// different PoW (Verus/VerusHash, Pearl/pearlhash, Ravencoin/KawPow,
/// Ergo/Autolykos2). Your share of the network at 1 kH/s is
/// `1000 / network_hashps`, times blocks/day, times reward, times price.
/// `None` if any input is non-positive, matching `score()`.
pub fn share_of_network_score(
    price_usd: f64,
    reward: f64,
    network_hashps: f64,
    block_secs: f64,
) -> Option<f64> {
    if price_usd <= 0.0 || reward <= 0.0 || network_hashps <= 0.0 || block_secs <= 0.0 {
        return None;
    }
    let blocks_per_day = 86_400.0 / block_secs;
    Some((1_000.0 / network_hashps) * blocks_per_day * reward * price_usd)
}

/// Verus's earnings rate — the share-of-network model fed Verus's own numbers.
pub fn verus_score(
    price_usd: f64,
    reward_vrsc: f64,
    network_hashps: f64,
    block_secs: f64,
) -> Option<f64> {
    share_of_network_score(price_usd, reward_vrsc, network_hashps, block_secs)
}

/// Pure switch decision. Given the ranked scores (desc), the tickers the user can
/// actually mine (valid payout set), and the current coin's ticker, return the
/// ticker to switch to — or `None` to stay put. Passing `current = ""` makes it a
/// plain "pick the best mineable coin" (used at start-up). Switches only on a
/// margin over the current coin, so ties don't thrash.
pub fn choose(ranked: &[CoinScore], mineable: &[String], current: &str) -> Option<String> {
    let best = ranked
        .iter()
        .find(|c| mineable.iter().any(|m| m == &c.ticker))?;
    if best.ticker == current {
        return None;
    }
    let cur = ranked
        .iter()
        .find(|c| c.ticker == current)
        .map(|c| c.usd_per_day_kh)
        .unwrap_or(0.0);
    (best.usd_per_day_kh > cur * SWITCH_MARGIN).then(|| best.ticker.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(t: &str, v: f64) -> CoinScore {
        CoinScore {
            ticker: t.into(),
            usd_per_day_kh: v,
        }
    }

    #[test]
    fn score_is_price_times_reward_over_difficulty() {
        // 1 coin/block reward, 1e0 units, difficulty 1e6, $10 → known value.
        let s = score(10.0, 1.0, 1.0, 1_000_000.0).unwrap();
        assert!((s - 86_400.0 * 1_000.0 * 10.0 / 1_000_000.0).abs() < 1e-9);
        // Higher difficulty → lower score; higher price → higher score.
        assert!(score(10.0, 1.0, 1.0, 2_000_000.0).unwrap() < s);
        assert!(score(20.0, 1.0, 1.0, 1_000_000.0).unwrap() > s);
        // Bad inputs drop the coin from the ranking.
        assert!(score(0.0, 1.0, 1.0, 1.0).is_none());
        assert!(score(10.0, 1.0, 1.0, 0.0).is_none());
    }

    #[test]
    fn net_score_subtracts_exactly_the_charged_fee() {
        // XMR: take-home = gross × (1 − 4%); every fee-free coin passes through.
        let gross = 100.0;
        assert!((net_score(gross, Coin::Xmr) - 96.0).abs() < 1e-9);
        for coin in [
            Coin::Zeph,
            Coin::Sal,
            Coin::Vrsc,
            Coin::Prl,
            Coin::Rvn,
            Coin::Erg,
        ] {
            assert_eq!(net_score(gross, coin), gross);
        }
    }

    #[test]
    fn verus_score_reflects_share_of_network() {
        // Live-ish figures (2026-07-26): $0.367 price, 3 VRSC miner reward,
        // ~1.36 TH/s network, 60 s blocks → a Mac at 7.4 MH/s earns under a cent
        // a day. This is the honest signal: Verus's big MH/s number is worth
        // little against an FPGA-dominated network.
        let rate = verus_score(0.367, 3.0, 1_360_910_012_433.0, 60.0).unwrap();
        let per_day_at_7_4mh = (7_400_000.0 / 1000.0) * rate;
        assert!(
            per_day_at_7_4mh > 0.0 && per_day_at_7_4mh < 0.05,
            "expected a few cents/day at most, got {per_day_at_7_4mh}"
        );

        // Monotonic the way you'd expect, and bad inputs drop out.
        let base = verus_score(0.367, 3.0, 1e12, 60.0).unwrap();
        assert!(verus_score(0.367, 3.0, 2e12, 60.0).unwrap() < base); // more network → less
        assert!(verus_score(0.734, 3.0, 1e12, 60.0).unwrap() > base); // higher price → more
        assert!(verus_score(0.0, 3.0, 1e12, 60.0).is_none());
        assert!(verus_score(0.367, 3.0, 0.0, 60.0).is_none());
    }

    #[test]
    fn choose_picks_best_mineable_and_respects_hysteresis() {
        let ranked = vec![cs("zeph", 100.0), cs("xmr", 96.0), cs("sal", 60.0)];
        let all = vec!["xmr".to_string(), "zeph".into(), "sal".into()];

        // From nothing (start-up): pick the outright best mineable coin.
        assert_eq!(choose(&ranked, &all, "").as_deref(), Some("zeph"));

        // Already on the best → stay put.
        assert_eq!(choose(&ranked, &all, "zeph"), None);

        // On XMR (96) vs ZEPH (100): only ~4% better, under the 8% margin → stay.
        assert_eq!(choose(&ranked, &all, "xmr"), None);

        // On SAL (60) vs ZEPH (100): a clear win → switch.
        assert_eq!(choose(&ranked, &all, "sal").as_deref(), Some("zeph"));

        // Only XMR has a payout set → never leaves XMR even though ZEPH scores more.
        let only_xmr = vec!["xmr".to_string()];
        assert_eq!(choose(&ranked, &only_xmr, "xmr"), None);
        // …and from start-up with only XMR mineable, it picks XMR.
        assert_eq!(choose(&ranked, &only_xmr, "").as_deref(), Some("xmr"));

        // No data / no mineable coin → no decision.
        assert_eq!(choose(&[], &all, "xmr"), None);
        assert_eq!(choose(&ranked, &[], "xmr"), None);
    }

    #[test]
    fn pearl_share_of_network_matches_measured_economics() {
        // LuckyPool + CoinGecko figures probed 2026-07-29: $0.330 price, block
        // reward 247096813541 atomic / 1e8 units = ~2471 PRL, network 2.404e19 H/s,
        // ~252 s blocks. An RTX 4070 SUPER at ~128 TH/s should net ~$1.49/day —
        // the number the earnings readout will show.
        let reward = 247_096_813_541.0 / 100_000_000.0;
        let rate = share_of_network_score(0.330151, reward, 2.403912902828623e19, 251.69).unwrap();
        let per_day = (128e12 / 1000.0) * rate;
        assert!(
            per_day > 0.5 && per_day < 4.0,
            "expected ~$1.49/day at 128 TH/s, got ${per_day}"
        );

        // Monotonic and fail-safe, like the other scores.
        let base = share_of_network_score(0.33, reward, 2.4e19, 252.0).unwrap();
        assert!(share_of_network_score(0.33, reward, 4.8e19, 252.0).unwrap() < base);
        assert!(share_of_network_score(0.66, reward, 2.4e19, 252.0).unwrap() > base);
        assert!(share_of_network_score(0.0, reward, 2.4e19, 252.0).is_none());
        assert!(share_of_network_score(0.33, reward, 0.0, 252.0).is_none());
    }

    /// Locks the KawPow difficulty→hashrate conversion the runners rely on,
    /// against a live cross-check taken 2026-08-17: HeroMiners difficulty
    /// 10714.566 ↔ WhatToMine `nethash` 766,978,531,240 H/s (Ethash-family:
    /// raw hashes/block = difficulty × 2^32).
    #[test]
    fn ravencoin_hashrate_from_difficulty_matches_reference() {
        let difficulty = 10714.566_f64;
        let hashps = difficulty * 4_294_967_296.0 / RVN_BLOCK_SECS;
        let reference = 766_978_531_240.0;
        assert!(
            (hashps - reference).abs() / reference < 0.01,
            "RVN hashrate {hashps} drifted from reference {reference}"
        );
    }

    /// The RVN earnings model produces a plausible per-GPU number: at ~$0.0028/RVN,
    /// ~767 GH/s network, 1250 reward, 60 s blocks, a 20 MH/s consumer GPU should
    /// land in cents/day — not zero, not dollars. Guards a units slip.
    #[test]
    fn ravencoin_share_of_network_is_sane() {
        let rate = share_of_network_score(0.0028, RVN_BLOCK_REWARD, 7.67e11, RVN_BLOCK_SECS)
            .expect("finite inputs");
        let gpu_day = rate * 20_000.0; // 20 MH/s = 20_000 kH/s
        assert!(
            (0.02..1.0).contains(&gpu_day),
            "implausible RVN/day for a 20 MH/s GPU: ${gpu_day}"
        );
    }

    /// Locks the Autolykos difficulty→hashrate conversion, against the live
    /// cross-check taken 2026-08-17: HeroMiners difficulty 59,452,111,716,352 ↔
    /// WhatToMine `nethash` 503,831,455,223 H/s. Note the convention differs
    /// from KawPow's: NO 2^32 factor, plain difficulty / block_time.
    #[test]
    fn ergo_hashrate_from_difficulty_matches_reference() {
        let difficulty = 59_452_111_716_352.0_f64;
        let hashps = difficulty / ERG_BLOCK_SECS;
        let reference = 503_831_455_223.0;
        assert!(
            (hashps - reference).abs() / reference < 0.02,
            "ERG hashrate {hashps} drifted from reference {reference}"
        );
    }

    /// The ERG earnings model produces a plausible per-GPU number: at ~$0.20/ERG,
    /// ~500 GH/s network, 3 ERG reward, 120 s blocks, a 120 MH/s consumer GPU
    /// should land around a dime/day — not zero, not dollars. Guards a units slip.
    #[test]
    fn ergo_share_of_network_is_sane() {
        let rate = share_of_network_score(0.2027, ERG_BLOCK_REWARD, 5.04e11, ERG_BLOCK_SECS)
            .expect("finite inputs");
        let gpu_day = rate * 120_000.0; // 120 MH/s = 120_000 kH/s
        assert!(
            (0.03..0.5).contains(&gpu_day),
            "implausible ERG/day for a 120 MH/s GPU: ${gpu_day}"
        );
    }
}
