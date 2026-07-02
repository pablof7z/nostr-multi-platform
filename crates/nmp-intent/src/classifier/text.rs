//! Rung 6 — free-text → `nmp_nip50::SearchRequest` bridge (issue #1804).
//!
//! Builds the built-in NIP-50 `TextQuery` target for a requested `nip50.*`
//! scope: maps the scope `name` → [`SearchScope`], translates the noun-free
//! [`TextSearchTargets`] → [`SearchTargets`], builds a bounded [`SearchRequest`],
//! and serializes it into the opaque `request_json` (core never imports
//! nmp-nip50). Pure — no IO.

use nmp_core::substrate::{InputIntentTarget, InputScopeId, TextSearchTargets};
use nmp_nip50::{
    SearchRequest, SearchScope, SearchTargets, SCOPE_NAME_LONGFORM, SCOPE_NAME_NOTES,
    SCOPE_NAME_PROFILES,
};

use super::NIP50_NAMESPACE;

/// NIP-01 short-text-note kind — the `nip50.notes` search scope's kind.
const KIND_SHORT_TEXT_NOTE: u32 = 1;

/// Build the built-in NIP-50 `TextQuery` target for a requested `nip50.*` scope.
///
/// Returns `None` for a non-`nip50` namespace, an unknown `nip50.*` name, or a
/// query the bounded-query rule rejects (e.g. all-whitespace).
pub(super) fn builtin_text_query(
    scope: &InputScopeId,
    query: &str,
    targets: &TextSearchTargets,
) -> Option<InputIntentTarget> {
    if scope.namespace != NIP50_NAMESPACE {
        return None;
    }
    let search_scope = search_scope_for(&scope.name)?;
    let request = SearchRequest::new(query, search_scope, to_search_targets(targets), None)?;
    let request_json = serde_json::to_string(&request).ok()?;
    Some(InputIntentTarget::TextQuery { request_json })
}

/// True iff `scope` is a `nip50.*` scope the built-in text fall-through serves.
pub(super) fn serves_builtin_scope(scope: &InputScopeId) -> bool {
    scope.namespace == NIP50_NAMESPACE && search_scope_name_known(&scope.name)
}

fn search_scope_for(name: &str) -> Option<SearchScope> {
    Some(match name {
        SCOPE_NAME_PROFILES => SearchScope::Users,
        SCOPE_NAME_LONGFORM => SearchScope::LongForm,
        SCOPE_NAME_NOTES => {
            SearchScope::Kinds(std::collections::BTreeSet::from([KIND_SHORT_TEXT_NOTE]))
        }
        _ => return None,
    })
}

fn search_scope_name_known(name: &str) -> bool {
    matches!(
        name,
        SCOPE_NAME_PROFILES | SCOPE_NAME_NOTES | SCOPE_NAME_LONGFORM
    )
}

/// Translate the noun-free [`TextSearchTargets`] into the NIP-50
/// [`SearchTargets`].
fn to_search_targets(targets: &TextSearchTargets) -> SearchTargets {
    match targets {
        TextSearchTargets::UserPreferred => SearchTargets::UserPreferred,
        TextSearchTargets::AppDefault => SearchTargets::AppDefault,
        TextSearchTargets::Explicit(relays) => SearchTargets::Explicit(relays.clone()),
    }
}
