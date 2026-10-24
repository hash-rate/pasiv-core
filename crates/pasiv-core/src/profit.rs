// SPDX-License-Identifier: GPL-3.0-only
//! Earnings-estimate math — the pure half of the desktop's profit module (it
//! was the Auto / Max-Profit ranking while the RandomX family had three
//! members; Auto retired with the direct route in 0.7.0, the maths stayed).
//! The live fetchers (CoinGecko prices, pool network stats) live with each
//! runner; everything here is a deterministic function of their inputs,
//! which is exactly the part worth auditing: *how* Pasiv prices a coin, and
//! that it shows the user's take-home, never gross.
//!
//! A RandomX coin is priced as `price × block_reward ÷ network_difficulty`
//! per unit hashrate (block time cancels out). Coins on other PoWs (Pearl;
//! Verus, Ravencoin and Ergo before they retired) are priced by their share
//! of that coin's own network instead.

use serde::Serialize;

use crate::types::Coin;

/// Switch only when the leader beats the current coin by this factor —
/// hysteresis so a near-tie doesn't cost a warmup (a switch restarts the
/// miner: cold RandomX dataset, ~30–60 s at 0 H/s).
pub const SWITCH_MARGIN: f64 = 1.08;

/// A coin's live revenue estimate, for the UI's "what and why".
#[derive(Debug, Clone, Serialize)]
pub struct CoinScore {
    pub ticker: String,
    /// USD/day at 1 kH/s, **net of Pasiv's own fee** — the user's take-home,
    /// so the number the UI shows is the number the user actually earns (4%
    /// on every coin: [`crate::fee::fee_fraction`]). Comparable across coins
    /// on the same algorithm.
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
/// the one line that makes the estimate honest: it must never show revenue
/// the user doesn't receive. Charged fee and estimated fee come from the same
/// [`crate::fee::fee_fraction`], so they cannot disagree.
pub fn net_score(gross: f64, coin: Coin) -> f64 {
    gross * (1.0 - crate::fee::fee_fraction(coin))
}

/// USD/day at 1 kH/s for a coin priced by its **share of network hashrate** —
/// the model for coins that can't be RandomX-priced because they run a
/// different PoW (Pearl/pearlhash today; Verus, Ravencoin and Ergo before
/// they retired). Your share of the network at 1 kH/s is
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
        // Take-home = gross × (1 − 4%) on EVERY coin (one route, one rule).
        let gross = 100.0;
        for coin in [Coin::Xmr, Coin::Prl] {
            assert!((net_score(gross, coin) - 96.0).abs() < 1e-9, "{coin:?}");
        }
    }

    /// `choose` is pure ranking logic (hysteresis, "only what the user can be
    /// paid in"); with one RandomX coin on the roster it is exercised on
    /// synthetic tickers so the maths stays pinned.
    #[test]
    fn choose_picks_best_mineable_and_respects_hysteresis() {
        let ranked = vec![cs("a", 100.0), cs("xmr", 96.0), cs("b", 60.0)];
        let all = vec!["xmr".to_string(), "a".into(), "b".into()];

        // From nothing (start-up): pick the outright best mineable coin.
        assert_eq!(choose(&ranked, &all, "").as_deref(), Some("a"));

        // Already on the best → stay put.
        assert_eq!(choose(&ranked, &all, "a"), None);

        // On XMR (96) vs A (100): only ~4% better, under the 8% margin → stay.
        assert_eq!(choose(&ranked, &all, "xmr"), None);

        // On B (60) vs A (100): a clear win → switch.
        assert_eq!(choose(&ranked, &all, "b").as_deref(), Some("a"));

        // Only XMR has a payout set → never leaves XMR even though A scores more.
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
}
