//! Pure helper functions used by `dispatch_command` arms.
//!
//! Extracted from `dispatch.rs` to keep `mod.rs` under the LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.

use crate::actor::commands;
use crate::actor::pending_sign::ParkedSignerOps;
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::slots::{ActiveLocalKeysSlot, MlsLocalNsecSlot};
use zeroize::Zeroizing;

use super::IdentityRuntime;

/// Sync every host-readable local-key mirror to the current active account.
///
/// Two parallel substrate-generic slots track the active account's local
/// signing material on every identity mutation:
///
/// * `mls_local_nsec` — bech32 `nsec1…` wrapped in [`Zeroizing`] so the
///   previous string is wiped from the heap on overwrite.
/// * `active_local_keys` — the parsed `nostr::Keys`. `Keys` zeroizes its own
///   secret on drop, so no extra wrapper is needed.
///
/// Both derive from `identity.active_keys()`, so they always change together.
/// The substrate publishes both unconditionally; non-substrate consumers
/// (FFI-shell readers exposed via `NmpApp::active_local_keys`) decide what
/// to do with the data (today: NIP-17 gift-wrap unsealing, NIP-57 zap
/// receipt pubkey reads). Each slot is locked, written, and dropped
/// sequentially — there is no cross-slot atomicity contract (a host that
/// races a snapshot read against an identity switch may briefly observe one
/// slot updated and the other not; the next snapshot tick reconciles).
///
/// Called synchronously BEFORE `maybe_emit_after_dispatch` (and before
/// `emit_now` on the `Start` arm) so the slots are visible to host callbacks
/// before any snapshot fires.
pub(super) fn update_local_key_slots(
    identity: &IdentityRuntime,
    nsec_slot: &MlsLocalNsecSlot,
    keys_slot: &ActiveLocalKeysSlot,
) {
    if let Ok(mut guard) = nsec_slot.lock() {
        *guard = identity.active_nsec_bech32().map(Zeroizing::new);
    }
    if let Ok(mut guard) = keys_slot.lock() {
        *guard = identity.active_local_keys().cloned();
    }
}

/// Re-publish the active account's NIP-65 kind:10002 relay list after an
/// `AddRelay` / `RemoveRelay` mutation, so other clients reading the relay
/// graph see the same set the user just edited.
///
/// # Why
///
/// Before this hook, the actor's `AddRelay` / `RemoveRelay` arms mutated
/// the local `AppRelay` projection and dialed / dropped sockets, but
/// never re-published the user's NIP-65 outbox. The asymmetric leak:
/// removing a defunct relay never told other clients to stop fanning out
/// to it; adding a new relay never told contacts to read/write there. The
/// `nmp.nip65.publish_relay_list` action (`nmp-router` crate) closes the
/// host-dispatched half of the loop; this helper closes the actor-internal
/// half so the FFI `nmp_app_add_relay` / `nmp_app_remove_relay` paths and
/// any non-action caller of those `ActorCommand`s also keep NIP-65 in
/// sync.
///
/// # Skip semantics — three guards
///
/// 1. **No active account.** A relay edit while signed out is a local
///    settings change; there is no identity to sign under. `publish_unsigned_event`
///    would otherwise set an error toast via `toast_no_account`, which is
///    the wrong observable for a config edit.
/// 2. **Projection unchanged.** Re-adding an already-present URL with the
///    same role, or removing a URL that was never present, leaves the
///    projection identical to its prior state. Republishing kind:10002
///    in that case would waste a write and bump the timestamp for no
///    behavioural change. `projection_before` is the snapshot the caller
///    took *before* the local mutation; equality means "no semantic change".
/// 3. **No NIP-65-eligible rows.** A projection containing only pure-indexer
///    rows (or one that becomes empty after the edit) cannot produce a
///    kind:10002 with `r` tags. `build_relay_list_event`
///    returns `None` in that case, and the function bails before any
///    publish — an empty kind:10002 is the destructive "clear my NIP-65
///    metadata" signal in `ingest_relay_list`, and we must never emit
///    that as a side effect of a relay edit.
///
/// # `correlation_id`
///
/// `None` — these are actor-internal publishes piggybacked onto a local
/// mutation, not action-dispatched. Hosts that *want* an observable
/// terminal verdict dispatch `nmp.nip65.publish_relay_list` directly,
/// which threads a registry-minted id through `PublishUnsignedEvent`.
///
/// # `created_at`
///
/// D7 sentinel: the builder sets `created_at = 0`; the actor's
/// `PublishUnsignedEvent` arm re-stamps it from the kernel clock. This
/// function never reads the system clock.
pub(super) fn maybe_publish_relay_list_after_edit(
    identity: &commands::IdentityRuntime,
    kernel: &mut Kernel,
    projection_before: &[crate::kernel::AppRelay],
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    // Guard 1: must have an active signer.
    if identity.active_pubkey().is_none() {
        return Vec::new();
    }
    // Guard 2: skip on no-op projection change.
    let projection_after = kernel.configured_relays_snapshot();
    if projection_after == projection_before {
        return Vec::new();
    }
    // Guard 3: skip when the projection has no NIP-65 expression.
    let Some(unsigned) = commands::build_relay_list_event(projection_after) else {
        return Vec::new();
    };
    commands::publish_unsigned_event(identity, kernel, unsigned, None, None, parked_ops)
}

/// Parse a host sign-and-return draft into an [`nmp_signer_iface::UnsignedEvent`].
///
/// The draft is `{ "kind": u64, "content": str, "tags": [[str, …], …],
/// "created_at": u64? }` — the shape `nmp_app_sign_event_for_return` accepts.
/// It carries NO `pubkey` (the host does not know which signer will be used)
/// and its `created_at` is advisory, so this helper fills both:
///
/// * `pubkey` ← the resolved signer's hex pubkey.
/// * `created_at` ← the kernel clock (`now_secs`, D7) — the host never owns
///   wall-clock time; any `created_at` in the draft is ignored.
///
/// `tags` defaults to empty when absent. `kind` and `content` are required.
pub(super) fn build_unsigned_for_return(
    unsigned_json: &str,
    signer_pubkey: &str,
    now_secs: u64,
) -> Result<nmp_signer_iface::UnsignedEvent, String> {
    let value: serde_json::Value =
        serde_json::from_str(unsigned_json).map_err(|e| e.to_string())?;
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "missing or non-integer `kind`".to_string())?;
    let kind = u32::try_from(kind).map_err(|_| "`kind` out of u32 range".to_string())?;
    let content = value
        .get("content")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "missing or non-string `content`".to_string())?
        .to_string();
    let tags: Vec<Vec<String>> = match value.get("tags") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(tags_value) => serde_json::from_value(tags_value.clone())
            .map_err(|e| format!("`tags` must be an array of string arrays: {e}"))?,
    };
    Ok(nmp_signer_iface::UnsignedEvent {
        pubkey: signer_pubkey.to_string(),
        kind,
        tags,
        content,
        created_at: now_secs,
    })
}

/// Serialize a [`nmp_signer_iface::SignedEvent`] into the standard flat Nostr
/// event JSON: `{ "id", "pubkey", "created_at", "kind", "tags", "content",
/// "sig" }`. This is the on-wire NIP-01 event object (the inner body of an
/// `["EVENT", …]` frame), which is what a host base64-encodes for a Blossom
/// `Authorization: Nostr …` header. NOT the kernel-internal `SignedEvent`
/// serde shape (which nests under `unsigned`).
///
/// `pub(super)` so the idle-loop parked-op drain in `mod.rs` reuses the exact
/// same flat-event serialization the dispatch arm uses.
pub(crate) fn signed_event_to_json(signed: &nmp_signer_iface::SignedEvent) -> String {
    // Delegates to the public `SignedEvent::to_nip01_json` so the flat-event
    // serialization has exactly one definition shared by the dispatch arm, the
    // idle-loop drain, and protocol-crate workers.
    signed.to_nip01_json()
}

#[cfg(test)]
mod sign_return_tests {
    //! D13 sign-and-return — unit tests for the two pure helpers the
    //! `SignEventForReturn` dispatch arm relies on: `build_unsigned_for_return`
    //! (host draft → `UnsignedEvent`, filling pubkey + clock-stamped
    //! `created_at`) and `signed_event_to_json` (kernel `SignedEvent` → the flat
    //! NIP-01 event JSON the host base64-encodes for a Blossom auth header).
    use super::{build_unsigned_for_return, signed_event_to_json};
    use nmp_signer_iface::{SignedEvent, UnsignedEvent};

    #[test]
    fn build_unsigned_fills_pubkey_and_restamps_created_at() {
        let draft = r#"{"kind":24242,"content":"Upload image","tags":[["t","upload"],["x","deadbeef"]],"created_at":111}"#;
        let unsigned = build_unsigned_for_return(draft, "signerpub", 999).expect("valid draft");
        // pubkey comes from the resolved signer, not the draft (the draft has none).
        assert_eq!(unsigned.pubkey, "signerpub");
        // created_at is re-stamped from the kernel clock (D7), ignoring the draft's 111.
        assert_eq!(unsigned.created_at, 999);
        assert_eq!(unsigned.kind, 24242);
        assert_eq!(unsigned.content, "Upload image");
        assert_eq!(
            unsigned.tags,
            vec![
                vec!["t".to_string(), "upload".to_string()],
                vec!["x".to_string(), "deadbeef".to_string()],
            ]
        );
    }

    #[test]
    fn build_unsigned_defaults_tags_to_empty_when_absent() {
        let unsigned =
            build_unsigned_for_return(r#"{"kind":1,"content":"hi"}"#, "pk", 5).expect("valid");
        assert!(unsigned.tags.is_empty(), "absent tags default to empty");
    }

    #[test]
    fn build_unsigned_rejects_missing_kind() {
        let err = build_unsigned_for_return(r#"{"content":"x"}"#, "pk", 0)
            .expect_err("missing kind is rejected");
        assert!(err.contains("kind"), "error names the missing field: {err}");
    }

    #[test]
    fn build_unsigned_rejects_missing_content() {
        let err = build_unsigned_for_return(r#"{"kind":1}"#, "pk", 0)
            .expect_err("missing content is rejected");
        assert!(
            err.contains("content"),
            "error names the missing field: {err}"
        );
    }

    #[test]
    fn build_unsigned_rejects_malformed_json() {
        assert!(
            build_unsigned_for_return("not json", "pk", 0).is_err(),
            "malformed JSON is rejected (surfaced as an Err verdict, never a panic)"
        );
    }

    #[test]
    fn signed_event_to_json_produces_flat_nip01_shape() {
        let signed = SignedEvent {
            id: "aa".repeat(32),
            sig: "bb".repeat(64),
            unsigned: UnsignedEvent {
                pubkey: "cc".repeat(32),
                kind: 24242,
                tags: vec![vec!["t".to_string(), "upload".to_string()]],
                content: "Upload image".to_string(),
                created_at: 1234,
            },
        };
        let json: serde_json::Value =
            serde_json::from_str(&signed_event_to_json(&signed)).expect("valid JSON");
        // Flat NIP-01 shape — NOT nested under `unsigned` (the kernel serde shape).
        assert_eq!(
            json.get("id").and_then(|v| v.as_str()),
            Some(signed.id.as_str())
        );
        assert_eq!(
            json.get("pubkey").and_then(|v| v.as_str()),
            Some(signed.unsigned.pubkey.as_str())
        );
        assert_eq!(
            json.get("kind").and_then(serde_json::Value::as_u64),
            Some(24242)
        );
        assert_eq!(
            json.get("created_at").and_then(serde_json::Value::as_u64),
            Some(1234)
        );
        assert_eq!(
            json.get("sig").and_then(|v| v.as_str()),
            Some(signed.sig.as_str())
        );
        assert_eq!(
            json.get("content").and_then(|v| v.as_str()),
            Some("Upload image")
        );
        assert!(
            json.get("unsigned").is_none(),
            "the wire shape is flat — no `unsigned` nesting"
        );
    }
}
