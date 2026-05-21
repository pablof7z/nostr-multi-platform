//! Tests for `SignerError` — the canonical error type.
//!
//! The variants drive user-facing diagnostics (`toast: Option<String>` at the
//! FFI boundary, per doctrine D6). Each variant must Display without panic and
//! must compare equal/non-equal as expected — the kernel matches on variants
//! to decide retry / surface / drop behaviour.

use nmp_signer_iface::SignerError;

fn all_variants() -> Vec<SignerError> {
    vec![
        SignerError::NotReady("not connected".into()),
        SignerError::Unsupported("nip44 unavailable".into()),
        SignerError::Rejected("user denied".into()),
        SignerError::Mismatch("pubkey drift".into()),
        SignerError::Timeout("5s".into()),
        SignerError::SignatureVerificationFailed("bad sig".into()),
        SignerError::Backend("relay io".into()),
    ]
}

#[test]
fn display_renders_non_empty_string_for_every_variant() {
    for v in all_variants() {
        let s = v.to_string();
        assert!(!s.is_empty(), "Display must be non-empty for {v:?}");
    }
}

#[test]
fn display_includes_inner_message_for_every_variant() {
    // Each variant's Display impl must include its inner message so the FFI
    // toast surfaces the actual reason — a leak of the variant name only
    // would be useless to the user.
    assert!(SignerError::NotReady("MARKER-A".into()).to_string().contains("MARKER-A"));
    assert!(SignerError::Unsupported("MARKER-B".into()).to_string().contains("MARKER-B"));
    assert!(SignerError::Rejected("MARKER-C".into()).to_string().contains("MARKER-C"));
    assert!(SignerError::Mismatch("MARKER-D".into()).to_string().contains("MARKER-D"));
    assert!(SignerError::Timeout("MARKER-E".into()).to_string().contains("MARKER-E"));
    assert!(
        SignerError::SignatureVerificationFailed("MARKER-F".into())
            .to_string()
            .contains("MARKER-F")
    );
    assert!(SignerError::Backend("MARKER-G".into()).to_string().contains("MARKER-G"));
}

#[test]
fn equality_distinguishes_variants_and_messages() {
    assert_eq!(
        SignerError::Timeout("5s".into()),
        SignerError::Timeout("5s".into()),
        "same variant + same message must compare equal"
    );
    assert_ne!(
        SignerError::Timeout("5s".into()),
        SignerError::Timeout("10s".into()),
        "same variant + different message must differ"
    );
    assert_ne!(
        SignerError::Timeout("5s".into()),
        SignerError::Backend("5s".into()),
        "different variants must never compare equal"
    );
}

#[test]
fn signer_error_implements_std_error() {
    // Confirm the trait bound is met — production code coerces these into
    // `Box<dyn std::error::Error>` for cross-layer error chains.
    fn _accepts<E: std::error::Error>(_: &E) {}
    _accepts(&SignerError::Backend("x".into()));
}
