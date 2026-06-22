//! Effective-relay resolution for a [`SearchRequest`](crate::SearchRequest).
//!
//! `open_search` resolves [`SearchTargets`](crate::SearchTargets) into a
//! concrete relay set before opening the relay-pinned generic interest. The
//! actual kind:10007 (NIP-51 search-relays) read and the app-default fallback
//! are NOT owned here — they are supplied through the [`SearchRelaySource`]
//! seam, which the composition root (nmp-ffi) backs with the live
//! `nmp_nip51::SearchRelayListProjection` snapshot + the app-declared default
//! set. This keeps `nmp-nip50` free of any kind:10007 parsing (that lives in
//! `nmp-nip51`, D0) while still owning the orchestration policy of WHICH source
//! each `SearchTargets` variant draws from.

use std::sync::Arc;

use crate::SearchTargets;

/// The host registration seam for the default [`SearchRelaySource`].
///
/// The composition root (`nmp-defaults`) calls this once, during
/// `register_defaults`, to auto-wire the kind:10007 read seam + app-default
/// fallback onto the host — so a plain app that calls only `open_search(..,
/// UserPreferred)` transparently fans out to the user's published search relays
/// with ZERO app code. The concrete host (`NmpApp` in `nmp-ffi`) implements
/// this; `nmp-nip50` never names the host (D0 — this trait is the seam).
pub trait SearchRelaySourceRegistrar {
    /// Register the active [`SearchRelaySource`] on the host. Last-writer-wins.
    fn set_search_relay_source(&self, source: Arc<dyn SearchRelaySource + Send + Sync>);
}

/// The read seam `open_search` uses to resolve relays for `UserPreferred` and
/// `AppDefault` targets.
///
/// ## Seam for the sibling kind:10007 read helper (#1747 search-relays lane)
///
/// `user_preferred()` MUST return the active account's NIP-51 kind:10007 search
/// relays. The composition root backs this with
/// `nmp_nip51::SearchRelayListProjection::snapshot().relays`. When the sibling
/// lane lands a dedicated read helper (e.g. an `nmp-defaults` accessor that
/// falls back to the app default when kind:10007 is empty), swap the closure
/// body in the composition root — this trait does NOT change.
///
/// `app_default()` MUST return the app-declared default search relay set
/// (`nmp-defaults` / app policy). Operator policy is never owned by NMP crates,
/// so this is supplied by the host, not hard-coded here.
pub trait SearchRelaySource {
    /// Active-account preferred (kind:10007) search relays, canonicalized.
    /// Empty when no kind:10007 is known for the active account.
    fn user_preferred(&self) -> Vec<String>;

    /// App-declared default search relays. Empty when the app declared none.
    fn app_default(&self) -> Vec<String>;
}

/// Blanket impl so a host can pass a pair of closures without a named type.
impl<U, A> SearchRelaySource for (U, A)
where
    U: Fn() -> Vec<String>,
    A: Fn() -> Vec<String>,
{
    fn user_preferred(&self) -> Vec<String> {
        (self.0)()
    }
    fn app_default(&self) -> Vec<String> {
        (self.1)()
    }
}

/// Blanket impl over references (including `&dyn SearchRelaySource`), so a host
/// holding the source behind an `Arc<dyn SearchRelaySource>` can pass
/// `&*arc` / `arc.as_ref()` into [`resolve_search_relays`] without an extra
/// adapter type.
impl<T: SearchRelaySource + ?Sized> SearchRelaySource for &T {
    fn user_preferred(&self) -> Vec<String> {
        (**self).user_preferred()
    }
    fn app_default(&self) -> Vec<String> {
        (**self).app_default()
    }
}

/// Resolve a request's [`SearchTargets`] into the concrete relay set to pin the
/// search interest to.
///
/// * [`SearchTargets::Explicit`] — the caller-supplied list verbatim.
/// * [`SearchTargets::UserPreferred`] — kind:10007 search relays; when those
///   are empty, falls back to the app default (a user with no kind:10007 still
///   gets a working search).
/// * [`SearchTargets::AppDefault`] — the app default set.
///
/// The result is de-duplicated preserving first-seen order. Blocked-relay
/// subtraction is NOT applied here — it is the router's subtractive post-pass
/// over every routed interest (`BlockedRelaySet`,
/// `docs/architecture/crate-boundaries.md` §3.1), so a relay this function
/// returns that the user has blocked is dropped downstream by the same generic
/// mechanism that protects every other interest.
#[must_use]
pub fn resolve_search_relays(
    targets: &SearchTargets,
    source: &dyn SearchRelaySource,
) -> Vec<String> {
    let raw = match targets {
        SearchTargets::Explicit(list) => list.clone(),
        SearchTargets::UserPreferred => {
            let preferred = source.user_preferred();
            if preferred.is_empty() {
                source.app_default()
            } else {
                preferred
            }
        }
        SearchTargets::AppDefault => source.app_default(),
    };
    dedup_preserving_order(raw)
}

fn dedup_preserving_order(relays: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    relays
        .into_iter()
        .filter(|r| !r.is_empty() && seen.insert(r.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Source {
        preferred: Vec<String>,
        default: Vec<String>,
    }
    impl SearchRelaySource for Source {
        fn user_preferred(&self) -> Vec<String> {
            self.preferred.clone()
        }
        fn app_default(&self) -> Vec<String> {
            self.default.clone()
        }
    }

    fn src(preferred: &[&str], default: &[&str]) -> Source {
        Source {
            preferred: preferred.iter().map(|s| s.to_string()).collect(),
            default: default.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn explicit_targets_use_given_list_verbatim() {
        let resolved = resolve_search_relays(
            &SearchTargets::Explicit(vec![
                "wss://a.example/".to_string(),
                "wss://b.example/".to_string(),
            ]),
            &src(&["wss://ignored/"], &["wss://ignored2/"]),
        );
        assert_eq!(resolved, vec!["wss://a.example/", "wss://b.example/"]);
    }

    #[test]
    fn user_preferred_reads_kind_10007() {
        let resolved = resolve_search_relays(
            &SearchTargets::UserPreferred,
            &src(&["wss://search.nos.lol/"], &["wss://default/"]),
        );
        assert_eq!(resolved, vec!["wss://search.nos.lol/"]);
    }

    #[test]
    fn user_preferred_falls_back_to_app_default_when_empty() {
        let resolved = resolve_search_relays(
            &SearchTargets::UserPreferred,
            &src(&[], &["wss://default/"]),
        );
        assert_eq!(resolved, vec!["wss://default/"]);
    }

    #[test]
    fn app_default_targets_use_default_set() {
        let resolved = resolve_search_relays(
            &SearchTargets::AppDefault,
            &src(&["wss://preferred/"], &["wss://default/"]),
        );
        assert_eq!(resolved, vec!["wss://default/"]);
    }

    #[test]
    fn dedups_preserving_first_seen_order() {
        let resolved = resolve_search_relays(
            &SearchTargets::Explicit(vec![
                "wss://a/".to_string(),
                "wss://b/".to_string(),
                "wss://a/".to_string(),
                String::new(),
            ]),
            &src(&[], &[]),
        );
        assert_eq!(resolved, vec!["wss://a/", "wss://b/"]);
    }

    #[test]
    fn closure_pair_implements_source() {
        let resolved = resolve_search_relays(
            &SearchTargets::UserPreferred,
            &(
                || vec!["wss://from-closure/".to_string()],
                Vec::<String>::new,
            ),
        );
        assert_eq!(resolved, vec!["wss://from-closure/"]);
    }
}
