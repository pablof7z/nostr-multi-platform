//! Input-intent resolver (issue #1804) — the higher-order orchestrator.
//!
//! One untyped input `String` from a single-box / paste / search field is
//! classified — purely and synchronously — into exactly one of:
//!
//! * a NIP-19/21 direct reference to open (`npub…`, `nostr:nevent…`),
//! * a relay URL (`ws://` / `wss://`),
//! * a NIP-05-shaped identifier (`name@domain`, SHAPE only — no HTTP here),
//! * a registered recognizer's target (opaque JSON),
//! * a free-text NIP-50 search request,
//!
//! or refused (secret-like, unparseable, unregistered scope, disallowed scope).
//!
//! # Layering
//!
//! The recognizer trait + registry are noun-free and live in
//! [`nmp_core::substrate::intent`]. This crate owns the **orchestrator**
//! ([`classify`]) and the **generic parsers** (NIP-19/21 ref decode via
//! [`nmp_core::resolve_open_uri`]'s decoder, relay-URL normalization, NIP-05
//! shape detection, free-text → `nmp_nip50::SearchRequest`).
//!
//! # Purity
//!
//! [`classify`] is PURE + SYNC + side-effect-free: zero IO. All IO (the NIP-05
//! `.well-known/nostr.json` reverse lookup, search REQs) happens only in the
//! dispatch layer (the FFI / actor), which routes each
//! [`nmp_core::substrate::InputIntentTarget`] this returns.
//!
//! # Frozen precedence (do not reorder — issue #1804)
//!
//! 1. secret-reject (`nsec` / `nostr:nsec` / `ncryptsec`) → `Rejection(SecretLike)`,
//! 2. NIP-19/21 reference via `resolve_open_uri`'s decoder → `DirectRef`,
//! 3. relay URL (`ws`/`wss`, normalized) → `RelayUrl`,
//! 4. NIP-05 shape (`name@domain`; SHAPE only) → `Nip05`,
//! 5. registered recognizers (in registration order),
//! 6. free text → `text_candidate` → `nmp_nip50::SearchRequest` → `TextQuery`,
//! 7. refusals (`DisallowedScope` / `UnregisteredScope` / `Unparseable`).

use std::sync::Arc;

use nmp_core::substrate::{
    InputIntentClassification, InputIntentRequest, InputScopeRecognizer,
};

pub mod classifier;
pub mod relay_url;

/// Classify one input request against the registered recognizers.
///
/// PURE + SYNC + side-effect-free (zero IO). The `recognizers` slice is a
/// snapshot taken by the dispatch layer from
/// [`nmp_core::substrate::InputScopeRegistry::recognizers`].
///
/// Returns the frozen-precedence [`InputIntentClassification`] (§ crate docs):
/// either one or more [`nmp_core::substrate::InputIntentCandidate`]s for the
/// caller to act on / disambiguate, or a single
/// [`nmp_core::substrate::InputIntentRejection`].
#[must_use]
pub fn classify(
    req: &InputIntentRequest,
    recognizers: &[Arc<dyn InputScopeRecognizer>],
) -> InputIntentClassification {
    classifier::classify_impl(req, recognizers)
}
