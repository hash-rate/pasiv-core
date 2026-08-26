// SPDX-License-Identifier: GPL-3.0-only
//! The $/day estimation formula — one function, shared by every surface, so
//! the number a desktop shows, the number a headless node reports, and the
//! number a phone totals can never be computed three different ways.

/// USD per day for a device hashing at `hashrate_hs` (H/s), given the coin's
/// live rate in USD/day per 1 kH/s. `None` when either input is missing or
/// non-positive — an estimate of `$0.00` would read as "earning nothing"
/// when the truth is "no data".
pub fn usd_per_day(hashrate_hs: f64, usd_per_day_per_kh: Option<f64>) -> Option<f64> {
    let rate = usd_per_day_per_kh?;
    if rate <= 0.0 || hashrate_hs <= 0.0 {
        return None;
    }
    Some((hashrate_hs / 1000.0) * rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_linearly_and_refuses_bad_inputs() {
        // 7,200 H/s at $0.03/day/kH/s = $0.216/day (a real desk measurement).
        let v = usd_per_day(7_200.0, Some(0.03)).unwrap();
        assert!((v - 0.216).abs() < 1e-9);
        // Double the hashrate, double the money.
        assert_eq!(
            usd_per_day(14_400.0, Some(0.03)).unwrap(),
            2.0 * usd_per_day(7_200.0, Some(0.03)).unwrap()
        );
        // No rate / zero rate / zero hashrate → no estimate, never $0.00.
        assert!(usd_per_day(7_200.0, None).is_none());
        assert!(usd_per_day(7_200.0, Some(0.0)).is_none());
        assert!(usd_per_day(0.0, Some(0.03)).is_none());
    }
}
