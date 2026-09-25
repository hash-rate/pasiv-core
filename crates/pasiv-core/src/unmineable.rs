// SPDX-License-Identifier: GPL-3.0-only
//! unMineable — the single user-facing payout route (decided 2026-09-25).
//!
//! The rig mines an ALGORITHM (PearlPow on the GPU, RandomX on the CPU) and
//! unMineable pays the user a chosen ASSET (USDT by default) straight to the
//! user's own address, once a day, from a small threshold. Pasiv never holds
//! funds: the pool pays the address the user entered (never-list item 7).
//!
//! Facts this module relies on, checked against unMineable's public API and
//! docs on 2026-09-25 (re-check before changing any of them):
//! - Login is `ASSET:address.worker#referral`; `?ref=` may replace `#`.
//! - A fresh `0x…` address under USDT defaults to BSC (BEP20), threshold
//!   1.5 USDT; ERC20's is 28 USDT, so BSC is the only EVM network we offer.
//! - An address cannot be changed once mining has started — validation here
//!   is the last line of defence.
//! - Stratum: `pearlpow.unmineable.com` and `rx.unmineable.com`, ports 3333
//!   (plain) and 443 (TLS).

use crate::address::{
    is_valid_bsc_address, is_valid_prl_address, is_valid_tron_address, is_valid_xmr_address,
};

/// What unMineable pays the user in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayoutAsset {
    /// USDT on BNB Smart Chain — the default, and what "Connect wallet" fills.
    UsdtBsc,
    /// USDT on TRON — the common exchange-deposit network.
    UsdtTron,
    /// Paid in the mined coin itself, for crypto-native users.
    Prl,
    Xmr,
}

impl PayoutAsset {
    /// unMineable's asset symbol (the network is inferred from the address
    /// shape: `0x…` → BSC by default, `T…` → TRON).
    pub fn symbol(self) -> &'static str {
        match self {
            PayoutAsset::UsdtBsc | PayoutAsset::UsdtTron => "USDT",
            PayoutAsset::Prl => "PRL",
            PayoutAsset::Xmr => "XMR",
        }
    }

    pub fn validate(self, address: &str) -> bool {
        match self {
            PayoutAsset::UsdtBsc => is_valid_bsc_address(address),
            PayoutAsset::UsdtTron => is_valid_tron_address(address),
            PayoutAsset::Prl => is_valid_prl_address(address),
            PayoutAsset::Xmr => is_valid_xmr_address(address),
        }
    }

    /// Which USDT network an address belongs to, if it is a USDT address.
    pub fn usdt_for(address: &str) -> Option<PayoutAsset> {
        if is_valid_bsc_address(address) {
            Some(PayoutAsset::UsdtBsc)
        } else if is_valid_tron_address(address) {
            Some(PayoutAsset::UsdtTron)
        } else {
            None
        }
    }
}

/// What the rig mines on unMineable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    PearlPow,
    RandomX,
}

impl Algo {
    pub fn host(self) -> &'static str {
        match self {
            Algo::PearlPow => "pearlpow.unmineable.com",
            Algo::RandomX => "rx.unmineable.com",
        }
    }
    pub const PORT: u16 = 3333;
    pub const TLS_PORT: u16 = 443;
}

/// A worker name unMineable accepts: letters, digits and `_` only (its docs:
/// "spaces and special characters are not supported, the mining process will
/// fail"). Anything else becomes `_`; empty or all-invalid falls back to
/// `pasiv`. Capped at 32 characters.
pub fn worker_name(raw: &str) -> String {
    let w: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(32)
        .collect();
    if w.chars().all(|c| c == '_') {
        "pasiv".into()
    } else {
        w
    }
}

/// The stratum login: `ASSET:address.worker#referral`. `referral` is
/// omitted when empty. Returns None for an address the asset rejects, so a
/// bad address can never reach the pool.
pub fn login(asset: PayoutAsset, address: &str, worker: &str, referral: &str) -> Option<String> {
    let address = address.trim();
    if !asset.validate(address) {
        return None;
    }
    let mut s = format!("{}:{}.{}", asset.symbol(), address, worker_name(worker));
    if !referral.is_empty() {
        s.push('#');
        s.push_str(referral);
    }
    Some(s)
}

/// unMineable's public stats page for an address — the "verify on the pool"
/// trust anchor, like every other coin's dashboard link. Only built for an
/// address the asset accepts.
pub fn stats_url(asset: PayoutAsset, address: &str) -> Option<String> {
    let address = address.trim();
    asset.validate(address).then(|| {
        format!(
            "https://unmineable.com/address/{}?coin={}",
            address,
            asset.symbol()
        )
    })
}

/// unMineable's public API for an address's balance and payout threshold.
pub fn address_api_url(asset: PayoutAsset, address: &str) -> Option<String> {
    let address = address.trim();
    asset.validate(address).then(|| {
        format!(
            "https://api.unminable.com/v4/address/{}?coin={}",
            address,
            asset.symbol()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // USDT's own TRC20 contract address — a real, checksummed TRON address.
    const TRON: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
    const BSC: &str = "0x000000000000000000000000000000000000dEaD";

    #[test]
    fn tron_checksum_is_enforced() {
        assert!(is_valid_tron_address(TRON));
        // One character changed: shape still fine, checksum must fail.
        let typo = TRON.replacen('7', "8", 1);
        assert!(!is_valid_tron_address(&typo));
        assert!(!is_valid_tron_address("T123"));
        assert!(!is_valid_tron_address(BSC));
    }

    #[test]
    fn usdt_network_follows_the_address() {
        assert_eq!(PayoutAsset::usdt_for(BSC), Some(PayoutAsset::UsdtBsc));
        assert_eq!(PayoutAsset::usdt_for(TRON), Some(PayoutAsset::UsdtTron));
        assert_eq!(PayoutAsset::usdt_for("prl1pqea7hz"), None);
    }

    #[test]
    fn login_has_unmineables_shape() {
        assert_eq!(
            login(PayoutAsset::UsdtBsc, BSC, "Simon's Rig 1", "abcd-1234").as_deref(),
            Some("USDT:0x000000000000000000000000000000000000dEaD.Simon_s_Rig_1#abcd-1234")
        );
        assert_eq!(
            login(PayoutAsset::UsdtTron, TRON, "", "").as_deref(),
            Some("USDT:TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t.pasiv")
        );
    }

    #[test]
    fn a_bad_address_never_reaches_the_pool() {
        assert_eq!(login(PayoutAsset::UsdtBsc, "0x1234", "w", ""), None);
        assert_eq!(login(PayoutAsset::UsdtTron, BSC, "w", ""), None); // wrong network
    }

    #[test]
    fn stats_links_only_for_valid_addresses() {
        assert_eq!(
            stats_url(PayoutAsset::UsdtBsc, BSC).as_deref(),
            Some("https://unmineable.com/address/0x000000000000000000000000000000000000dEaD?coin=USDT")
        );
        assert_eq!(stats_url(PayoutAsset::UsdtBsc, "nope"), None);
        assert!(address_api_url(PayoutAsset::UsdtTron, TRON)
            .unwrap()
            .starts_with("https://api.unminable.com/v4/address/T"));
    }

    #[test]
    fn worker_names_are_sanitised() {
        assert_eq!(worker_name("DESKTOP-KGQIA43"), "DESKTOP_KGQIA43");
        assert_eq!(worker_name("---"), "pasiv");
        assert_eq!(worker_name(&"x".repeat(50)).len(), 32);
    }
}
