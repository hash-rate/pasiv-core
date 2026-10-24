// SPDX-License-Identifier: GPL-3.0-only
//! unMineable — Pasiv's ONE payout route (the default from 0.5.0, decided
//! 2026-09-25; the only route from 0.7.0, decided 2026-10-24, when the legacy
//! direct-pool route and its XMR-only fee retired).
//!
//! The rig mines an ALGORITHM (PearlPow on the GPU, RandomX on the CPU) and
//! unMineable pays the user a chosen ASSET straight to the user's own
//! address, once a day, from a small threshold. The asset is either
//! account-wide — USDT (the default) or BTC, one address for every lane — or
//! native per lane: the GPU lane paid in PRL to a Pearl address, the CPU lane
//! in XMR to a Monero address ("Advanced: native coins"). A lane with no
//! native address falls back to the account-wide address. Pasiv never holds
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
    is_valid_bsc_address, is_valid_btc_address, is_valid_prl_address, is_valid_tron_address,
    is_valid_xmr_address,
};
use crate::types::Coin;

/// What unMineable pays the user in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayoutAsset {
    /// USDT on BNB Smart Chain — the default, and what "Connect wallet" fills.
    UsdtBsc,
    /// USDT on TRON — the common exchange-deposit network.
    UsdtTron,
    /// Bitcoin, paid to the user's own address. The slow-but-holdable
    /// counterpart to USDT: USDT is the cash-like route (1.5 USDT threshold,
    /// paid daily), BTC the store-of-value one. Its payout threshold is far
    /// higher — 0.00075 BTC, read live per address from the pool, never
    /// hard-coded here — so a caller offering this must show the user what
    /// that means for THEIR hashrate before they commit.
    Btc,
    /// Paid in the mined coin itself ("Advanced: native coins", 0.7.0): PRL
    /// to a Pearl address for the GPU lane, XMR to a Monero address for the
    /// CPU lane. Chosen per LANE — see [`PayoutAsset::native_for`]. Live
    /// thresholds on 2026-10-24: 1 PRL and 0.03 XMR, paid daily; read live
    /// per address (`address_api_url`), never hard-coded.
    Prl,
    Xmr,
}

impl PayoutAsset {
    /// unMineable's asset symbol (the network is inferred from the address
    /// shape: `0x…` → BSC by default, `T…` → TRON).
    pub fn symbol(self) -> &'static str {
        match self {
            PayoutAsset::UsdtBsc | PayoutAsset::UsdtTron => "USDT",
            PayoutAsset::Btc => "BTC",
            PayoutAsset::Prl => "PRL",
            PayoutAsset::Xmr => "XMR",
        }
    }

    pub fn validate(self, address: &str) -> bool {
        match self {
            PayoutAsset::UsdtBsc => is_valid_bsc_address(address),
            PayoutAsset::UsdtTron => is_valid_tron_address(address),
            PayoutAsset::Btc => is_valid_btc_address(address),
            PayoutAsset::Prl => is_valid_prl_address(address),
            PayoutAsset::Xmr => is_valid_xmr_address(address),
        }
    }

    /// Which ACCOUNT-WIDE payout asset an address is for, by its shape alone:
    /// USDT on either network, or BTC — the one address every lane falls back
    /// to. The shapes cannot collide (`0x…` BSC, `T…` TRON, `1…`/`3…`/`bc1…`
    /// BTC), and each arm checksum-verifies, so a wrong-chain address is
    /// rejected rather than silently re-homed.
    ///
    /// `Xmr` and `Prl` are deliberately absent: a native address is chosen
    /// per lane, keyed by the coin the lane mines (`native_for`), never
    /// inferred from a shape in the account-wide field — so a Monero address
    /// pasted where USDT belongs is refused, not quietly re-routed.
    pub fn infer(address: &str) -> Option<PayoutAsset> {
        Self::usdt_for(address)
            .or_else(|| is_valid_btc_address(address).then_some(PayoutAsset::Btc))
    }

    /// The native payout asset for a lane mining `coin` — the coin itself,
    /// paid by unMineable to an address of that chain: `Prl` for Pearl,
    /// `Xmr` for the RandomX family (Monero; the retired Zephyr/Salvium
    /// variants map here too, since a RandomX lane is paid in XMR). `None`
    /// for a coin unMineable does not pay natively, so a caller can never
    /// build a native login for it.
    pub fn native_for(coin: Coin) -> Option<PayoutAsset> {
        match coin {
            Coin::Prl => Some(PayoutAsset::Prl),
            Coin::Xmr | Coin::Zeph | Coin::Sal => Some(PayoutAsset::Xmr),
            Coin::Vrsc | Coin::Rvn | Coin::Erg | Coin::Etc => None,
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

/// unMineable pays an address automatically (once a day at 13:00 UTC, over
/// the threshold) ONLY when that address's "auto pay" setting is on — and it
/// is OFF for every new address. Off, the owner must press "Payout now" on
/// unmineable.com each time, which breaks "paid daily from 1.5 USDT". The
/// setting is per address, keyed by the address's unMineable id (the `uuid` in
/// the address API, assigned once the address has mined), and is the same
/// unauthenticated call unmineable.com's own address page makes.
///
/// POST this URL with `{"setting": true}`. `None` for anything that isn't a
/// UUID, so no caller can splice a path into it.
pub fn auto_pay_url(uuid: &str) -> Option<String> {
    let u = uuid.trim();
    let shape = u.len() == 36
        && u.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    shape.then(|| format!("https://api.unmineable.com/v1/address/{u}/setting/auto_pay"))
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
        assert_eq!(PayoutAsset::usdt_for("prl1pxyz"), None);
    }

    #[test]
    fn login_has_unmineables_shape() {
        assert_eq!(
            login(PayoutAsset::UsdtBsc, BSC, "Office Rig 1", "abcd-1234").as_deref(),
            Some("USDT:0x000000000000000000000000000000000000dEaD.Office_Rig_1#abcd-1234")
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

    /// BTC as the second selectable payout asset (user's call, 2026-10-14:
    /// USDT for cash-like spending, BTC for holding). Every mainnet address
    /// form is accepted — see `address::is_valid_btc_address` for why.
    #[test]
    fn btc_is_a_payout_asset_in_every_mainnet_address_form() {
        const P2PKH: &str = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
        const P2SH: &str = "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy";
        const BECH32: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

        assert_eq!(PayoutAsset::Btc.symbol(), "BTC");
        for a in [P2PKH, P2SH, BECH32] {
            assert!(PayoutAsset::Btc.validate(a), "{a}");
        }
        // A bad checksum must never reach the pool: the address can't be
        // changed once mining starts.
        assert!(!PayoutAsset::Btc.validate("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb"));

        // The login carries unMineable's BTC symbol, the sanitised worker and
        // the referral, in that order.
        assert_eq!(
            login(PayoutAsset::Btc, P2SH, "DESKTOP-1", "pasiv").as_deref(),
            Some("BTC:3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy.DESKTOP_1#pasiv")
        );
        assert_eq!(login(PayoutAsset::Btc, "not-an-address", "w", ""), None);

        // The pool links address BTC, not USDT.
        assert!(stats_url(PayoutAsset::Btc, BECH32)
            .unwrap()
            .ends_with("?coin=BTC"));
    }

    /// Shape-inference must map each account-wide asset to its own chain and
    /// never a native coin: a Monero address in the account-wide field is
    /// refused, not re-routed (native payouts are keyed by LANE, below).
    #[test]
    fn infer_maps_each_account_wide_shape_and_never_a_native_coin() {
        assert_eq!(PayoutAsset::infer(BSC), Some(PayoutAsset::UsdtBsc));
        assert_eq!(PayoutAsset::infer(TRON), Some(PayoutAsset::UsdtTron));
        assert_eq!(
            PayoutAsset::infer("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
            Some(PayoutAsset::Btc)
        );
        assert_eq!(
            PayoutAsset::infer("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"),
            Some(PayoutAsset::Btc)
        );
        // A Monero address is NOT an account-wide choice (it is a per-lane one).
        let xmr = format!("4{}", "A".repeat(94));
        assert_eq!(PayoutAsset::infer(&xmr), None);
        assert_eq!(PayoutAsset::infer("nonsense"), None);
        // usdt_for is unchanged: BTC is not a USDT address.
        assert_eq!(
            PayoutAsset::usdt_for("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
            None
        );
    }

    /// "Advanced: native coins" (0.7.0): each lane's coin names its native
    /// asset, the login carries that asset's symbol and the coin's own
    /// address, and a wrong-chain address never reaches the pool.
    #[test]
    fn native_payout_is_per_lane_in_the_lanes_own_coin() {
        const PRL: &str = "prl1p7avmp9fu3gfkx64629q050q9d9snvwd07avmp9fu3gfkx64629q050q9d9";
        let xmr = format!("4{}", "A".repeat(94));
        let sub = format!("8{}", "B".repeat(94));

        assert_eq!(PayoutAsset::native_for(Coin::Prl), Some(PayoutAsset::Prl));
        assert_eq!(PayoutAsset::native_for(Coin::Xmr), Some(PayoutAsset::Xmr));
        // The retired RandomX coins still name XMR (a RandomX lane is paid in
        // XMR); the GPU coins SRBMiner mined, and ETC, have no native payout.
        for c in [Coin::Zeph, Coin::Sal] {
            assert_eq!(PayoutAsset::native_for(c), Some(PayoutAsset::Xmr));
        }
        for c in [Coin::Vrsc, Coin::Rvn, Coin::Erg, Coin::Etc] {
            assert_eq!(PayoutAsset::native_for(c), None, "{c:?}");
        }

        // GPU lane: PRL:<prl>.<worker>#ref on pearlpow; CPU lane: XMR:<xmr>…
        assert_eq!(
            login(PayoutAsset::Prl, PRL, "rig-1", "drp0-8auk").as_deref(),
            Some(format!("PRL:{PRL}.rig_1#drp0-8auk").as_str())
        );
        assert_eq!(
            login(PayoutAsset::Xmr, &xmr, "rig-1", "drp0-8auk").as_deref(),
            Some(format!("XMR:{xmr}.rig_1#drp0-8auk").as_str())
        );
        assert!(
            login(PayoutAsset::Xmr, &sub, "w", "").is_some(),
            "subaddress"
        );
        // Wrong chain for the lane: refused, never re-homed.
        assert_eq!(login(PayoutAsset::Prl, &xmr, "w", ""), None);
        assert_eq!(login(PayoutAsset::Xmr, PRL, "w", ""), None);
        assert_eq!(login(PayoutAsset::Xmr, BSC, "w", ""), None);
        // An integrated Monero address (106 chars) is refused — the pool
        // strips its payment id.
        assert_eq!(
            login(PayoutAsset::Xmr, &format!("4{}", "A".repeat(105)), "w", ""),
            None
        );
        // Each (asset, address) is its own pool account: two links.
        assert!(stats_url(PayoutAsset::Prl, PRL)
            .unwrap()
            .ends_with("?coin=PRL"));
        assert!(stats_url(PayoutAsset::Xmr, &xmr)
            .unwrap()
            .ends_with("?coin=XMR"));
        assert!(address_api_url(PayoutAsset::Prl, PRL)
            .unwrap()
            .ends_with("?coin=PRL"));
    }

    #[test]
    fn worker_names_are_sanitised() {
        assert_eq!(worker_name("DESKTOP-AB12CDE"), "DESKTOP_AB12CDE");
        assert_eq!(worker_name("---"), "pasiv");
        assert_eq!(worker_name(&"x".repeat(50)).len(), 32);
    }

    #[test]
    fn auto_pay_url_takes_only_a_uuid() {
        assert_eq!(
            auto_pay_url("655f9ac0-87d1-4515-b002-fc0304bc8cd6").as_deref(),
            Some("https://api.unmineable.com/v1/address/655f9ac0-87d1-4515-b002-fc0304bc8cd6/setting/auto_pay")
        );
        assert!(auto_pay_url("").is_none());
        assert!(auto_pay_url("655f9ac0-87d1-4515-b002-fc0304bc8cd").is_none());
        assert!(auto_pay_url("../../v1/payments?x=1-aaaa-bbbb-cccc-dddddddddddd").is_none());
        assert!(auto_pay_url("655f9ac0x87d1-4515-b002-fc0304bc8cd6").is_none());
    }
}
