//! Shared fixtures for the retention-audit suite: a deterministic hex-pubkey
//! builder and a collapsed `resolve_ref` call at the feed-avatar shape (the
//! only shape the per-pubkey retention tests exercise).

use crate::kernel::{Kernel, ProfileShape, RefLiveness, RefNamespace, RefShape};
use crate::relay::OutboundMessage;

/// Construct a 64-char hex pubkey from a small index. Deterministic, valid by
/// `is_hex_pubkey`. Matches the harness's `test_pubkeys` shape.
pub(super) fn deterministic_pubkey(idx: u32) -> String {
    let mut hex = String::with_capacity(64);
    for _ in 0..56 {
        hex.push('0');
    }
    hex.push_str(&format!("{idx:08x}"));
    hex
}

/// Resolve a profile reference at the feed-avatar shape (`Profile`/`Card`,
/// `CacheOk`, no force, no hints) — the only shape these per-pubkey retention
/// tests exercise. Collapses the 8-arg `resolve_ref` call so each claim site
/// stays one line. Returns the kernel's outbound vec unchanged.
pub(super) fn resolve_profile_card(
    kernel: &mut Kernel,
    pubkey: &str,
    consumer_id: impl Into<String>,
) -> Vec<OutboundMessage> {
    kernel.resolve_ref(
        RefNamespace::Profile,
        pubkey.to_string(),
        consumer_id.into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
        false,
        Vec::new(),
    )
}
