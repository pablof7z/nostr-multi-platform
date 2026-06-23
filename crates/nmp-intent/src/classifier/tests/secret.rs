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
