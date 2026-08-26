#[link(name = "verushash", kind = "static")]
unsafe extern "C" {
    pub unsafe fn verus_v1_hash(result: *mut u8, data: *const u8, len: usize);
    pub unsafe fn verus_v2_hash(result: *mut u8, data: *const u8, len: usize);
    pub unsafe fn verus_v2_1_hash(result: *mut u8, data: *const u8, len: usize);
    pub unsafe fn verus_v2_2_hash(result: *mut u8, data: *const u8, len: usize);
}
