//! Session initialization, restore, and signer construction for NIP-46 runtime.

use nmp_core::CommandSender;
use nmp_nip46::{start_bunker, start_nostrconnect, start_restore};
use nmp_nip46::{Effect, SignerReady};
use nmp_signers::{Nip46Signer, Nip46SignerHandle};
use nostr::{Keys, PublicKey};
use std::sync::Arc;

use super::{canonicalize_relay, Nip46Runtime, Nip46RuntimeHandle};
use crate::transport::ActorLaneTransport;

/// Initialise the handle with a `bunker://` session.
///
/// `relay_urls` is the FULL relay list from the parsed bunker URI
/// (`BunkerUri::relays` supports multiple relays). The session subscribes
/// (REQ) and publishes the `connect` EVENT to EVERY relay, and accepts inbound
/// responses from any of them. Every URL is canonicalized at this boundary so
/// inbound/reconnect filtering matches the pool's canonical keys.
///
/// Returns the initial [`Effect`]s — a Subscribe + Progress + SendFrame for the
/// primary relay, plus a Subscribe + SendFrame fanned to each additional relay.
/// The caller MUST:
///
/// 1. Call `kernel.register_persistent_sub(relay_url, sub_id)` for each
///    Subscribe (or leave `persistent_sub_registered = false` so the
///    interceptor registers on first idle tick).
/// 2. Convert the [`Effect::Subscribe`] and [`Effect::SendFrame`] results
///    into `OutboundMessage`s and deliver them to the relay.
///
/// Returns `Err(String)` when the runtime mutex is poisoned or `relay_urls` is
/// empty.
#[allow(clippy::too_many_arguments)]
pub fn init_bunker(
    handle: &Nip46RuntimeHandle,
    sub_id: String,
    local_keys: Keys,
    remote_pubkey: PublicKey,
    relay_urls: Vec<String>,
    secret: Option<&str>,
    perms: Option<&str>,
    now_secs: u64,
) -> Result<Vec<Effect>, String> {
    let relay_urls = canonicalize_relays(&relay_urls);
    let Some(primary) = relay_urls.first().cloned() else {
        return Err("init_bunker: relay_urls must not be empty".to_string());
    };
    let (state, mut effects) = start_bunker(
        &sub_id,
        local_keys.clone(),
        remote_pubkey,
        primary,
        secret,
        perms,
        now_secs,
    );

    if relay_urls.len() > 1 {
        let req_frame = effects.iter().find_map(|e| match e {
            Effect::Subscribe { frame, .. } => Some(frame.clone()),
            _ => None,
        });
        let connect_frame = effects.iter().find_map(|e| match e {
            Effect::SendFrame { text, .. } => Some(text.clone()),
            _ => None,
        });
        for relay in &relay_urls[1..] {
            if let Some(frame) = &req_frame {
                effects.push(Effect::Subscribe {
                    relay_url: relay.clone(),
                    frame: frame.clone(),
                });
            }
            if let Some(text) = &connect_frame {
                effects.push(Effect::SendFrame {
                    relay_url: relay.clone(),
                    text: text.clone(),
                });
            }
        }
    }

    let rt = Nip46Runtime {
        state,
        relay_urls,
        sub_id,
        local_keys,
        remote_pubkey,
        user_pubkey: None,
        persistent_sub_registered: false,
    };
    match handle.lock() {
        Ok(mut guard) => {
            *guard = Some(rt);
            Ok(effects)
        }
        Err(_) => Err("runtime handle mutex poisoned".to_string()),
    }
}

/// Initialise the handle with a `nostrconnect://` session.
///
/// Same contract as [`init_bunker`] — caller must register the persistent
/// sub and deliver initial outbound frames.
///
/// Returns `Ok((uri, effects))` where `uri` is the `nostrconnect://` URI to
/// display as a QR code and `effects` are the initial protocol effects
/// (Subscribe + Progress).
#[allow(clippy::too_many_arguments)]
pub fn init_nostrconnect(
    handle: &Nip46RuntimeHandle,
    sub_id: String,
    local_keys: Keys,
    relay_url: String,
    expected_secret: String,
    perms: Option<String>,
    name: &str,
    now_secs: u64,
) -> Result<(String, Vec<Effect>), String> {
    let relay_url = canonicalize_relay(&relay_url);
    let relay_urls = vec![relay_url.clone()];
    let (uri, state, effects) = start_nostrconnect(
        &sub_id,
        local_keys.clone(),
        relay_url,
        expected_secret,
        perms,
        name,
        now_secs,
    );
    let local_pk = local_keys.public_key();
    let rt = Nip46Runtime {
        state,
        relay_urls,
        sub_id,
        local_keys,
        remote_pubkey: local_pk,
        user_pubkey: None,
        persistent_sub_registered: false,
    };
    match handle.lock() {
        Ok(mut guard) => {
            *guard = Some(rt);
            Ok((uri, effects))
        }
        Err(_) => Err("runtime handle mutex poisoned".to_string()),
    }
}

/// Restore the handle from a persisted `SignerPayload::Nip46` without
/// re-running the handshake.
///
/// The restored session is in the `Done` phase so the reducer ignores all
/// further relay inputs. The caller must process each returned
/// [`Effect::Subscribe`] and install the signer built from the payload.
///
/// `remote_pubkey` is the bunker's signing key (NIP-44 decrypt); `relay_urls`
/// are the relay URLs from the payload; `sub_id` is the persisted subscription
/// id. `now_secs` is used to set the `since` filter in the REQ frame.
///
/// Returns `Err(String)` when the runtime mutex is poisoned.
pub fn init_restore(
    handle: &Nip46RuntimeHandle,
    sub_id: String,
    local_keys: Keys,
    remote_pubkey: PublicKey,
    relay_urls: Vec<String>,
    now_secs: u64,
) -> Result<Vec<Effect>, String> {
    let relay_urls = canonicalize_relays(&relay_urls);
    let (state, effects) = start_restore(&sub_id, local_keys.clone(), &relay_urls, now_secs);
    let rt = Nip46Runtime {
        state,
        relay_urls,
        sub_id,
        local_keys,
        remote_pubkey,
        user_pubkey: None,
        persistent_sub_registered: false,
    };
    match handle.lock() {
        Ok(mut guard) => {
            *guard = Some(rt);
            Ok(effects)
        }
        Err(_) => Err("runtime handle mutex poisoned".to_string()),
    }
}

/// Drop the session state from the handle.
///
/// Called on account removal or session cancellation (PR-B2 teardown path).
/// Sets the handle to `None` so subsequent relay frames and connected-hook
/// calls are no-ops. Does NOT send `UnregisterPersistentSub` — the caller
/// is responsible for cleaning up the kernel subscription registration.
pub fn clear_runtime(handle: &Nip46RuntimeHandle) {
    if let Ok(mut guard) = handle.lock() {
        *guard = None;
    }
}

/// Persist the remote signer pubkey learned during the handshake.
///
/// Called on [`Effect::SignerReady`] (by the interceptor) so steady-state
/// decode ([`Nip46Runtime::on_relay_text`]) decrypts with the correct key.
///
/// - **bunker**: this equals the pubkey already stored from the URI — a no-op
///   write.
/// - **nostrconnect**: the remote signer pubkey is unknown until the signer's
///   `connect` event arrives, so [`init_nostrconnect`] stores the local pubkey
///   as a placeholder. Without this write-back, `decode_inbound_response`
///   would reject every steady-state response (pubkey mismatch) and sign would
///   hang.
///
/// No-op when the handle is empty or the mutex is poisoned.
pub fn record_signer_ready(handle: &Nip46RuntimeHandle, remote_pubkey: PublicKey) {
    if let Ok(mut guard) = handle.lock() {
        if let Some(rt) = guard.as_mut() {
            rt.remote_pubkey = remote_pubkey;
        }
    }
}

/// #2976 — persist the ACCOUNT's user pubkey learned at `SignerReady`.
///
/// Later health callbacks from this runtime instance (reconnect `connected`,
/// post-`SignerReady` errors) read [`Nip46Runtime::user_pubkey`] so the
/// `signer_state` health is attributed to the correct identity rather than
/// clobbering whatever account happens to be active.
///
/// No-op when the handle is empty or the mutex is poisoned.
pub fn record_user_pubkey(handle: &Nip46RuntimeHandle, user_pubkey: PublicKey) {
    if let Ok(mut guard) = handle.lock() {
        if let Some(rt) = guard.as_mut() {
            rt.user_pubkey = Some(user_pubkey);
        }
    }
}

/// Mark the active session's persistent subscription as registered.
///
/// Used by runtimes that register the initial `Subscribe` effects directly
/// during effect translation.
pub fn mark_persistent_sub_registered(handle: &Nip46RuntimeHandle) {
    if let Ok(mut guard) = handle.lock() {
        if let Some(rt) = guard.as_mut() {
            rt.persistent_sub_registered = true;
        }
    }
}

/// Return the relay/subscription rows that still need persistent registration.
///
/// The returned rows are marked as registered atomically with the read so
/// callers can register them with their kernel without duplicate idle-tick
/// work. `None` means no active runtime, poisoned mutex, or already registered.
#[must_use]
pub fn take_persistent_registration(handle: &Nip46RuntimeHandle) -> Option<(Vec<String>, String)> {
    let mut guard = handle.lock().ok()?;
    let rt = guard.as_mut()?;
    if rt.persistent_sub_registered {
        return None;
    }
    rt.persistent_sub_registered = true;
    Some((rt.relay_urls.clone(), rt.sub_id.clone()))
}

/// Build a fully connected [`Nip46Signer`] from a completed NIP-46 handshake.
///
/// This is shared by the native actor interceptor and the browser runtime
/// bridge. It records the learned remote signer pubkey, captures the session's
/// relay list and local keypair, and installs an [`ActorLaneTransport`] that
/// fans later signer RPCs to every bunker relay.
pub fn complete_signer_from_ready(
    handle: &Nip46RuntimeHandle,
    ready: SignerReady,
    sender: CommandSender,
) -> Result<Nip46Signer, String> {
    let remote_signer_pubkey = PublicKey::from_hex(&ready.remote_signer_pubkey_hex)
        .map_err(|_| "invalid remote signer pubkey in SignerReady".to_string())?;
    let user_pubkey = PublicKey::from_hex(&ready.user_pubkey_hex)
        .map_err(|_| "invalid user pubkey in SignerReady".to_string())?;

    record_signer_ready(handle, remote_signer_pubkey);
    // #2976 — remember WHICH account this session belongs to so later health
    // callbacks (reconnect / error) can attribute their `signer_state` to it.
    record_user_pubkey(handle, user_pubkey);

    let (relay_urls, local_keys) = {
        let guard = handle
            .lock()
            .map_err(|_| "runtime handle mutex poisoned".to_string())?;
        let rt = guard
            .as_ref()
            .ok_or_else(|| "nip46 runtime is not initialized".to_string())?;
        (rt.relay_urls.clone(), rt.local_keys.clone())
    };

    let transport = ActorLaneTransport::new_multi(
        sender,
        local_keys.clone(),
        remote_signer_pubkey,
        relay_urls.clone(),
    );

    let relay_params: String = relay_urls
        .iter()
        .map(|u| format!("&relay={}", nmp_nip46::percent_encode_query_value(u)))
        .collect();
    let synthetic_uri = format!(
        "bunker://{}?{}",
        remote_signer_pubkey.to_hex(),
        relay_params.trim_start_matches('&'),
    );

    let signer_handle = Nip46SignerHandle::from_bunker_uri_with_local_key(
        &synthetic_uri,
        local_keys.secret_key().clone(),
    )
    .map_err(|e| format!("internal: signer handle build failed: {e}"))?;

    Ok(signer_handle.complete(Arc::new(transport), user_pubkey))
}

fn canonicalize_relays(urls: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(urls.len());
    for u in urls {
        let c = canonicalize_relay(u);
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}
