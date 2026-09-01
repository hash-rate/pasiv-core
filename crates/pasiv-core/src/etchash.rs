// SPDX-License-Identifier: GPL-3.0-only
//! Etchash (Ethereum Classic) DAG sizing — the one number the ETC lane's
//! pre-flight gate turns on.
//!
//! **Why this cannot be a constant.** Every other GPU coin in the roster gates on
//! a fixed `min_vram_mb`, because KawPow's and autolykos2's memory needs are
//! effectively flat for a consumer card. Etchash's is not: the DAG grows 8 MiB
//! every epoch, forever, and a card that mines ETC today stops being able to at
//! a predictable future date. A hardcoded number would be silently wrong within
//! months — it would either refuse cards that still work, or (worse) start a
//! miner that allocates and dies.
//!
//! Measured 2026-09-02 against the live chain at height 25,262,741: epoch 421,
//! DAG 4,605,344,896 bytes (4.289 GiB), growing ~323 MiB/year.
//!
//! **ECIP-1099 is the trap.** Ethereum's epoch is 30,000 blocks. ETC halved its
//! DAG growth at block 11,700,000 by *doubling* the epoch length to 60,000 —
//! so any formula copied from an Ethash implementation computes roughly double
//! the correct epoch for ETC, and therefore a DAG about 1.7 GiB too large. That
//! would refuse every 6 GB card for no reason. `epoch_for_height` encodes the
//! fork rather than assuming either constant.

/// ECIP-1099 activation height on ETC mainnet — the "Thanos" hardfork.
pub const ECIP1099_HEIGHT: u64 = 11_700_000;
/// Epoch length before ECIP-1099 (the Ethash inheritance).
pub const EPOCH_LEN_LEGACY: u64 = 30_000;
/// Epoch length after ECIP-1099 — doubled, which halves DAG growth per block.
pub const EPOCH_LEN: u64 = 60_000;

const DATASET_BYTES_INIT: u64 = 1 << 30; // 1 GiB at epoch 0
const DATASET_BYTES_GROWTH: u64 = 1 << 23; // +8 MiB per epoch
const MIX_BYTES: u64 = 128;

/// The epoch a block height falls in, honouring ECIP-1099.
///
/// Before the fork the chain used 30k-block epochs and had already reached
/// epoch 390; after it, epochs are 60k blocks and the *epoch number itself* was
/// halved so the DAG shrank rather than jumped. Post-fork, `height / 60_000`
/// gives the right answer directly.
pub fn epoch_for_height(height: u64) -> u64 {
    if height >= ECIP1099_HEIGHT {
        height / EPOCH_LEN
    } else {
        height / EPOCH_LEN_LEGACY
    }
}

/// Whether `n` is prime. Trial division is correct and instant at the sizes
/// involved (the DAG search tests a few hundred candidates around 3.6e7).
fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    if n.is_multiple_of(2) {
        return n == 2;
    }
    let mut i: u64 = 3;
    while i.saturating_mul(i) <= n {
        if n.is_multiple_of(i) {
            return false;
        }
        i += 2;
    }
    true
}

/// DAG size in bytes for an epoch.
///
/// Start from init + growth·epoch, drop one mix, then walk *down* in 2·MIX_BYTES
/// steps until the size divided by MIX_BYTES is prime. The prime constraint is
/// part of the spec, not an optimisation: it is what makes the dataset's page
/// addressing distribute evenly.
pub fn dag_bytes_for_epoch(epoch: u64) -> u64 {
    let mut size = DATASET_BYTES_INIT + DATASET_BYTES_GROWTH * epoch - MIX_BYTES;
    while !is_prime(size / MIX_BYTES) {
        size -= 2 * MIX_BYTES;
    }
    size
}

/// DAG size in bytes at a given chain height.
pub fn dag_bytes_at_height(height: u64) -> u64 {
    dag_bytes_for_epoch(epoch_for_height(height))
}

/// Headroom over the DAG a card needs to actually mine.
///
/// The DAG is the big allocation but not the only one: the miner also holds the
/// light cache, per-thread scratch and the OS/compositor framebuffer. 512 MiB is
/// deliberately generous — the failure we are avoiding is a card that passes the
/// gate, spends a minute building a DAG, and then dies on allocation, which
/// reads to the user as "Pasiv is broken" rather than "this card is too small".
pub const HEADROOM_MIB: u64 = 512;

/// Minimum free VRAM (MiB) to mine ETC at this height — what the pre-flight
/// compares a card against, and the number the refusal message quotes.
pub fn required_vram_mib(height: u64) -> u64 {
    dag_bytes_at_height(height).div_ceil(1024 * 1024) + HEADROOM_MIB
}

/// The user-facing refusal, in GB because that is how cards are sold.
///
/// Deliberately states BOTH numbers. "Your card is too small" invites the user
/// to wonder by how much; "needs 4.6 GB, this card has 4.0 GB" is a fact they
/// can act on — and when the DAG later grows past their card, the same sentence
/// explains why something that used to work stopped.
pub fn vram_refusal(height: u64, card_vram_mib: u64) -> String {
    let need_gb = required_vram_mib(height) as f64 * 1024.0 * 1024.0 / 1e9;
    let have_gb = card_vram_mib as f64 * 1024.0 * 1024.0 / 1e9;
    format!("ETC needs {need_gb:.1} GB free on your GPU; this card has {have_gb:.1} GB.")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned against the live chain on the spike date. If this ever fails, the
    /// formula changed — not the chain, which only moves the height.
    #[test]
    fn matches_the_live_chain_on_the_spike_date() {
        // 2026-09-02, 2Miners' own /api/stats reported this height.
        let height = 25_262_741;
        assert_eq!(epoch_for_height(height), 421);
        assert_eq!(dag_bytes_at_height(height), 4_605_344_896);
        // 4.289 GiB + 512 MiB headroom
        assert_eq!(required_vram_mib(height), 4904);
    }

    #[test]
    fn ecip1099_is_honoured_not_assumed() {
        // One block before the fork: 30k epochs.
        assert_eq!(epoch_for_height(ECIP1099_HEIGHT - 1), 389);
        // At the fork: 60k epochs, and the epoch number HALVES rather than
        // continuing to climb — the DAG shrank at Thanos, it did not jump.
        assert_eq!(epoch_for_height(ECIP1099_HEIGHT), 195);
        assert!(dag_bytes_at_height(ECIP1099_HEIGHT) < dag_bytes_at_height(ECIP1099_HEIGHT - 1));

        // The bug this guards: using Ethereum's 30k epoch on a modern ETC height
        // computes ~2x the epoch and a DAG ~1.7 GiB too large, which would
        // refuse every 6 GB card.
        let h = 25_262_741;
        let wrong = dag_bytes_for_epoch(h / EPOCH_LEN_LEGACY);
        let right = dag_bytes_at_height(h);
        assert!(
            wrong > right + 1_500_000_000,
            "the 30k/60k mix-up must be visible"
        );
    }

    #[test]
    fn dag_grows_and_stays_prime_indexed() {
        let a = dag_bytes_for_epoch(421);
        let b = dag_bytes_for_epoch(422);
        assert!(b > a, "the DAG only ever grows within an era");
        // ~8 MiB per epoch, allowing for the prime walk-down.
        assert!((b - a) < 9 * 1024 * 1024);
        for e in [0, 1, 195, 421, 500] {
            assert!(is_prime(dag_bytes_for_epoch(e) / MIX_BYTES), "epoch {e}");
        }
    }

    #[test]
    fn epoch_zero_is_the_one_gib_floor() {
        assert!(dag_bytes_for_epoch(0) <= DATASET_BYTES_INIT);
        assert!(dag_bytes_for_epoch(0) > DATASET_BYTES_INIT - 100_000);
    }

    #[test]
    fn the_refusal_names_both_numbers() {
        // A 4 GB card at the spike height — the brief's worked example.
        let msg = vram_refusal(25_262_741, 4096);
        assert!(msg.contains("5.1 GB"), "{msg}");
        assert!(msg.contains("4.3 GB"), "{msg}");
        // Never a bare "unsupported": the user must be able to act on it.
        assert!(msg.contains("this card has"), "{msg}");
    }

    #[test]
    fn a_6gb_card_passes_today_and_an_8gb_card_has_years() {
        let h = 25_262_741;
        let need = required_vram_mib(h);
        assert!(
            need < 6144,
            "a 6 GB card must still qualify today: need {need} MiB"
        );
        assert!(need < 8188, "the lab's RTX 4060 (8188 MiB) must qualify");
        // And the honest limit: 6 GB cards do not have forever.
        let years_for_6gb = (6144 - need) as f64 / 323.0;
        assert!(
            years_for_6gb < 5.0,
            "6 GB has a finite runway, and we should know it"
        );
    }
}
