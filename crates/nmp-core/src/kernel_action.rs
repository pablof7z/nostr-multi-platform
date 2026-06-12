//! `KernelAction` reducer (T95).
//!
//! `KernelAction` / `KernelUpdate` are the generic, FFI-serializable boundary
//! defined in [`crate::app`]. `nmp-codegen` projects them into Swift/Kotlin by
//! name, but until T95 nothing on the Rust side consumed them — T80 shipped the
//! pure [`resolve_open_uri`] handler without a dispatcher.
//!
//! This module is that dispatcher: a single `match` over `KernelAction` with a
//! clean slot for every future variant. Only `OpenUri` needs a real arm today;
//! the remaining variants map to their trivially-correct `KernelUpdate` so the
//! seam is uniform without growing scope.
//!
//! # Placement (V-01 Phase 1c)
//!
//! Lives at the crate root (not under `actor/`) because the reducer is pure
//! over `&mut Kernel` — it never touches `ActorCommand`, the relay worker, or
//! any I/O. Keeping it outside the `actor` tree lets [`crate::kernel_reducer::KernelReducer`]
//! (a public reducer consumed by codegen-projected `FfiApp`s) compile without
//! the `native` feature, even when the actor runtime is gated out for wasm32
//! or other no-std-io targets.
//!
//! Doctrine:
//! - **D0** — operates only on the app-noun-free `KernelAction`/`KernelUpdate`
//!   primitives and the protocol-neutral `LogicalInterest`. No app vocabulary.
//! - **D6** — total function: never panics, never unwinds across FFI. Every
//!   failure path returns [`KernelUpdate::UriRejected`] with a stable,
//!   app-noun-free reason string.
//! - **D8** — no per-event allocation: this runs once per *action*, not per
//!   ingested event, and registers through the single-writer registry whose
//!   snapshot order stays deterministic.

use crate::app::{resolve_open_uri, KernelAction, KernelUpdate, OpenUriRouting};
use crate::kernel::Kernel;
use crate::planner::InterestScope;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};

/// Reduce one [`KernelAction`] against the kernel, returning the
/// [`KernelUpdate`] the host app should observe.
///
/// Total and panic-free (D6): the only fallible action (`OpenUri`) funnels its
/// typed error into [`KernelUpdate::UriRejected`].
pub(crate) fn dispatch_kernel_action(kernel: &mut Kernel, action: KernelAction) -> KernelUpdate {
    match action {
        KernelAction::OpenUri { uri } => open_uri(kernel, uri),

        // Lifecycle / view variants have no resolver yet — warn loudly so
        // callers detect the unwired seam (V-110). Real arms land as their
        // handlers are wired (one `match` arm each, no future-proofing).
        KernelAction::Start => KernelUpdate::Started { rev: 0 },
        KernelAction::Stop => KernelUpdate::Stopped { rev: 0 },
        KernelAction::OpenView { namespace, key } => {
            tracing::warn!(
                namespace = %namespace,
                key = %key,
                "OpenView has no resolver — interest not compiled; relay subscription was NOT opened"
            );
            KernelUpdate::ViewOpened { namespace, key }
        }
        KernelAction::CloseView { namespace, key } => {
            tracing::warn!(
                namespace = %namespace,
                key = %key,
                "CloseView has no resolver — view-lifecycle seam unwired"
            );
            KernelUpdate::ViewClosed { namespace, key }
        }
        KernelAction::RunDiagnostics => KernelUpdate::Diagnostics {
            summary: String::new(),
        },
    }
}

/// Resolve a `nostr:` URI and, on success, register the resolved interest
/// through the single-writer registry (D4) + surface the `ViewOpened` update.
/// On any failure emit [`KernelUpdate::UriRejected`] (D6 — no panic).
fn open_uri(kernel: &mut Kernel, uri: String) -> KernelUpdate {
    let OpenUriRouting { interest, view } = match resolve_open_uri(&uri) {
        Ok(routing) => routing,
        Err(err) => {
            return KernelUpdate::UriRejected {
                uri,
                reason: err.to_string(),
            }
        }
    };

    // Destructure the view rather than `match`ing its variants: `resolve_open_uri`
    // only ever yields `ViewOpened`, so a non-`ViewOpened` view is a defensive
    // rejection, never a panic (D6).
    let KernelUpdate::ViewOpened { namespace, key } = &view else {
        return KernelUpdate::UriRejected {
            uri,
            reason: "resolver produced a non-view update".to_string(),
        };
    };

    let scope = scope_of(&interest.scope);
    // Stable hashed identity for this opened target (no allocation in the key
    // itself; the strings are borrowed for hashing only).
    let sub_key = SubKey::builder("open-uri")
        .with(namespace.as_str())
        .with(key.as_str())
        .finish();
    let owner = SubOwnerKey::new(("open-uri-view", namespace.as_str(), key.as_str()));
    let identity = SubIdentity::new(owner, sub_key, scope);

    // ADR-0045 — route through the single ensure-install front door so opening
    // a `nostr:` URI whose target is already in the store serves those events
    // to parsers/projections (closes the F2 bypass: this path previously called
    // bare `ensure_sub` with neither a recompile trigger nor a store-cache
    // serve). `ensure_interest_and_serve` is idempotent register-if-absent —
    // re-opening the same URI attaches another owner without clobbering the
    // live filter (§3.3) and re-serving a completed shape is a no-op. The
    // newly-installed return is unused here; the side effect is what matters.
    let _ = kernel.ensure_interest_and_serve(identity, interest, "open-uri");

    view
}

/// Map a planner [`InterestScope`] onto the registry's [`SubScope`].
///
/// `ActiveAccount` is not resolvable to a concrete pubkey here (that happens at
/// compile time), so it shares the global slot space — identical to the
/// registry's own legacy scope bridge.
fn scope_of(scope: &InterestScope) -> SubScope {
    match scope {
        InterestScope::Account(pk) => SubScope::Account(pk.clone()),
        InterestScope::ActiveAccount | InterestScope::Global => SubScope::Global,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{VIEW_ADDRESSABLE, VIEW_PROFILE};
    use crate::nip19::{encode_naddr, encode_npub, encode_nsec, NaddrData};
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const PK2: &str = "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2";

    fn kernel() -> Kernel {
        Kernel::new(DEFAULT_VISIBLE_LIMIT)
    }

    #[test]
    fn open_uri_npub_registers_profile_interest_and_opens_profile_view() {
        let mut k = kernel();
        let npub = encode_npub(PK).unwrap();

        let update = dispatch_kernel_action(
            &mut k,
            KernelAction::OpenUri {
                uri: format!("nostr:{npub}"),
            },
        );

        assert_eq!(
            update,
            KernelUpdate::ViewOpened {
                namespace: VIEW_PROFILE.into(),
                key: PK.into(),
            }
        );

        let active = k.lifecycle_mut().registry_mut().iter_active();
        assert_eq!(active.len(), 1, "exactly one interest registered");
        let interest = &active[0];
        assert!(interest.shape.kinds.contains(&0), "kind:0 metadata filter");
        assert!(interest.shape.authors.contains(PK), "author pinned to npub");
        assert_eq!(
            interest.shape.limit,
            Some(3),
            "one event per kind (profile, contacts, relay list)"
        );
    }

    #[test]
    fn open_uri_nsec_is_rejected_not_routable_no_interest() {
        let mut k = kernel();
        let nsec = encode_nsec(PK).unwrap();

        let update = dispatch_kernel_action(
            &mut k,
            KernelAction::OpenUri {
                uri: format!("nostr:{nsec}"),
            },
        );

        match update {
            KernelUpdate::UriRejected { uri, reason } => {
                assert_eq!(uri, format!("nostr:{nsec}"));
                assert!(
                    reason.contains("not routable"),
                    "stable app-noun-free reason: {reason}"
                );
            }
            other => panic!("expected UriRejected, got {other:?}"),
        }
        assert!(
            k.lifecycle_mut().registry_mut().iter_active().is_empty(),
            "rejected URI must register no interest"
        );
    }

    #[test]
    fn open_uri_garbage_is_rejected_unparseable() {
        let mut k = kernel();
        let update = dispatch_kernel_action(
            &mut k,
            KernelAction::OpenUri {
                uri: "not-a-nostr-thing".into(),
            },
        );
        assert!(matches!(
            update,
            KernelUpdate::UriRejected { reason, .. } if reason.contains("unparseable")
        ));
        assert!(k.lifecycle_mut().registry_mut().iter_active().is_empty());
    }

    #[test]
    fn open_uri_naddr_with_relay_hint_sets_relay_pin() {
        let mut k = kernel();
        let naddr = encode_naddr(&NaddrData {
            identifier: "pinned".into(),
            pubkey: PK2.into(),
            kind: 30023,
            relays: vec!["wss://naddr.example".into()],
        })
        .unwrap();

        let update = dispatch_kernel_action(
            &mut k,
            KernelAction::OpenUri {
                uri: format!("nostr:{naddr}"),
            },
        );

        assert_eq!(
            update,
            KernelUpdate::ViewOpened {
                namespace: VIEW_ADDRESSABLE.into(),
                key: format!("30023:{PK2}:pinned"),
            }
        );
        let active = k.lifecycle_mut().registry_mut().iter_active();
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].shape.relay_pin.as_deref(),
            Some("wss://naddr.example"),
            "ADR-0012 relay_pin third routing lane set from the naddr hint"
        );
    }

    #[test]
    fn open_uri_same_target_twice_dedups_to_one_interest() {
        let mut k = kernel();
        let npub = encode_npub(PK).unwrap();
        let action = || KernelAction::OpenUri {
            uri: format!("nostr:{npub}"),
        };

        let first = dispatch_kernel_action(&mut k, action());
        let second = dispatch_kernel_action(&mut k, action());

        assert_eq!(first, second, "idempotent view-open update");
        assert_eq!(
            k.lifecycle_mut().registry_mut().iter_active().len(),
            1,
            "ensure_sub dedups: re-opening the same URI keeps one interest"
        );
    }

    #[test]
    fn non_open_uri_variants_echo_trivially_correct_updates() {
        let mut k = kernel();
        assert_eq!(
            dispatch_kernel_action(&mut k, KernelAction::Start),
            KernelUpdate::Started { rev: 0 }
        );
        assert_eq!(
            dispatch_kernel_action(&mut k, KernelAction::Stop),
            KernelUpdate::Stopped { rev: 0 }
        );
        assert_eq!(
            dispatch_kernel_action(
                &mut k,
                KernelAction::OpenView {
                    namespace: "n".into(),
                    key: "k".into()
                }
            ),
            KernelUpdate::ViewOpened {
                namespace: "n".into(),
                key: "k".into()
            }
        );
        // No interest registered by the placeholder arms (D8 / scope hygiene).
        assert!(k.lifecycle_mut().registry_mut().iter_active().is_empty());
    }
}
