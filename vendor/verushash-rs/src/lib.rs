// (C) 2026 flopetautschnig (floscodes)
// Distributed under the MIT software license, see the accompanying
// file COPYING or http://www.opensource.org/licenses/mit-license.php.

mod testunit;
mod verushash;

pub fn verus_hash_v1(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        verushash::verus_v1_hash(out.as_mut_ptr(), data.as_ptr(), data.len());
    }
    out
}

pub fn verus_hash_v2(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        verushash::verus_v2_hash(out.as_mut_ptr(), data.as_ptr(), data.len());
    }
    out
}

pub fn verus_hash_v2_1(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        verushash::verus_v2_1_hash(out.as_mut_ptr(), data.as_ptr(), data.len());
    }
    out
}

pub fn verus_hash_v2_2(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        verushash::verus_v2_2_hash(out.as_mut_ptr(), data.as_ptr(), data.len());
    }
    out
}
