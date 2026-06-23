//! Rung 1 — secret rejection. Asserts the input is never echoed into the
//! rejection, and that secret-reject precedes reference decoding.

use nmp_core::nip19::encode_nsec;
use nmp_core::substrate::{InputIntentRejection, InputScopeId};

use super::{classify_bare, expect_rejection, profiles_scope, req, SK};

#[test]
fn bare_nsec_is_rejected_without_echoing_input() {
    let nsec = encode_nsec(SK).unwrap();
    let r = req(&nsec, vec![profiles_scope()]);
    let rejection = expect_rejection(classify_bare(&r));
    assert_eq!(rejection, InputIntentRejection::SecretLike);
    // The rejection must carry NO copy of the secret. SecretLike is a unit
    // variant; assert its Debug never contains the input.
    let dbg = format!("{rejection:?}");
    assert!(!dbg.contains(&nsec), "rejection debug leaked the nsec");
    assert!(!dbg.contains(SK), "rejection debug leaked the secret hex");
}

#[test]
fn nostr_prefixed_nsec_is_rejected() {
    let nsec = encode_nsec(SK).unwrap();
    let r = req(&format!("nostr:{nsec}"), vec![profiles_scope()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::SecretLike
    );
}

#[test]
fn ncryptsec_is_rejected_by_prefix() {
    // A representative ncryptsec (NIP-49) bech32 string — only the HRP prefix is
    // load-bearing for the reject; the body need not decode.
    let r = req(
        "ncryptsec1qgg9947rlpvqu76pj5ecreduf9jxhdce8nxvjt5",
        vec![profiles_scope()],
    );
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::SecretLike
    );
}

#[test]
fn malformed_partial_nsec_is_rejected_without_echo() {
    // A typoed / mid-typing `nsec1…` is NOT a decodable NIP-19 entity. The
    // prefix-based detector must still reject it as SecretLike (#1882) so the
    // bech32 body never falls through to free-text and gets echoed.
    let garbage = "nsec1qpzry9x8gf2tvdw0sjufzfwxyzqqqqqqq0notvalid";
    let r = req(garbage, vec![profiles_scope()]);
    let c = classify_bare(&r);
    // No echo: the full classification (Debug + serialized) must contain NO
    // substring of the secret-bearing input — not the whole string, and not the
    // distinctive bech32 body tail.
    let dbg = format!("{c:?}");
    let json = serde_json::to_string(&c).unwrap();
    for rendered in [&dbg, &json] {
        assert!(!rendered.contains(garbage), "leaked the full input: {rendered}");
        assert!(
            !rendered.contains("0notvalid"),
            "leaked a substring of the secret body: {rendered}"
        );
    }
    assert_eq!(expect_rejection(c), InputIntentRejection::SecretLike);
}

#[test]
fn malformed_nostr_prefixed_nsec_is_rejected_without_echo() {
    let garbage = "nostr:nsec1xxgarbagebodythatdoesnotdecode99";
    let r = req(garbage, vec![profiles_scope()]);
    let c = classify_bare(&r);
    let dbg = format!("{c:?}");
    let json = serde_json::to_string(&c).unwrap();
    for rendered in [&dbg, &json] {
        assert!(!rendered.contains(garbage), "leaked the full input: {rendered}");
        assert!(
            !rendered.contains("garbagebody"),
            "leaked a substring of the secret body: {rendered}"
        );
    }
    assert_eq!(expect_rejection(c), InputIntentRejection::SecretLike);
}

#[test]
fn uppercase_nsec_prefix_is_rejected_case_insensitively() {
    // HRP matching is case-insensitive (including the `nostr:` scheme).
    let r = req("NOSTR:NSEC1typedinuppercase", vec![profiles_scope()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::SecretLike
    );
}

#[test]
fn secret_precedes_reference_decoding() {
    // An nsec is a valid bech32 the ref decoder would otherwise reject as
    // non-routable — assert rung 1 short-circuits first as SecretLike.
    let nsec = encode_nsec(SK).unwrap();
    let r = req(&nsec, vec![InputScopeId::nostr_ref()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::SecretLike
    );
}
