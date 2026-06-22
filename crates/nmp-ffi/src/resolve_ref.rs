//! ADR-0063 Lane D — unified `resolve_ref` / `release_ref` C-ABI surface.
//!
//! Generalizes the former per-kind profile claim + `nmp_app_claim_event` behind
//! one origin-blind entry point. ADR-0063 Lane H deleted the per-kind profile
//! `claim_*` / `release_*` symbols; profiles now resolve exclusively through
//! `nmp_app_resolve_ref`. `nmp_app_claim_event` / `nmp_app_release_event` are
//! retained (event claims keep their dedicated URI front-door).
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
//! namespace `key`
//! is either a 64-char **lowercase** hex event id or a `"kind:pubkey:d"`
//! coordinate (canonical decimal kind, lowercase-hex pubkey; the `naddr`
//! primary-id encoding) — NOT a `nostr:`/NIP-21 URI. An invalid key (wrong case,
//! wrong length, non-decimal kind, missing segment) is a silent no-op at the
//! kernel's resolver body (D6).

use super::{app_ref, c_string_argument, NmpApp};
use nmp_core::__ffi_internal::is_hex_pubkey;
use nmp_core::{ActorCommand, EventShape, ProfileShape, RefLiveness, RefNamespace, RefShape};
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
/// **`key`** — 64-hex pubkey (profile); lowercase 64-hex event-id or
///   `"kind:pubkey:d"` coordinate (event) — not a `nostr:` URI.
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
    // validates the key (it accepts both event-id hex and naddr coordinates).
    if ns == RefNamespace::Profile && !is_hex_pubkey(&key) {
        return;
    }

    let liveness_val = RefLiveness::from_ffi(liveness);

    app.send_cmd(ActorCommand::ResolveRef {
        namespace: ns,
        key,
        consumer_id,
        shape: shape_val,
        liveness: liveness_val,
        force: false,
        hints: Vec::new(),
    });
}

/// ADR-0063 Lane D — release a reference previously registered via
/// [`nmp_app_resolve_ref`].
///
/// Decrements the refcount for `consumer_id`'s stake in `(namespace, key)`.
/// The resolver slot is torn down when the last consumer releases (the same
/// release contract the former per-kind profile release and `nmp_app_release_event`
/// use).
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

    app.send_cmd(ActorCommand::ReleaseRef {
        namespace: ns,
        key,
        consumer_id,
    });
}
