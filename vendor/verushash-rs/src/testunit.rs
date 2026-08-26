#[cfg(test)]
mod tests {
    use crate::*;
    use hex;

    const TEST_DATA: &[u8] = b"hello world";

    // NOTE: Some tests are skipped on windows since the Haraka alorithm c code combined with the Rust test harness will cause
    // a segfault at the end of the hashing process, altough the output is correct

    #[test]
    fn test_verus_hash_v1() {
        let h = verus_hash_v1(TEST_DATA);
        let h_display = hex::encode(h);

        assert_eq!(
            "071cf6db1bf047ee07e0fcc3f5a9c77d2580798ae19e3c4ef242e14a78378f40",
            h_display
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2() {
        let h = verus_hash_v2(TEST_DATA);
        let h_display = hex::encode(h);

        assert_eq!(
            "5bc0ba7e97fe14d19f527a803577dadeae3ea45b15a68b050ee308d581e096b1",
            h_display
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2_3x() {
        for _ in 0..3 {
            let h = verus_hash_v2(TEST_DATA);
            let h_display = hex::encode(h);

            assert_eq!(
                "5bc0ba7e97fe14d19f527a803577dadeae3ea45b15a68b050ee308d581e096b1",
                h_display
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2_1() {
        let h = verus_hash_v2_1(TEST_DATA);
        let h_display = hex::encode(h);

        assert_eq!(
            "6cae82cbef9b80afe08e2ceab0073f5db66b3f2f9c3ebca9e8f4e36f7cef4baf",
            h_display
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2_1_3x() {
        for _ in 0..3 {
            let h = verus_hash_v2_1(TEST_DATA);
            let h_display = hex::encode(h);

            assert_eq!(
                "6cae82cbef9b80afe08e2ceab0073f5db66b3f2f9c3ebca9e8f4e36f7cef4baf",
                h_display
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2_2() {
        let h = verus_hash_v2_2(TEST_DATA);
        let h_display = hex::encode(h);

        assert_eq!(
            "6cae82cbef9b80afe08e2ceab0073f5db66b3f2f9c3ebca9e8f4e36f7cef4baf",
            h_display
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2_2_3x() {
        for _ in 0..3 {
            let h = verus_hash_v2_2(TEST_DATA);
            let h_display = hex::encode(h);

            assert_eq!(
                "6cae82cbef9b80afe08e2ceab0073f5db66b3f2f9c3ebca9e8f4e36f7cef4baf",
                h_display
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_verus_hash_v2_2_1000x() {
        for _ in 0..1000 {
            let h = verus_hash_v2_2(TEST_DATA);
            let h_display = hex::encode(h);

            assert_eq!(
                "6cae82cbef9b80afe08e2ceab0073f5db66b3f2f9c3ebca9e8f4e36f7cef4baf",
                h_display
            );
        }
    }

    fn build_test_header() -> Vec<u8> {
        // Verus-Header-Layout (simplified):
        // version (4)
        // prev_block (32)
        // merkle_root (32)
        // sapling_root (32)
        // time (4)
        // bits (4)
        // nonce / solution placeholder (32)

        let version: u32 = 65540;
        let time: u32 = 1_700_000_000;
        let bits: u32 = 0x1b018ab8;

        let prev_block = [0u8; 32];
        let merkle_root = [1u8; 32];
        let sapling_root = [2u8; 32];
        let nonce = [3u8; 32];

        let mut header = Vec::with_capacity(140);
        header.extend_from_slice(&version.to_le_bytes());
        header.extend_from_slice(&prev_block);
        header.extend_from_slice(&merkle_root);
        header.extend_from_slice(&sapling_root);
        header.extend_from_slice(&time.to_le_bytes());
        header.extend_from_slice(&bits.to_le_bytes());
        header.extend_from_slice(&nonce);

        header
    }

    #[test]
    fn test_check_verus_header_dummy_length() {
        let mut header = build_test_header(); // 108-139

        let mut solution = vec![0xfd, 0x40, 0x05, 0x00];
        solution.resize(1347, 0u8); // Padding mit 0
        header.extend_from_slice(&solution);

        assert_eq!(header.len(), 1487);
    }

    #[test]
    #[cfg(not(windows))]
    fn verushash_v2_2_is_stable() {
        let header = build_test_header();

        let h1 = verus_hash_v2_2(&header);
        let h2 = verus_hash_v2_2(&header);

        assert_eq!(h1, h2);

        assert_ne!(h1, [0u8; 32]);
    }
}
