//! Core runtime state for the NIP-46 actor-lane driver.
//!
//! [`Nip46Runtime`] owns the transport-agnostic [`SessionState`] reducer and
//! all session metadata (relay URLs, subscription ID, local keypair, remote
//! pubkey). It is held behind a [`Nip46RuntimeHandle`] so the interceptor,
//! connected hook, and actor-lane transport can each hold an `Arc` clone and
//! lock it independently.
//!
//! ## Multi-relay support
//!
//! A `bunker://` URI may include multiple relay URLs.  The runtime stores all
//! of them in `relay_urls`.  Inbound frames are accepted from any relay in the
//! list; outbound frames are fanned to ALL relays via
//! [`crate::transport::ActorLaneTransport::new_multi`].
//!
//! ## Steady-state decode
//!
//! Once the handshake completes (`Phase::Done`), the reducer ignores all
//! further relay inputs.  `on_relay_text` therefore also tries
//! `nmp_nip46::decode_inbound_response` on any EVENT frame addressed to our
//! subscription and returns the decrypted body as the second element of its
//! `(effects, decoded)` pair.  The caller (`Nip46Interceptor`) delivers the
//! decoded body via `CommandSender::deliver_signer_response`.
//!
//! ## Session lifecycle
//!
//! 1. **Start**: call [`init_bunker`] or [`init_nostrconnect`]. These store
//!    the [`SessionState`] and return the initial [`Effect`]s (Subscribe +
//!    Progress + SendFrame). The caller must register the persistent sub and
//!    send the initial outbound frames.
//! 2. **Restore**: call [`init_restore`] to reseed from a `SignerPayload::Nip46`
//!    without re-running the handshake. Posts REQ to each relay and expects the
//!    caller to install the signer via `add_signer`.
//! 3. **Relay text**: [`Nip46Runtime::on_relay_text`] — called by the
//!    interceptor on every inbound frame from any bunker relay.
//! 4. **Tick**: [`Nip46Runtime::tick`] — called from `on_idle_tick` to
//!    enforce the 60 s per-step deadline.
//! 5. **Reconnect**: [`Nip46Runtime::on_relay_connected`] — called by the
//!    connected hook; returns REQ replay effects for the SPECIFIC relay.
//! 6. **Clear**: [`clear_runtime`] — drops session state (PR-B2 teardown).

use nmp_nip46::decode_inbound_response;
use nmp_nip46::reducer::SessionState;
use nmp_nip46::Effect;
use nmp_nip46::{build_req_frame, start_bunker, start_restore, start_nostrconnect};
use nostr::{Keys, PublicKey};
use std::sync::{Arc, Mutex};

// ─── Nip46Runtime ────────────────────────────────────────────────────────────

/// Live state for a single NIP-46 session on the actor relay lane.
///
/// Held behind [`Nip46RuntimeHandle`]; all field access goes through the mutex.
pub struct Nip46Runtime {
    /// Transport-agnostic handshake state machine.
    pub(crate) state: SessionState,
    /// All bunker relay URLs — frames from any of these are accepted.
    pub(crate) relay_urls: Vec<String>,
    /// Subscription id used in REQ frames.
    pub(crate) sub_id: String,
    /// Local ephemeral keypair (NIP-44 encrypt/decrypt + event signing).
    pub(crate) local_keys: Keys,
    /// Remote signer pubkey (bunker or nostrconnect signer app).
    pub(crate) remote_pubkey: PublicKey,
    /// Whether `kernel.register_persistent_sub` has been called for this session.
    /// Set to `true` by the interceptor's `on_idle_tick` on first registration.
    pub(crate) persistent_sub_registered: bool,
}

impl Nip46Runtime {
    /// Drive the step-deadline timer.  Returns an [`Effect::Error`] when the
    /// deadline elapses; empty on still-live sessions and `Done` phase.
    pub fn tick(&mut self, now_secs: u64) -> Vec<Effect> {
        self.state.tick(now_secs)
    }

    /// Feed a raw relay text frame from any of the bunker relays.
    ///
    /// Returns `(effects, decoded)`:
    /// - `effects`: reducer output (handshake progress, SignerReady, etc.).
    /// - `decoded`: `Some(body)` when the frame is a kind:24133 EVENT addressed
    ///   to our subscription in `Done` phase and NIP-44 decryption succeeds.
    ///   The caller must deliver this via `CommandSender::deliver_signer_response`.
    ///
    /// Frames from relays NOT in `relay_urls` return `([], None)` (D6 — silent
    /// ignore).
    pub fn on_relay_text(
        &mut self,
        relay_url: &str,
        text: &str,
        now_secs: u64,
    ) -> (Vec<Effect>, Option<String>) {
        if !self.relay_urls.iter().any(|u| u == relay_url) {
            return (Vec::new(), None);
        }
        let effects = self.state.on_relay_text(text, now_secs);

        // Steady-state decode: when the reducer returns empty (Done phase or
        // non-matching frame), try to decode a kind:24133 EVENT addressed to
        // our subscription so the registered Nip46Signer can resolve its parked
        // sign operation.  Non-EVENT frames and frames for other sub_ids return
        // None quickly.
        if effects.is_empty() {
            let decoded =
                try_decode_steady_state(text, &self.sub_id, &self.local_keys, self.remote_pubkey);
            return (Vec::new(), decoded);
        }
        (effects, None)
    }

    /// React to a bunker relay (re)connecting.
    ///
    /// Returns one [`Effect::Subscribe`] for the SPECIFIC reconnecting relay
    /// (not just the primary) so every relay in the session gets its own REQ
    /// on reconnect.  The connected hook registers each such frame as the
    /// worker's reconnect preamble for that relay.
    ///
    /// Frames from relays NOT in `relay_urls` return empty (D6).
    pub fn on_relay_connected(
        &mut self,
        relay_url: &str,
        _is_reconnect: bool,
        now_secs: u64,
    ) -> Vec<Effect> {
        if !self.relay_urls.iter().any(|u| u == relay_url) {
            return Vec::new();
        }
        // Bypass state.on_relay_connected (which hardcodes the primary relay URL).
        // Generate the Subscribe effect directly for the SPECIFIC reconnecting relay.
        let pubkey_hex = self.local_keys.public_key().to_hex();
        let frame = build_req_frame(&self.sub_id, &pubkey_hex, now_secs);
        vec![Effect::Subscribe { relay_url: relay_url.to_string(), frame }]
    }

    /// The primary bunker relay URL (first in `relay_urls`, or empty string if
    /// the list is empty).  For single-relay sessions this is the only URL.
    pub fn relay_url(&self) -> &str {
        self.relay_urls.first().map(String::as_str).unwrap_or("")
    }

    /// All bunker relay URLs for this session.
    pub fn relay_urls(&self) -> &[String] {
        &self.relay_urls
    }

    /// The subscription id (e.g. `"nip46-<local_pubkey_prefix>"`).
    pub fn sub_id(&self) -> &str {
        &self.sub_id
    }

    /// Local ephemeral keypair for this session.
    pub fn local_keys(&self) -> &Keys {
        &self.local_keys
    }

    /// Remote signer pubkey.
    pub fn remote_pubkey(&self) -> PublicKey {
        self.remote_pubkey
    }
}

// ─── Nip46RuntimeHandle ──────────────────────────────────────────────────────

/// Shared handle to an optional [`Nip46Runtime`].
///
/// `None` when no session is active.  All components (interceptor, connected
/// hook, actor-lane transport) hold their own `Arc` clone and lock the mutex
/// for each operation; no lock is held across `await` boundaries (no async
/// code here) or across any actor-thread kernel call.
pub type Nip46RuntimeHandle = Arc<Mutex<Option<Nip46Runtime>>>;

/// Construct a fresh, empty [`Nip46RuntimeHandle`].
#[must_use]
pub fn new_nip46_runtime_handle() -> Nip46RuntimeHandle {
    Arc::new(Mutex::new(None))
}

// ─── Session initialisation helpers ──────────────────────────────────────────

/// Initialise the handle with a `bunker://` session.
///
/// `relay_url` is the single relay URL from the parsed bunker URI. For
/// multi-relay URIs, pass the slice-derived `relay_urls` vector instead.
///
/// Returns the initial [`Effect`]s (Subscribe + Progress + SendFrame).
/// The caller MUST:
///
/// 1. Call `kernel.register_persistent_sub(relay_url, sub_id)` (or leave
///    `persistent_sub_registered = false` so the interceptor registers on
///    first idle tick).
/// 2. Convert the [`Effect::Subscribe`] and [`Effect::SendFrame`] results
///    into `OutboundMessage`s and deliver them to the relay.
///
/// Returns `Err(String)` when the runtime mutex is poisoned or the URI is
/// malformed.
pub fn init_bunker(
    handle: &Nip46RuntimeHandle,
    sub_id: String,
    local_keys: Keys,
    remote_pubkey: PublicKey,
    relay_url: String,
    secret: Option<&str>,
    perms: Option<&str>,
    now_secs: u64,
) -> Result<Vec<Effect>, String> {
    let relay_urls = vec![relay_url.clone()];
    let (state, effects) = start_bunker(
        &sub_id,
        local_keys.clone(),
        remote_pubkey,
        relay_url,
        secret,
        perms,
        now_secs,
    );
    let rt = Nip46Runtime {
        state,
        relay_urls,
        sub_id,
        local_keys,
        remote_pubkey,
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
    // The remote pubkey is learned during the nostrconnect handshake; use
    // the local pubkey as a placeholder that the `SignerReady` effect will
    // replace when the interceptor builds the ActorLaneTransport.
    let local_pk = local_keys.public_key();
    let rt = Nip46Runtime {
        state,
        relay_urls,
        sub_id,
        local_keys,
        remote_pubkey: local_pk, // placeholder; overwritten on SignerReady
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
/// further relay inputs.  The caller must:
///
/// 1. Process each returned [`Effect::Subscribe`] (register persistent sub +
///    set reconnect preamble).
/// 2. Call `CommandSender::add_signer(SignerSource::RemoteHandle(signer))` to
///    install the signer built from the payload.
///
/// `remote_pubkey` is the bunker's signing key (NIP-44 decrypt); `relay_urls`
/// are the relay URLs from the payload; `sub_id` is the persisted subscription
/// id.  `now_secs` is used to set the `since` filter in the REQ frame.
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
    let (state, effects) = start_restore(&sub_id, local_keys.clone(), &relay_urls, now_secs);
    let rt = Nip46Runtime {
        state,
        relay_urls,
        sub_id,
        local_keys,
        remote_pubkey,
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
/// calls are no-ops.  Does NOT send `UnregisterPersistentSub` — the caller
/// is responsible for cleaning up the kernel subscription registration.
pub fn clear_runtime(handle: &Nip46RuntimeHandle) {
    if let Ok(mut guard) = handle.lock() {
        *guard = None;
    }
}

// ─── steady-state decode helper ──────────────────────────────────────────────

/// Try to NIP-44 decrypt a steady-state kind:24133 response.
///
/// Only processes `["EVENT", sub_id, event]` frames addressed to our
/// subscription.  All other frames return `None` quickly.
fn try_decode_steady_state(
    text: &str,
    sub_id: &str,
    local_keys: &Keys,
    remote_pubkey: PublicKey,
) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "EVENT" {
        return None;
    }
    if arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let event = arr.get(2)?;
    decode_inbound_response(event, local_keys, remote_pubkey)
}
