//! Composition-boundary helpers for `nmp-native-runtime`'s `signer_broker.rs`.
//!
//! Despite the module's former filename (`ffi_support.rs`), it contains no
//! FFI / C-ABI code at all — it is a plain in-crate composition seam between
//! two Rust crates, renamed to `composition_boundary` to say so truthfully.
//!
//! These functions wrap `RelayRole` / `ActorLaneTransport` details that
//! `nmp-native-runtime` must not name directly (keeping it free of an
//! `nmp-network` dependency on the `signer-broker` feature path).  Production
//! code in `nmp-native-runtime/src/signer_broker.rs` calls these helpers from
//! the bunker hook closure; test code in `nmp-testing` can use the
//! lower-level runtime API directly since `nmp-testing` already depends on
//! `nmp-network`.
//!
//! ## Responsibilities
//!
//! - [`deliver_init_effects`] — translate the initial [`Effect`]s returned by
//!   `init_bunker` / `init_nostrconnect` into `CommandSender` calls so they
//!   reach the actor's pool without `nmp-native-runtime` naming
//!   `RelayRole::Signer`.
//! - [`cancel_nip46_session`] — clear the runtime and post
//!   `UnregisterPersistentSub` for every relay, unblocking EOSE-triggered
//!   CLOSE.
//! - [`restore_nip46_from_payload`] — parse a `SignerPayload::Nip46` JSON
//!   blob, call `init_restore`, build a `Nip46Signer` with an
//!   `ActorLaneTransport`, and post `AddSigner`.
//! - [`make_sub_id`] — canonical subscription-id scheme so the hook and the
//!   runtime agree on the `REQ` filter the interceptor uses.

use std::sync::Arc;

use nmp_core::{CommandSender, SignerSource};
use nmp_network::role::RelayRole;
use nmp_nip46::Effect;
use nmp_signers::signers::SignerPayload;
use nmp_signers::Nip46Signer;
use nostr::{Keys, PublicKey, SecretKey};

use crate::runtime::{
    clear_runtime, init_restore, record_signer_ready, record_user_pubkey, Nip46RuntimeHandle,
};
use crate::transport::ActorLaneTransport;

// ─── sub-id scheme ───────────────────────────────────────────────────────────

/// Canonical subscription-id for a NIP-46 session keyed by `local_pubkey`.
///
/// Format: `"nip46-<hex16>"` where `<hex16>` is the first 16 hex chars of
/// `local_pubkey.to_hex()`.  The prefix is long enough to avoid accidental
/// collisions between concurrent sessions while remaining short enough for
/// relay filter labels.
///
/// The interceptor's `on_idle_tick` uses `persistent_sub_registered` to avoid
/// double-registering; callers must use THIS function to guarantee the sub-id
/// matches the REQ frame extracted by `extract_sub_id`.
#[must_use]
pub fn make_sub_id(local_pubkey: PublicKey) -> String {
    format!("nip46-{}", &local_pubkey.to_hex()[..16])
}

// ─── deliver_init_effects ────────────────────────────────────────────────────

/// Translate the initial [`Effect`]s from `init_bunker` / `init_nostrconnect`
/// into actor commands via `sender`.
///
/// - [`Effect::Subscribe`] → `enqueue_outbound(RelayRole::Signer, …)` (sends
///   the `["REQ", …]` wire frame) **+** `set_reconnect_preamble(…)` (registers
///   the same frame as the relay worker's reconnect preamble so the REQ
///   arrives before any `EnqueueOutbound` EVENT after a reconnect).
/// - [`Effect::SendFrame`] → `enqueue_outbound(RelayRole::Signer, …)` (sends
///   the `["EVENT", …]` connect frame).
/// - [`Effect::Progress`] → `bunker_handshake_progress(…)`.
/// - All other variants (should not appear from init functions) are silently
///   ignored.
///
/// ## Ordering
///
/// The actor processes inbox commands in FIFO order.  The `EnqueueOutbound`
/// for the Subscribe frame is posted before the `SetReconnectPreamble` and
/// before the `EnqueueOutbound` for the SendFrame — matching the
/// `["REQ", …]` + `["EVENT", …]` wire ordering the protocol requires.
pub fn deliver_init_effects(effects: Vec<Effect>, sender: &CommandSender) {
    for effect in effects {
        match effect {
            Effect::Subscribe { relay_url, frame } => {
                sender.enqueue_outbound(RelayRole::Signer, relay_url.clone(), frame.clone());
                sender.set_reconnect_preamble(RelayRole::Signer, relay_url, vec![frame]);
            }
            Effect::SendFrame { relay_url, text } => {
                sender.enqueue_outbound(RelayRole::Signer, relay_url, text);
            }
            Effect::Progress {
                stage,
                code,
                detail,
            } => {
                sender.bunker_handshake_progress(stage, code, detail);
            }
            // SignerReady, DeliverResponse, Error are not expected from init
            // functions; silently ignore so we never panic on unexpected output.
            _ => {}
        }
    }
}

// ─── cancel_nip46_session ────────────────────────────────────────────────────

/// Cancel the active NIP-46 session.
///
/// 1. Reads relay URLs and sub_id from the handle under the mutex.
/// 2. Calls [`clear_runtime`] to drop session state (future relay frames and
///    connected-hook calls become no-ops).
/// 3. Posts `UnregisterPersistentSub` for every relay via `sender` so the
///    relay worker stops suppressing EOSE-triggered CLOSE for this sub_id.
///
/// No-op when no session is active (handle is `None`).
pub fn cancel_nip46_session(handle: &Nip46RuntimeHandle, sender: &CommandSender) {
    let (relay_urls, sub_id) = {
        let Ok(guard) = handle.lock() else { return };
        let Some(rt) = guard.as_ref() else { return };
        (rt.relay_urls().to_vec(), rt.sub_id().to_string())
    };
    clear_runtime(handle);
    for relay_url in relay_urls {
        sender.unregister_persistent_sub(relay_url, sub_id.clone());
    }
}

// ─── restore_nip46_from_payload ──────────────────────────────────────────────

/// Restore a NIP-46 session from a persisted `SignerPayload::Nip46` JSON blob.
///
/// Steps:
/// 1. Parse `payload_json` as [`SignerPayload`]; return `Err` if not `Nip46`.
/// 2. Build `local_keys` from the persisted `local_secret_hex`.
/// 3. Derive `sub_id` via [`make_sub_id`] (persisted sessions use the same
///    scheme as new sessions — the relay filter is keyed by local pubkey).
/// 4. Call [`init_restore`] to seed the handle with the persisted relay URLs
///    and keys in the `Done` phase.
/// 5. Deliver the returned `Effect::Subscribe` effects via
///    [`deliver_init_effects`] so the relay worker replays the REQ on reconnect.
/// 6. Build a `Nip46Signer` via `Nip46Signer::from_payload` with a new
///    `ActorLaneTransport`.
/// 7. Call [`record_signer_ready`] with the cached remote user pubkey so
///    steady-state decode uses the correct key immediately.
/// 8. Post `AddSigner(RemoteHandle(signer))` via `sender` with `make_active = false`
///    (the account manager decides activation from stored state).
///
/// Returns `Err(String)` when parsing fails, the payload is not NIP-46, or
/// signer construction fails (e.g. no cached remote user pubkey yet).
pub fn restore_nip46_from_payload(
    handle: &Nip46RuntimeHandle,
    payload_json: &str,
    sender: CommandSender,
    now_secs: u64,
) -> Result<(), String> {
    // ── Step 1: parse payload ─────────────────────────────────────────────
    let payload: SignerPayload = serde_json::from_str(payload_json)
        .map_err(|e| format!("restore_nip46: invalid payload JSON: {e}"))?;
    let SignerPayload::Nip46(ref p) = payload else {
        return Err("restore_nip46: expected Nip46 variant".to_string());
    };

    // ── Step 2: build local keys ──────────────────────────────────────────
    let local_sk = SecretKey::from_hex(p.local_secret_hex.as_str())
        .map_err(|e| format!("restore_nip46: invalid local_secret_hex: {e}"))?;
    let local_keys = Keys::new(local_sk);
    let local_pubkey = local_keys.public_key();

    // ── Step 3: remote pubkey ─────────────────────────────────────────────
    let remote_pubkey = PublicKey::from_hex(&p.remote_pubkey_hex)
        .map_err(|e| format!("restore_nip46: invalid remote_pubkey_hex: {e}"))?;

    // ── Step 4: seed runtime ──────────────────────────────────────────────
    let sub_id = make_sub_id(local_pubkey);
    let relay_urls = p.relays.clone();
    let subscribe_effects = init_restore(
        handle,
        sub_id,
        local_keys.clone(),
        remote_pubkey,
        relay_urls.clone(),
        now_secs,
    )?;

    // ── Step 5: deliver Subscribe effects (REQ replay) ────────────────────
    deliver_init_effects(subscribe_effects, &sender);

    // ── Step 6: build signer ──────────────────────────────────────────────
    let transport =
        ActorLaneTransport::new_multi(sender.clone(), local_keys, remote_pubkey, relay_urls);
    let signer = Nip46Signer::from_payload(p, Arc::new(transport))
        .map_err(|e| format!("restore_nip46: signer build failed: {e}"))?;

    // ── Step 7: write-back learned pubkey ────────────────────────────────
    let remote_user_pubkey = signer.remote_user_pubkey();
    record_signer_ready(handle, remote_user_pubkey);
    // #2976 — this restored session's account identity is the user pubkey; store
    // it so a later reconnect's "connected" health event is attributed to this
    // account instead of clobbering whatever account is active.
    record_user_pubkey(handle, remote_user_pubkey);

    // ── Step 8: add signer ────────────────────────────────────────────────
    sender.add_signer(SignerSource::RemoteHandle(Box::new(signer)), false);

    Ok(())
}
