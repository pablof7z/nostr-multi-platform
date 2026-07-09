//! Shared facade input guard for profile-ref keys (split out of `lib.rs` for
//! file-size discipline).
//!
//! Hoisted from the deleted `nmp-uniffi` reference facade (#2763): the
//! profile-key input guard every ref-resolution facade method needs before
//! handing a caller-supplied string to `RefNamespace::Profile` handling.

/// Validate that `key` is a well-formed 64-char lowercase-hex Nostr pubkey.
///
/// Facades must reject malformed profile-ref keys (wrong length, non-hex
/// characters, bech32 `npub…` forms, empty strings) before treating a
/// caller-supplied string as a hex pubkey for `RefNamespace::Profile`
/// resolution. This is the one input-guard behavior the deleted
/// `nmp-uniffi` reference facade had that real app-owned facades lacked;
/// it now lives here so every facade built over this crate gets it for
/// free instead of drifting per-app.
#[must_use]
pub fn is_hex_pubkey(key: &str) -> bool {
    nmp_core::__ffi_internal::is_hex_pubkey(key)
}

#[cfg(test)]
mod tests {
    use super::is_hex_pubkey;

    #[test]
    fn accepts_well_formed_hex_pubkey() {
        let valid = "a".repeat(64);
        assert!(is_hex_pubkey(&valid));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(!is_hex_pubkey(&"a".repeat(63)));
        assert!(!is_hex_pubkey(&"a".repeat(65)));
    }

    #[test]
    fn rejects_non_hex_characters() {
        let mut s = "a".repeat(63);
        s.push('z');
        assert!(!is_hex_pubkey(&s));
    }

    #[test]
    fn rejects_bech32_npub_form() {
        assert!(!is_hex_pubkey(
            "npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq"
        ));
    }

    #[test]
    fn rejects_empty_string() {
        assert!(!is_hex_pubkey(""));
    }
}
