//! Shared fixtures for the state-projection suite: fixed 64-char hex
//! pubkeys/ids and the `make_update` snapshot driver.

use crate::kernel::Kernel;

// 64-char hex pubkeys / ids — the kernel's `is_hex_pubkey` / `is_hex_id`
// gates require exactly 64 ascii hex digits.
pub(super) const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const FOLLOW_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const FOLLOW_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// Drive `make_update` and parse the emitted JSON snapshot.
pub(super) fn snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let json = kernel.make_update_json_for_test(true);
    serde_json::from_str(&json).expect("kernel snapshot must be valid JSON")
}
