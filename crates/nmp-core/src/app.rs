//! App-facing kernel command/update surface.
//!
//! These enums are the generic, FFI-serializable boundary between the host
//! app and the kernel. They carry only protocol-neutral primitives — no app
//! nouns (D0) — and never panic across the FFI seam (D6): every fallible path
//! returns a typed `Result` that the caller maps to a `KernelUpdate`.
//!
//! `nmp-codegen` generates the Swift/Kotlin `AppAction` / `AppUpdate` enums
//! from `KernelAction` / `KernelUpdate` by name, so every variant must remain
//! `Serialize`/`Deserialize` and free of crate-internal types.

use serde::{Deserialize, Serialize};

use crate::nip19;
use crate::nip21::{self, NostrUri};
use crate::planner::{
    HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
    NaddrCoord, RelayHint,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelAction {
    Start,
    Stop,
    OpenView {
        namespace: String,
        key: String,
    },
    CloseView {
        namespace: String,
        key: String,
    },
    RunDiagnostics,
    /// Open whatever a `nostr:` URI (or bare NIP-19 entity) points at.
    ///
    /// The handler ([`resolve_open_uri`]) decodes the entity and dispatches to
    /// the correct logical interest + view: `npub`/`nprofile` → profile,
    /// `note`/`nevent` → thread, `naddr` → addressable-event. Relay hints
    /// carried by the entity are honoured as the `relay_pin` third routing
    /// lane (ADR-0012).
    OpenUri {
        uri: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelUpdate {
    Started {
        rev: u64,
    },
    Stopped {
        rev: u64,
    },
    ViewOpened {
        namespace: String,
        key: String,
    },
    ViewClosed {
        namespace: String,
        key: String,
    },
    Diagnostics {
        summary: String,
    },
    /// A `nostr:` URI could not be resolved into a view. Carries a stable,
    /// app-noun-free reason string for diagnostics/telemetry.
    UriRejected {
        uri: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelViewSpec {
    Diagnostics,
}

// ─── OpenUri routing ─────────────────────────────────────────────────────────

/// View namespace for `npub` / `nprofile` targets.
pub const VIEW_PROFILE: &str = "profile";
/// View namespace for `note` / `nevent` targets.
pub const VIEW_THREAD: &str = "thread";
/// View namespace for `naddr` (addressable / parameterised-replaceable) targets.
pub const VIEW_ADDRESSABLE: &str = "addressable";

/// Why an `OpenUri` action could not be resolved. App-noun-free (D0) and
/// FFI-safe (D6): the kernel converts this into [`KernelUpdate::UriRejected`]
/// rather than ever unwinding across the boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenUriError {
    /// The string is neither a `nostr:` URI nor a bare NIP-19 entity.
    Unparseable(String),
    /// The entity decoded but is not routable to a view (e.g. `nsec`).
    NotRoutable(String),
}

impl std::fmt::Display for OpenUriError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable(m) => write!(f, "unparseable nostr URI: {m}"),
            Self::NotRoutable(m) => write!(f, "entity not routable to a view: {m}"),
        }
    }
}

impl std::error::Error for OpenUriError {}

/// The fully-resolved routing outcome of a `KernelAction::OpenUri`.
///
/// Carries both halves the task requires: the [`LogicalInterest`] to register
/// with the planner registry, and the [`KernelUpdate::ViewOpened`] the host
/// app renders. Relay hints have already been folded into the interest's
/// `relay_pin` (ADR-0012) and `hints` list.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenUriRouting {
    /// The interest to push into the subscription registry.
    pub interest: LogicalInterest,
    /// The `ViewOpened` update describing which view to surface.
    pub view: KernelUpdate,
}

/// Parse the input either as a `nostr:` URI (NIP-21) or, failing the scheme
/// check, as a bare NIP-19 entity. Bare `nsec` is rejected exactly like the
/// `nostr:nsec…` case so the two entry forms behave identically.
fn parse_target(input: &str) -> Result<NostrUri, OpenUriError> {
    match nip21::parse_nostr_uri(input) {
        Ok(target) => Ok(target),
        Err(nip21::Nip21Error::MissingScheme) => match nip19::parse(input) {
            Ok(entity) => bare_entity_to_target(entity),
            Err(e) => Err(OpenUriError::Unparseable(e.to_string())),
        },
        Err(nip21::Nip21Error::NsecForbidden) => {
            Err(OpenUriError::NotRoutable("nsec is not routable".into()))
        }
        Err(nip21::Nip21Error::Nip19(e)) => Err(OpenUriError::Unparseable(e.to_string())),
    }
}

/// Map a bare (schemeless) NIP-19 entity onto the same `NostrUri` routing
/// targets `parse_nostr_uri` would produce, so both entry forms converge.
fn bare_entity_to_target(entity: nip19::Nip19Entity) -> Result<NostrUri, OpenUriError> {
    use nip19::Nip19Entity::{Naddr, Nevent, Note, Nprofile, Npub, Nsec};
    Ok(match entity {
        Nsec(_) => return Err(OpenUriError::NotRoutable("nsec is not routable".into())),
        Npub(pubkey) => NostrUri::Profile {
            pubkey,
            relays: vec![],
        },
        Nprofile(d) => NostrUri::Profile {
            pubkey: d.pubkey,
            relays: d.relays,
        },
        Note(event_id) => NostrUri::Event {
            event_id,
            relays: vec![],
            author: None,
            kind: None,
        },
        Nevent(d) => NostrUri::Event {
            event_id: d.event_id,
            relays: d.relays,
            author: d.author,
            kind: d.kind,
        },
        Naddr(d) => NostrUri::Address {
            identifier: d.identifier,
            pubkey: d.pubkey,
            kind: d.kind,
            relays: d.relays,
        },
    })
}

/// Fold relay hints onto an interest shape + hint list.
///
/// ADR-0012: the first relay hint becomes the `relay_pin` third routing lane
/// (subscriptions/publishes are addressed to that host regardless of the
/// author's NIP-65 mailboxes). All hints are also surfaced as `RelayHint`s so
/// the compiler can still use the remainder as soft outbox hints.
fn apply_relay_hints(shape: &mut InterestShape, relays: &[String]) -> Vec<RelayHint> {
    if let Some(first) = relays.first() {
        shape.relay_pin = Some(first.clone());
    }
    relays
        .iter()
        .map(|url| RelayHint {
            url: url.clone(),
            source: HintSource::UserConfigured,
        })
        .collect()
}

/// Resolve a `KernelAction::OpenUri` payload into the logical interest to
/// register and the view to open.
///
/// Pure and side-effect-free: the caller decides registry/dispatch sequencing.
/// Never panics — every failure path is a typed [`OpenUriError`] (D6).
#[must_use]
pub fn resolve_open_uri(uri: &str) -> Result<OpenUriRouting, OpenUriError> {
    let target = parse_target(uri)?;

    let (mut shape, namespace, key) = match &target {
        NostrUri::Profile { pubkey, .. } => {
            let shape = InterestShape::profile_for(pubkey.clone());
            (shape, VIEW_PROFILE, pubkey.clone())
        }
        NostrUri::Event {
            event_id,
            author,
            kind,
            ..
        } => {
            let mut shape = InterestShape::default();
            shape.event_ids.insert(event_id.clone());
            if let Some(author) = author {
                shape.authors.insert(author.clone());
            }
            if let Some(kind) = kind {
                shape.kinds.insert(*kind);
            }
            shape.limit = Some(1);
            (shape, VIEW_THREAD, event_id.clone())
        }
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            ..
        } => {
            let mut shape = InterestShape::default();
            shape.addresses.insert(NaddrCoord {
                pubkey: pubkey.clone(),
                kind: *kind,
                d_tag: identifier.clone(),
            });
            shape.authors.insert(pubkey.clone());
            shape.kinds.insert(*kind);
            shape.limit = Some(1);
            // Addressable coordinate key: `kind:pubkey:d-tag` (NIP-01 `a` form).
            let key = format!("{kind}:{pubkey}:{identifier}");
            (shape, VIEW_ADDRESSABLE, key)
        }
    };

    let relays = match &target {
        NostrUri::Profile { relays, .. }
        | NostrUri::Event { relays, .. }
        | NostrUri::Address { relays, .. } => relays.clone(),
    };
    let hints = apply_relay_hints(&mut shape, &relays);

    let interest = LogicalInterest {
        id: InterestId(0),
        scope: InterestScope::ActiveAccount,
        shape,
        hints,
        lifecycle: InterestLifecycle::OneShot,
        // Pointer-resolution interests come with explicit relay hints (the
        // `nevent` / `nprofile` / `naddr` carried them); no bootstrap-indexer
        // fallback needed.
        is_indexer_discovery: false,
    };

    Ok(OpenUriRouting {
        interest,
        view: KernelUpdate::ViewOpened {
            namespace: namespace.to_string(),
            key,
        },
    })
}

#[cfg(test)]
mod open_uri_tests {
    use super::*;
    use crate::nip19::{
        encode_naddr, encode_nevent, encode_note, encode_nprofile, encode_npub, encode_nsec,
        NaddrData, NeventData, NprofileData,
    };

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const PK2: &str = "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2";
    const EVID: &str = "5c83da77af1dec6d7289834998ad7aafbd9e2191396d75ec3cc27f5a77226f36";
    const KIND_METADATA: u32 = 0;

    #[test]
    fn npub_uri_routes_to_profile_view_with_metadata_interest() {
        let bech = encode_npub(PK).unwrap();
        let r = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            r.view,
            KernelUpdate::ViewOpened {
                namespace: VIEW_PROFILE.into(),
                key: PK.into()
            }
        );
        assert!(r.interest.shape.kinds.contains(&KIND_METADATA));
        assert!(r.interest.shape.authors.contains(PK));
        assert_eq!(
            r.interest.shape.limit,
            Some(3),
            "one event per kind (profile, contacts, relay list)"
        );
        assert!(r.interest.shape.relay_pin.is_none());
        assert!(r.interest.hints.is_empty());
    }

    #[test]
    fn bare_npub_without_scheme_resolves_identically() {
        let bech = encode_npub(PK).unwrap();
        let with = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        let bare = resolve_open_uri(&bech).unwrap();
        assert_eq!(with, bare);
    }

    #[test]
    fn nprofile_uri_relay_hint_becomes_relay_pin() {
        let bech = encode_nprofile(&NprofileData {
            pubkey: PK.into(),
            relays: vec!["wss://relay.example".into(), "wss://backup.example".into()],
        })
        .unwrap();
        let r = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            r.view,
            KernelUpdate::ViewOpened {
                namespace: VIEW_PROFILE.into(),
                key: PK.into()
            }
        );
        assert_eq!(
            r.interest.shape.relay_pin.as_deref(),
            Some("wss://relay.example")
        );
        assert_eq!(r.interest.hints.len(), 2);
    }

    #[test]
    fn note_uri_routes_to_thread_view() {
        let bech = encode_note(EVID).unwrap();
        let r = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            r.view,
            KernelUpdate::ViewOpened {
                namespace: VIEW_THREAD.into(),
                key: EVID.into()
            }
        );
        assert!(r.interest.shape.event_ids.contains(EVID));
        assert!(r.interest.shape.relay_pin.is_none());
    }

    #[test]
    fn nevent_uri_carries_author_kind_and_relay_pin() {
        let bech = encode_nevent(&NeventData {
            event_id: EVID.into(),
            relays: vec!["wss://nevent.example".into()],
            author: Some(PK.into()),
            kind: Some(1),
        })
        .unwrap();
        let r = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            r.view,
            KernelUpdate::ViewOpened {
                namespace: VIEW_THREAD.into(),
                key: EVID.into()
            }
        );
        assert!(r.interest.shape.event_ids.contains(EVID));
        assert!(r.interest.shape.authors.contains(PK));
        assert!(r.interest.shape.kinds.contains(&1));
        assert_eq!(
            r.interest.shape.relay_pin.as_deref(),
            Some("wss://nevent.example")
        );
        assert_eq!(r.interest.hints.len(), 1);
    }

    #[test]
    fn naddr_uri_routes_to_addressable_view_with_coord() {
        let bech = encode_naddr(&NaddrData {
            identifier: "my-article".into(),
            pubkey: PK2.into(),
            kind: 30023,
            relays: vec![],
        })
        .unwrap();
        let r = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            r.view,
            KernelUpdate::ViewOpened {
                namespace: VIEW_ADDRESSABLE.into(),
                key: format!("30023:{PK2}:my-article"),
            }
        );
        let coord = r.interest.shape.addresses.iter().next().unwrap();
        assert_eq!(coord.pubkey, PK2);
        assert_eq!(coord.kind, 30023);
        assert_eq!(coord.d_tag, "my-article");
        assert!(r.interest.shape.relay_pin.is_none());
    }

    #[test]
    fn naddr_uri_with_relay_hint_pins_relay() {
        let bech = encode_naddr(&NaddrData {
            identifier: "pinned".into(),
            pubkey: PK2.into(),
            kind: 30023,
            relays: vec!["wss://naddr.example".into()],
        })
        .unwrap();
        let r = resolve_open_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            r.interest.shape.relay_pin.as_deref(),
            Some("wss://naddr.example")
        );
        assert_eq!(r.interest.hints.len(), 1);
    }

    #[test]
    fn nsec_uri_is_rejected_not_routable() {
        let nsec = encode_nsec(PK).unwrap();
        assert_eq!(
            resolve_open_uri(&format!("nostr:{nsec}")),
            Err(OpenUriError::NotRoutable("nsec is not routable".into()))
        );
        assert_eq!(
            resolve_open_uri(&nsec),
            Err(OpenUriError::NotRoutable("nsec is not routable".into()))
        );
    }

    #[test]
    fn garbage_input_is_unparseable() {
        assert!(matches!(
            resolve_open_uri("not-a-nostr-thing"),
            Err(OpenUriError::Unparseable(_))
        ));
        assert!(matches!(
            resolve_open_uri("nostr:totally-bogus"),
            Err(OpenUriError::Unparseable(_))
        ));
    }

    #[test]
    fn action_round_trips_through_serde() {
        let a = KernelAction::OpenUri {
            uri: "nostr:npub1xyz".into(),
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: KernelAction = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}
