// SPDX-License-Identifier: GPL-3.0-only
//! Payout-address validation — extracted from the desktop app's config module.
//
// Every validator below checks prefix + length + alphabet, and none verifies a
// checksum, even though Monero, Zephyr, Salvium and Verus addresses all carry
// one. That is a deliberate, documented limit rather than an oversight: these
// run on every keystroke as paste-time feedback, and the pool's own
// `mining.authorize` is the authoritative check that rejects a bad address.
//
// The gap is real — a typo that happens to keep the right prefix and length
// passes here, and mining to an address nobody controls loses those shares.
// Closing it properly means Monero's block-based base58 decode plus keccak-256
// (a new dependency), and a validator that is *wrong* in the strict direction
// is worse than one that is loose: it would block legitimate users from mining
// at all. Worth doing with real-address test vectors, not in passing.

const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn is_base58(a: &str) -> bool {
    !a.is_empty() && a.chars().all(|c| BASE58.contains(c))
}

/// Paste-time validation: a standard (4…) or subaddress (8…) address at 95
/// chars, base58 throughout. **Integrated addresses (106 chars, 4…) are
/// deliberately NOT accepted** — see `is_integrated_xmr_address`.
pub fn is_valid_xmr_address(a: &str) -> bool {
    is_base58(a) && a.len() == 95 && (a.starts_with('4') || a.starts_with('8'))
}

/// A Monero *integrated* address: a standard address with an 8-byte payment ID
/// baked in, 106 base58 chars starting with `4`. We reject these for mining
/// payouts rather than accept them silently: Monero deprecated unencrypted
/// payment IDs (getmonero.org — subaddresses are the supported replacement), and
/// a pool that pays only standard/subaddresses would strip the ID and pay a
/// different account than the user expects. Detected here so `set_config` can
/// say *why* it was refused and point at a subaddress, not a blank "invalid".
pub fn is_integrated_xmr_address(a: &str) -> bool {
    is_base58(a) && a.len() == 106 && a.starts_with('4')
}

/// True when the input looks like an email / paymail / exchange-memo entry
/// rather than a wallet address (contains `@`). No mineable payout address in
/// the roster contains `@` — including Verus, whose stratum payouts go to a
/// transparent `R…` address, not a VerusID (`name@`, an identity, not a payout
/// target). Used to answer a paste with "mine to a wallet you control" instead
/// of a generic "invalid".
pub fn looks_like_paymail(a: &str) -> bool {
    a.contains('@')
}

/// Zephyr (CryptoNote fork): base58 main addresses render with a "ZEPHYR"
/// prefix, length ~101 (small tolerance). The pool is the authoritative check;
/// this is instant paste-time feedback. Verified live against a real address.
pub fn is_valid_zeph_address(a: &str) -> bool {
    is_base58(a) && a.starts_with("ZEPHYR") && (98..=104).contains(&a.len())
}

/// Salvium payout address — the post-fork **Carrot** form, `SC1…`, ~97–143 chars.
///
/// NOT `SaLv…`. This flip-flopped once, so the reasoning is worth pinning: the
/// Salvium One fork retired legacy CryptoNote `SaLv…` addresses, and the mining
/// pool enforces it — a `login` with a `SaLv…` address gets
/// `{"code":-1,"message":"Invalid address"}` from `de.salvium.herominers.com:1230`
/// (verified live). A Salvium wallet still *shows* a `SaLv…` address as the
/// "Primary account", which is what misled an earlier fix into accepting it — but
/// the wallet's Carrot (`SC1…`) address is the one that mines. The pool is the
/// authoritative check, and it says `SC1`. Accepting `SaLv…` only lets a user
/// start a miner the pool immediately rejects, so we reject it up front (with a
/// specific hint — see commands::set_config).
pub fn is_valid_sal_address(a: &str) -> bool {
    is_base58(a) && a.starts_with("SC1") && (97..=143).contains(&a.len())
}

/// True for a legacy Salvium `SaLv…` address — retired at the Salvium One fork
/// and rejected by the pool. Used only to give a helpful "use your Carrot
/// address" message instead of a blank "invalid".
pub fn is_legacy_sal_address(a: &str) -> bool {
    is_base58(a) && a.starts_with("SaLv") && (95..=110).contains(&a.len())
}

/// Verus (VRSC) transparent address: base58check, 'R' prefix (version byte 60 =
/// 'R…'), 34 characters — same shape as a Bitcoin/Zcash-t address. This is
/// paste-time feedback over the base58 alphabet + prefix + length; the pool's
/// `mining.authorize` is the authoritative check. (A full base58check-decode +
/// double-SHA256 checksum is deferred — the roster's other validators are all
/// prefix/length/alphabet too.)
///
/// R-address only, on purpose. Pasiv mines Verus to LuckPool, whose stratum
/// pays a transparent `R…` address (its own miner examples use one, e.g.
/// `RRZwoAcm5qiKVYnc2CM2LARypRuNs14sTJ`). A **VerusID** (`name@`) is an on-chain
/// identity, not a stratum payout target — a user who pastes one is caught by
/// `looks_like_paymail` and told to use their R-address instead.
pub fn is_valid_vrsc_address(a: &str) -> bool {
    is_base58(a) && a.starts_with('R') && a.len() == 34
}

/// Ravencoin (RVN) payout address. Bitcoin-family base58check with pubkey version
/// byte 60 (0x3C), which yields a single-'R' prefix and a 34-char string — the
/// SAME shape as Verus (VRSC also uses version 60), so this is byte-for-byte the
/// VRSC rule. Shape-only, like every validator here: the pool's authorize is the
/// authoritative check, and a VRSC address would also pass (both are 'R'/34/base58)
/// — the ticker and pool select the chain, not this function.
pub fn is_valid_rvn_address(a: &str) -> bool {
    is_base58(a) && a.starts_with('R') && a.len() == 34
}

/// Ergo (ERG) payout address — mainnet P2PK: base58, leading `9`, 51 chars
/// (network byte 0x00 + P2PK type 0x01 → base58 always starts '9'; e.g.
/// `9f4QF8AD1nQ3nJahQVkMj8hFSVVzVom77b52JU7EW71Zexg6N8v`). Shape-only, like
/// every validator here — the pool's authorize is the authoritative check.
pub fn is_valid_erg_address(a: &str) -> bool {
    is_base58(a) && a.starts_with('9') && a.len() == 51
}

/// Pearl (PRL) payout address — a bech32 string: hrp `prl`, separator `1`, then
/// a bech32-charset payload (e.g. `prl1pqea7hz…lwl2`, ~62 chars). Shape-only
/// validation — prefix + bech32 alphabet + a sane length — like every other
/// coin here; the pool's authorize is the authoritative check. Note bech32
/// excludes `1`, `b`, `i`, `o`, so the payload charset is distinct from base58.
///
/// Format taken from the OFFICIAL Pearl Research Labs implementation
/// (github.com/pearl-research-labs/pearl), NOT third-party "pearl wallet" sites —
/// several of those rank in search with contradictory address claims (XMSS vs
/// Taproot), some for a different chain. A real address from that wallet
/// (`prl1pqea7hz…`) matches this rule.
pub fn is_valid_prl_address(a: &str) -> bool {
    const BECH32: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    match a.strip_prefix("prl1") {
        Some(data) => {
            (50..=66).contains(&a.len())
                && !data.is_empty()
                && data.chars().all(|c| BECH32.contains(c))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearl_address_is_bech32_prl1() {
        // A real AlphaPool Pearl address (bech32: hrp "prl", then payload).
        let good = "prl1pqea7hz42566cckfmg70uyw43e7c67rtazrvy927ghjcx6txn5lzsjrlwl2";
        assert!(is_valid_prl_address(good));
        assert!(!is_valid_prl_address("prl1"), "empty payload");
        assert!(
            !is_valid_prl_address("xyz1pqea7hz42566cckfmg70uyw43e7c67rtazrvy927ghjcx6txn5"),
            "wrong hrp"
        );
        // bech32 excludes 1/b/i/o, so a 'b' in the payload is invalid even at the
        // right length — this is what makes it distinct from a base58 address.
        assert!(
            !is_valid_prl_address(&good.replacen('q', "b", 1)),
            "non-bech32 char"
        );
        // An XMR address must not pass as Pearl.
        assert!(!is_valid_prl_address(&format!("4{}", "A".repeat(94))));
    }

    #[test]
    fn address_validation() {
        let std_addr = format!("4{}", "A".repeat(94));
        let sub_addr = format!("8{}", "A".repeat(94));
        let integrated = format!("4{}", "A".repeat(105));
        assert!(is_valid_xmr_address(&std_addr));
        assert!(is_valid_xmr_address(&sub_addr));
        // Integrated addresses (106 chars) are NOT accepted for mining — the
        // pool would strip the deprecated payment ID and pay a different account.
        assert!(!is_valid_xmr_address(&integrated));
        assert!(is_integrated_xmr_address(&integrated));
        assert!(!is_integrated_xmr_address(&std_addr)); // a standard addr is not "integrated"
        assert!(!is_valid_xmr_address(""));
        assert!(!is_valid_xmr_address("4short"));
        assert!(!is_valid_xmr_address(&format!("9{}", "A".repeat(94)))); // bad prefix
        assert!(!is_valid_xmr_address(&format!("4{}0", "A".repeat(93)))); // 0 not in base58

        // Paymail / email / exchange entries are refused with guidance, and this
        // catches a VerusID (name@) too — the Verus pool pays R…, not an identity.
        assert!(looks_like_paymail("me@exchange.com"));
        assert!(looks_like_paymail("myid@"));
        assert!(!looks_like_paymail(&std_addr));
    }

    #[test]
    fn zeph_address_validation() {
        let valid = format!("ZEPHYR{}", "3".repeat(95)); // 6 + 95 = 101, base58
        assert!(is_valid_zeph_address(&valid));
        assert!(!is_valid_zeph_address(&format!("4{}", "A".repeat(94)))); // XMR-style
        assert!(!is_valid_zeph_address("ZEPHYRshort"));
        assert!(!is_valid_zeph_address(""));
        assert!(!is_valid_zeph_address(&format!("ZEPHYR{}", "0".repeat(95)))); // 0 not base58
                                                                               // ZEPH and XMR validators reject each other's addresses.
        assert!(!is_valid_xmr_address(&valid));
    }

    #[test]
    fn sal_address_validation() {
        // A real legacy Salvium address (a live wallet's "Primary account").
        // The pool REJECTS it as "Invalid address" (verified live), so the
        // validator must too — accepting it only starts a miner the pool refuses.
        let legacy = "SaLvdTRp1ivEQ5ZcY4UGxhK1fJVE7QanHWWxtkM7ZPDjL2JH8hxwWMFGd7eh3tvjUv7YVTyyqvXg22DvTYBV8Y5KZRk93G1UDVf";
        assert_eq!(legacy.len(), 99);
        assert!(
            !is_valid_sal_address(legacy),
            "legacy SaLv… must be rejected"
        );
        // …but recognised as legacy, so the UI can point the user at their Carrot address.
        assert!(is_legacy_sal_address(legacy));

        // Carrot (SC1…) is the accepted form.
        let carrot = format!("SC1{}", "3".repeat(97)); // 100 chars, within 97..=143
        assert!(is_valid_sal_address(&carrot));
        assert!(is_valid_sal_address(&format!("SC1{}", "a".repeat(94)))); // min 97
        assert!(is_valid_sal_address(&format!("SC1{}", "a".repeat(140)))); // max 143
        assert!(!is_legacy_sal_address(&carrot)); // SC1 is not "legacy"

        // Rejections: wrong length, wrong prefix, non-base58, empty.
        assert!(!is_valid_sal_address(&format!("SC1{}", "a".repeat(141)))); // too long
        assert!(!is_valid_sal_address("SC1short"));
        assert!(!is_valid_sal_address(""));
        assert!(!is_valid_sal_address(&format!("SC1{}", "0".repeat(97)))); // 0 not base58

        // SAL and the other coins reject each other's addresses.
        assert!(!is_valid_xmr_address(&carrot));
        assert!(!is_valid_zeph_address(&carrot));
        assert!(!is_valid_sal_address(&format!("4{}", "A".repeat(94)))); // XMR-style
    }

    #[test]
    fn vrsc_address_validation() {
        // A real VRSC address shape (the public test worker used to prove the
        // accepted share): 'R' prefix, 34 base58 chars.
        let valid = "RPPPm6dVbpx3L3yDRK1ktZ1VnDbBTtNMoy";
        assert_eq!(valid.len(), 34);
        assert!(is_valid_vrsc_address(valid));
        assert!(!is_valid_vrsc_address("")); // empty
        assert!(!is_valid_vrsc_address(&format!("R{}", "a".repeat(34)))); // 35 chars
        assert!(!is_valid_vrsc_address(&format!("R{}", "a".repeat(32)))); // 33 chars
        assert!(!is_valid_vrsc_address(&format!("X{}", "a".repeat(33)))); // wrong prefix
        assert!(!is_valid_vrsc_address(&format!("R{}", "0".repeat(33)))); // 0 not base58
                                                                          // VRSC and the RandomX-family validators reject each other.
        assert!(!is_valid_xmr_address(valid));
        assert!(!is_valid_vrsc_address(&format!("4{}", "A".repeat(94))));
    }

    #[test]
    fn erg_address_validation() {
        // Ergo mainnet P2PK: base58, '9' prefix, 51 chars.
        let valid = "9f4QF8AD1nQ3nJahQVkMj8hFSVVzVom77b52JU7EW71Zexg6N8v";
        assert_eq!(valid.len(), 51);
        assert!(is_valid_erg_address(valid));
        assert!(!is_valid_erg_address("")); // empty
        assert!(!is_valid_erg_address(&format!("9{}", "a".repeat(51)))); // 52 chars
        assert!(!is_valid_erg_address(&format!("9{}", "a".repeat(49)))); // 50 chars
        assert!(!is_valid_erg_address(&format!("8{}", "a".repeat(50)))); // wrong prefix
        assert!(!is_valid_erg_address(&format!("9{}", "0".repeat(50)))); // 0 not base58
                                                                         // ERG rejects every other roster shape.
        assert!(!is_valid_erg_address(&format!("4{}", "A".repeat(94)))); // XMR
        assert!(!is_valid_erg_address("RPPPm6dVbpx3L3yDRK1ktZ1VnDbBTtNM25")); // RVN/VRSC
        assert!(!is_valid_xmr_address(valid));
        assert!(!is_valid_rvn_address(valid));
    }

    #[test]
    fn rvn_address_validation() {
        // Ravencoin's address format is byte-identical to Verus (both Bitcoin-family
        // version 60 → 'R' prefix, 34 base58 chars), so a format-valid address is a
        // valid address for either. Base58 excludes 0/O/I/l.
        let valid = "RPPPm6dVbpx3L3yDRK1ktZ1VnDbBTtNM25";
        assert_eq!(valid.len(), 34);
        assert!(is_valid_rvn_address(valid));
        assert!(!is_valid_rvn_address("")); // empty
        assert!(!is_valid_rvn_address(&format!("R{}", "a".repeat(34)))); // 35 chars
        assert!(!is_valid_rvn_address(&format!("R{}", "a".repeat(32)))); // 33 chars
        assert!(!is_valid_rvn_address(&format!("X{}", "a".repeat(33)))); // wrong prefix
        assert!(!is_valid_rvn_address(&format!("R{}", "0".repeat(33)))); // 0 not base58
                                                                         // RVN rejects the RandomX-family shapes.
        assert!(!is_valid_rvn_address(&format!("4{}", "A".repeat(94))));
        assert!(!is_valid_xmr_address(valid));
        // Honest limit, documented on the validator: RVN and VRSC share the 'R'/34
        // base58 shape (both version 60), so each accepts the other. The ticker and
        // pool select the chain — this asserts the KNOWN overlap, so a future
        // "tighten it" change has to confront it rather than silently break VRSC.
        assert!(is_valid_vrsc_address(valid));
    }
}
