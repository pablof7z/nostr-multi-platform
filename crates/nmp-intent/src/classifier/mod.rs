//! The classification pipeline (issue #1804) — the frozen-precedence cascade and
//! the small per-rung modules it drives.
//!
//! Everything here is PURE + SYNC + side-effect-free (zero IO). The only IO the
//! resolver implies — the NIP-05 `.well-known/nostr.json` reverse lookup and the
//! search REQs — happens later, in the dispatch layer, once it routes the
//! [`InputIntentTarget`] this returns.
//!
//! # Frozen precedence (do not reorder — issue #1804)
//!
//! 1. secret-reject (`nsec` / `nostr:nsec` / `ncryptsec`) → `Rejection(SecretLike)`,
//! 2. NIP-19/21 reference via [`nmp_core::resolve_open_uri`] → `DirectRef`
//!    (subject to the ref entity-class allow-list → `DisallowedScope`),
//! 3. relay URL (`ws`/`wss`, normalized) → `RelayUrl`,
//! 4. NIP-05 shape (`name@domain`; SHAPE only, no HTTP) → `Nip05`,
//! 5. registered recognizers (in registration order) → `Registered`,
//! 6. free text → each text-capable scope's `text_candidate` → `TextQuery`,
//! 7. refusals (`UnregisteredScope` / `Unparseable`).

use std::sync::Arc;

use nmp_core::substrate::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentRequest,
    InputIntentTarget, InputScopeId, InputScopeRecognizer, ResolvedInput, ResolvedInputKind,
};

use crate::relay_url::recognize_relay_url;

mod ad;
mod nip05;
mod reference;
mod secret;
mod text;

/// The NIP-50 namespace whose scopes the built-in text fall-through serves.
const NIP50_NAMESPACE: &str = "nip50";

/// Run the full precedence cascade for one request. See module docs for the
/// frozen order. PURE + SYNC + IO-free.
#[must_use]
pub fn classify_impl(
    req: &InputIntentRequest,
    recognizers: &[Arc<dyn InputScopeRecognizer>],
) -> InputIntentClassification {
    let trimmed = req.input.trim();

    // (1) secret-reject — before any other inspection. The rejection never
    //     carries the input (never logged / stored / echoed).
    if secret::is_secret_like(trimmed) {
        return reject(InputIntentRejection::SecretLike);
    }

    // (2) NIP-19/21 reference — gated by the app's scope allow-list on its class.
    if let Some(entity_class) = reference::reference_entity_class(trimmed) {
        return reference::classify_reference(trimmed, entity_class, &req.scopes);
    }

    // (3) relay URL (`ws://` / `wss://`), normalized through the single authority.
    if let Some(url) = recognize_relay_url(trimmed) {
        return one(
            InputScopeId::nostr_ref(),
            InputIntentTarget::RelayUrl { url },
        );
    }

    // (4) NIP-05 shape (`name@domain` / `_@domain`) — SHAPE only, no HTTP.
    if let Some(identifier) = nip05::nip05_shape(trimmed) {
        return one(
            InputScopeId::nostr_ref(),
            InputIntentTarget::Nip05 { identifier },
        );
    }

    // (4.5) NIP-AD candidate (#2927) — an `http(s)://` URL that may double as a
    //       pointer to Nostr events. SHAPE only, no HTTP. Emitted ALONGSIDE the
    //       free-text candidates for the same input (D1): the app attempts the
    //       AD `.well-known` resolution and the free-text search in parallel,
    //       never blocking on the fetch. Unlike rungs 2–4 this does NOT
    //       short-circuit — the URL stays searchable as text if it turns out
    //       not to be AD-enabled (the overwhelmingly common case).
    if let Some(url) = ad::ad_candidate_url(trimmed) {
        let mut candidates = vec![InputIntentCandidate {
            scope: InputScopeId::nostr_ref(),
            target: InputIntentTarget::AdCandidate { url },
        }];
        candidates.extend(free_text_candidates(trimmed, req, recognizers));
        return InputIntentClassification::Candidates(candidates);
    }

    // Build the generic-parse output once for the recognizer passes (5)/(6).
    // Nothing structural matched above, so this is `FreeText`. (Refs / relay /
    // nip05 are claimed by the framework's own rungs 2–4 before any registered
    // recognizer sees them — registered recognizers extend the *free-text* and
    // app-specific space, not the frozen structural classes.)
    let resolved = ResolvedInput {
        raw: trimmed.to_string(),
        kind: ResolvedInputKind::FreeText {
            text: trimmed.to_string(),
        },
    };

    // (5) registered recognizers — only those whose scope the app requested.
    //     Registration order is preserved (the registry snapshot is ordered).
    for recognizer in recognizers {
        let scope = recognizer.scope();
        if !req.scopes.contains(&scope) {
            continue;
        }
        if let Some(target) = recognizer.recognize(&resolved) {
            return one(scope, target);
        }
    }

    // (6) free text → each requested, text-capable recognizer's `text_candidate`,
    //     plus the implicit built-in NIP-50 fall-through for requested `nip50.*`
    //     scopes that no registered recognizer answered.
    if !trimmed.is_empty() {
        let candidates = free_text_candidates(trimmed, req, recognizers);
        if !candidates.is_empty() {
            return InputIntentClassification::Candidates(candidates);
        }
    }

    // (7) refusals.
    if let Some(scope) = first_unregistered_scope(req, recognizers) {
        return reject(InputIntentRejection::UnregisteredScope { scope });
    }
    reject(InputIntentRejection::Unparseable)
}

/// Collect the free-text candidates: one per requested text-capable recognizer
/// that answers, plus the built-in NIP-50 fall-through for any requested
/// `nip50.*` scope no recognizer answered.
fn free_text_candidates(
    trimmed: &str,
    req: &InputIntentRequest,
    recognizers: &[Arc<dyn InputScopeRecognizer>],
) -> Vec<InputIntentCandidate> {
    let mut candidates = Vec::new();
    for recognizer in recognizers {
        let scope = recognizer.scope();
        if !req.scopes.contains(&scope) {
            continue;
        }
        if let Some(target) = recognizer.text_candidate(trimmed, &req.text_targets) {
            candidates.push(InputIntentCandidate { scope, target });
        }
    }
    for scope in &req.scopes {
        if candidates.iter().any(|c| &c.scope == scope) {
            continue;
        }
        if let Some(target) = text::builtin_text_query(scope, trimmed, &req.text_targets) {
            candidates.push(InputIntentCandidate {
                scope: scope.clone(),
                target,
            });
        }
    }
    candidates
}

/// The first requested scope that has no registered recognizer AND is not a
/// built-in `nip50.*` scope (which the resolver serves itself) AND is not the
/// synthetic ref scope. `None` when every requested scope is serviceable.
fn first_unregistered_scope(
    req: &InputIntentRequest,
    recognizers: &[Arc<dyn InputScopeRecognizer>],
) -> Option<InputScopeId> {
    req.scopes
        .iter()
        .find(|scope| {
            if **scope == InputScopeId::nostr_ref() {
                return false;
            }
            if text::serves_builtin_scope(scope) {
                return false;
            }
            !recognizers.iter().any(|r| &r.scope() == *scope)
        })
        .cloned()
}

// ─── small constructors (shared with the rung modules) ───────────────────────

fn one(scope: InputScopeId, target: InputIntentTarget) -> InputIntentClassification {
    InputIntentClassification::Candidates(vec![InputIntentCandidate { scope, target }])
}

fn reject(rejection: InputIntentRejection) -> InputIntentClassification {
    InputIntentClassification::Rejection(rejection)
}

#[cfg(test)]
mod tests;
