//! ADR-0063 Lane D — unified `resolve_ref` / `release_ref` C-ABI surface.
//!
//! Generalizes the former per-kind profile/event claim entry points behind one
//! origin-blind entry point. ADR-0063 Lane H deleted the per-kind profile
//! `claim_*` / `release_*` symbols; #1946 deleted the event URI front doors.
//! Profiles and events now resolve through raw-key `resolve_ref`; app-owned URI
//! adapters use `nmp_app_resolve_ref_with_metadata` when decoded relay/author
//! TLVs need to cross the same raw-key seam.
//!
//! ## Integer encoding
//!
//! **Why `i32` integers for namespace/shape/liveness?** The C-ABI boundary
//! cannot cross Rust enums. Three small closed integer codes with in-process
//! decode (fail-closed on unknown values) carry the same `liveness` intent the
//! former per-kind profile claim used, and keep the header readable without a C
//! enum typedef.
//! An alternative was to keep the shape as a pair `(namespace, shape_within_ns)`,
//! but a single `shape` int avoids ambiguity: each value is globally unique and
//! unambiguous regardless of the caller's namespace, making the pair check at the
//! kernel's `resolve_ref` front door (which fails closed on mismatch) an extra
//! safety net rather than the primary guard.
//!
//! ### `namespace` values (`i32`)
//! * `0` — Profile (`refs.profile`)
//! * `1` — Event (`refs.event`)
//!
//! ### `shape` values (`i32`) — globally unique across namespaces
//! * `0` — `profile.ref` (`{pubkey, display_name, picture_url}`; feed-avatar).
//!   Only valid with `namespace == 0`.
//! * `1` — `profile.card` (full `ProfileCard`; profile-screen).
//!   Only valid with `namespace == 0`.
//! * `2` — `event.embed` (render-an-embed-card subset).
//!   Only valid with `namespace == 1`.
//! * `3` — `event.raw` (full raw event).
//!   Only valid with `namespace == 1`.
//!
//! ### `liveness` values (`i32`)
//! * `0` — `CacheOk` (serve from store; OneShot fetch on miss; no live sub).
//!   Use for feed-row avatars and background embed claims.
//! * non-zero — `Live` (tailing sub kept open while the consumer holds the key).
//!   Use for an open profile screen or live-updating embed.
//!
//! Unknown integer values are silent no-ops (D6: fail closed, never a panic or
//! an FFI error return).
//!
//! ## Key encoding
//!
//! For the `Profile` namespace `key` must be a 64-hex-char lowercase pubkey (the
//! same constraint the former per-kind profile claim enforced). For the `Event`
//! namespace `key` is one of three forms — NOT a `nostr:`/NIP-21 URI:
//!
//! * a 64-char **lowercase** hex event id, or
//! * a `"kind:pubkey:d"` coordinate (canonical decimal kind, lowercase-hex
//!   pubkey; the `naddr` primary-id encoding), or
//! * an `"i:<external-id>"` NIP-73 external reference (#1654 — e.g.
//!   `i:podcast:item:guid:<guid>`, `i:isbn:<n>`, `i:doi:<id>`). The `i:` prefix
//!   disambiguates the external ref; `<external-id>` is the verbatim NIP-73
//!   `i`-tag value. The resolver fetches the event tagging that external id and
//!   surfaces it in `refs.event` keyed by the full `i:<external-id>` string.
//!
//! An invalid key (wrong case, wrong length, non-decimal kind, missing segment,
//! empty external id) is a silent no-op at the kernel's resolver body (D6).

use super::{NmpApp, app_ref, c_string_argument};
use nmp_core::__ffi_internal::is_hex_pubkey;
use nmp_core::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape};
use serde_json::Value;
use std::ffi::{c_char, c_int};

/// Decode the `namespace` FFI integer into a [`RefNamespace`].
/// Returns `None` (silent no-op, D6) for unknown values.
fn decode_namespace(namespace: c_int) -> Option<RefNamespace> {
    match namespace {
        0 => Some(RefNamespace::Profile),
        1 => Some(RefNamespace::Event),
        _ => None,
    }
}

/// Decode the `shape` FFI integer into a [`RefShape`].
///
/// Shape codes are globally unique across namespaces, so each code is callable
/// with exactly ONE namespace. The four valid `(namespace, shape)` pairs are:
/// `(0,0)`=profile.ref, `(0,1)`=profile.card, `(1,2)`=event.embed,
/// `(1,3)`=event.raw. A cross-namespace pair such as `(1,0)` (event namespace
/// with the profile.ref code) or `(0,2)` (profile namespace with the event.embed
/// code) is rejected at the kernel's `resolve_ref` front door, which validates
/// `shape.namespace() == namespace` and fails closed (D6) on mismatch — a second
/// safety net behind this decode.
///
/// Returns `None` (silent no-op, D6) for unknown values.
fn decode_shape(shape: c_int) -> Option<RefShape> {
    match shape {
        0 => Some(RefShape::Profile(ProfileShape::Ref)),
        1 => Some(RefShape::Profile(ProfileShape::Card)),
        2 => Some(RefShape::Event(EventShape::Embed)),
        3 => Some(RefShape::Event(EventShape::Raw)),
        _ => None,
    }
}

fn decode_metadata(metadata_json: *const c_char) -> Option<RefResolveMetadata> {
    if metadata_json.is_null() {
        return Some(RefResolveMetadata::default());
    }
    let raw = c_string_argument(metadata_json)?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let object = value.as_object()?;

    let hints = match object.get("hints") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(item.as_str()?.to_string());
            }
            out
        }
        Some(_) => return None,
    };

    let event_author = match object.get("event_author").or_else(|| object.get("author")) {
        None | Some(Value::Null) => None,
        Some(Value::String(author)) if is_hex_pubkey(author) => Some(author.clone()),
        Some(Value::String(_)) => return None,
        Some(_) => return None,
    };

    // The deleted URI front door ignored nevent kind TLV for event-id fetches
    // because the raw event-id filter is already exact. Accept and validate the
    // field here so app-owned adapters can pass the decoded URI metadata object
    // unchanged, but keep behavior identical.
    if let Some(kind) = object.get("event_kind").or_else(|| object.get("kind")) {
        let Some(kind) = kind.as_u64() else {
            return None;
        };
        if u32::try_from(kind).is_err() {
            return None;
        }
    }

    Some(RefResolveMetadata {
        hints,
        event_author,
    })
}

fn resolve_ref_typed(
    app: *mut NmpApp,
    ns: RefNamespace,
    key: *const c_char,
    consumer_id: *const c_char,
    shape: RefShape,
    liveness: RefLiveness,
    metadata: RefResolveMetadata,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if ns == RefNamespace::Profile && !is_hex_pubkey(&key) {
        return;
    }
    app.resolve_ref_with_metadata(ns, key, consumer_id, shape, liveness, metadata);
}

fn release_ref_typed(
    app: *mut NmpApp,
    ns: RefNamespace,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if ns == RefNamespace::Profile && !is_hex_pubkey(&key) {
        return;
    }
    app.release_ref(ns, key, consumer_id);
}

/// ADR-0063 Lane D — unified, origin-blind reference-resolution entry point.
///
/// Registers (or upgrades) a consumer's interest in the entity identified by
/// `(namespace, key)`. The kernel refcounts per `consumer_id`; a key already
/// held by another consumer is deduped to one resolver slot with the widest
/// requested `shape` and the highest `liveness` (`Live` wins).
///
/// On the first claim for a key the kernel fetches the entity (store-first, then
/// relay) and surfaces it in the matching typed projection (`refs.profile` /
/// `refs.event`) keyed by `key` in the next update frame.
///
/// **`namespace`** — `0` = profile, `1` = event.
/// **`key`** — 64-hex pubkey (profile); lowercase 64-hex event-id,
///   `"kind:pubkey:d"` coordinate, or `"i:<external-id>"` NIP-73 external ref
///   (event) — not a `nostr:` URI.
/// **`consumer_id`** — opaque caller-chosen refcount owner key (e.g. SwiftUI view id).
/// **`shape`** — `0`=profile.ref `1`=profile.card `2`=event.embed `3`=event.raw.
/// **`liveness`** — `0`=CacheOk (background), non-zero=Live (open screen).
///
/// D6: null/invalid arguments and unknown integer codes are silent no-ops.
/// D8: fire-and-forget; the actor processes the command asynchronously.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_ref(
    app: *mut NmpApp,
    namespace: c_int,
    key: *const c_char,
    consumer_id: *const c_char,
    shape: c_int,
    liveness: c_int,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(ns) = decode_namespace(namespace) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    let Some(shape_val) = decode_shape(shape) else {
        return;
    };

    // D6: for the Profile namespace, validate the key is a hex pubkey before
    // sending to the actor. For the Event namespace the kernel's resolver body
    // validates the key (it accepts event-id hex, naddr coordinates, and
    // `i:<external-id>` NIP-73 external refs).
    if ns == RefNamespace::Profile && !is_hex_pubkey(&key) {
        return;
    }

    let liveness_val = RefLiveness::from_ffi(liveness);

    app.resolve_ref(ns, key, consumer_id, shape_val, liveness_val);
}

/// ADR-0063 raw-key reference resolution with caller-decoded metadata.
///
/// `metadata_json` is optional JSON:
/// `{ "hints": ["wss://..."], "author": "<hex pubkey>", "kind": 1 }`.
/// It is for app-owned URI adapters that decode `nostr:` / NIP-19 values before
/// crossing the FFI boundary. The key is still raw and never a URI.
///
/// D6: malformed metadata is a silent no-op for the whole resolve; null metadata
/// is equivalent to `{}`.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_ref_with_metadata(
    app: *mut NmpApp,
    namespace: c_int,
    key: *const c_char,
    consumer_id: *const c_char,
    shape: c_int,
    liveness: c_int,
    metadata_json: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(ns) = decode_namespace(namespace) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    let Some(shape_val) = decode_shape(shape) else {
        return;
    };
    if ns == RefNamespace::Profile && !is_hex_pubkey(&key) {
        return;
    }
    let Some(metadata) = decode_metadata(metadata_json) else {
        return;
    };

    let liveness_val = RefLiveness::from_ffi(liveness);

    app.resolve_ref_with_metadata(ns, key, consumer_id, shape_val, liveness_val, metadata);
}

/// ADR-0063 Lane D — release a reference previously registered via
/// [`nmp_app_resolve_ref`].
///
/// Decrements the refcount for `consumer_id`'s stake in `(namespace, key)`.
/// The resolver slot is torn down when the last consumer releases (the same
/// release contract the former per-kind profile/event releases used).
///
/// **`namespace`** — `0` = profile, `1` = event (must match the `resolve_ref` call).
/// **`key`** — same key that was passed to `nmp_app_resolve_ref`.
/// **`consumer_id`** — same consumer_id that was passed to `nmp_app_resolve_ref`.
///
/// D6: null/invalid arguments, unknown integer codes, and unknown
/// `(namespace, key, consumer_id)` triples are silent no-ops.
/// D8: fire-and-forget; the actor processes the command asynchronously.
#[no_mangle]
pub extern "C" fn nmp_app_release_ref(
    app: *mut NmpApp,
    namespace: c_int,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(ns) = decode_namespace(namespace) else {
        return;
    };
    let Some(key) = c_string_argument(key) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };

    app.release_ref(ns, key, consumer_id);
}

/// Typed profile-ref adapter for host shells.
///
/// Resolves `key` as `refs.profile` / `profile.ref` with `CacheOk` liveness.
/// The caller cannot express an event/profile shape mismatch because the
/// namespace, shape, and liveness are selected by this adapter.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_profile_ref(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    resolve_ref_typed(
        app,
        RefNamespace::Profile,
        key,
        consumer_id,
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        RefResolveMetadata::default(),
    );
}

/// Typed profile-card adapter for open profile screens.
///
/// Resolves `key` as `refs.profile` / `profile.card` with `Live` liveness.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_profile_card_live(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    resolve_ref_typed(
        app,
        RefNamespace::Profile,
        key,
        consumer_id,
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::Live,
        RefResolveMetadata::default(),
    );
}

/// Release a profile ref acquired through a typed profile adapter.
#[no_mangle]
pub extern "C" fn nmp_app_release_profile_ref(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    release_ref_typed(app, RefNamespace::Profile, key, consumer_id);
}

/// Typed event-embed adapter with `CacheOk` liveness and no URI metadata.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_event_embed(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    resolve_ref_typed(
        app,
        RefNamespace::Event,
        key,
        consumer_id,
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        RefResolveMetadata::default(),
    );
}

/// Typed event-embed adapter with `Live` liveness and no URI metadata.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_event_embed_live(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    resolve_ref_typed(
        app,
        RefNamespace::Event,
        key,
        consumer_id,
        RefShape::Event(EventShape::Embed),
        RefLiveness::Live,
        RefResolveMetadata::default(),
    );
}

/// Typed event-embed adapter for app-owned URI adapters.
///
/// `metadata_json` has the same optional `{ "hints": [...], "author": "...",
/// "kind": n }` shape accepted by [`nmp_app_resolve_ref_with_metadata`].
#[no_mangle]
pub extern "C" fn nmp_app_resolve_event_embed_with_metadata(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
    metadata_json: *const c_char,
) {
    let Some(metadata) = decode_metadata(metadata_json) else {
        return;
    };
    resolve_ref_typed(
        app,
        RefNamespace::Event,
        key,
        consumer_id,
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        metadata,
    );
}

/// Typed live event-embed adapter for app-owned URI adapters.
#[no_mangle]
pub extern "C" fn nmp_app_resolve_event_embed_live_with_metadata(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
    metadata_json: *const c_char,
) {
    let Some(metadata) = decode_metadata(metadata_json) else {
        return;
    };
    resolve_ref_typed(
        app,
        RefNamespace::Event,
        key,
        consumer_id,
        RefShape::Event(EventShape::Embed),
        RefLiveness::Live,
        metadata,
    );
}

/// Release an event ref acquired through a typed event adapter.
#[no_mangle]
pub extern "C" fn nmp_app_release_event_ref(
    app: *mut NmpApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    release_ref_typed(app, RefNamespace::Event, key, consumer_id);
}
