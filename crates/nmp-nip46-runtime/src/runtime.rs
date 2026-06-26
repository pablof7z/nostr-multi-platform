//! Core runtime state for the NIP-46 actor-lane driver.
//!
//! [`Nip46Runtime`] owns the transport-agnostic [`SessionState`] reducer and
//! all session metadata (relay URL, subscription ID, local keypair, remote
//! pubkey). It is held behind a [`Nip46RuntimeHandle`] so the interceptor,
//! connected hook, and actor-lane transport can each hold an `Arc` clone and
//! lock it independently.
//!
//! ## Session lifecycle
//!
//! 1. **Start**: call [`Nip46Runtime::init_bunker`] or
//!    [`Nip46Runtime::init_nostrconnect`].  These store the [`SessionState`]
//!    and return the initial [`Effect`]s (Subscribe + Progress + SendFrame).
//!    The caller (a `ProtocolCommand` body in production; a test helper in
//!    isolation tests) must
//!    - call `kernel.register_persistent_sub(relay_url, sub_id)` so the
//!      subscription survives EOSE auto-CLOSE, and
//!    - send the initial outbound frames.
//! 2. **Relay text**: [`Nip46Runtime::on_relay_text`] — called by the
//!    interceptor on every inbound frame from the bunker relay.
//! 3. **Tick**: [`Nip46Runtime::tick`] — called from `on_idle_tick` to
//!    enforce the 60s per-step deadline.
//! 4. **Reconnect**: [`Nip46Runtime::on_relay_connected`] — called by the
//!    connected hook; returns REQ replay effects.
//! 5. **Reset**: [`Nip46Runtime::clear`] — clears session state.

use nmp_nip46::reducer::SessionState;
use nmp_nip46::Effect;
use nmp_nip46::{start_bunker, start_nostrconnect};
use nostr::{Keys, PublicKey};
use std::sync::{Arc, Mutex};

// ─── Nip46Runtime ────────────────────────────────────────────────────────────

/// Live state for a single NIP-46 session on the actor relay lane.
///
/// Held behind [`Nip46RuntimeHandle`]; all field access goes through the mutex.
pub struct Nip46Runtime {
    /// Transport-agnostic handshake state machine.
    pub(crate) state: SessionState,
    /// Bunker relay URL (filters inbound frames in the interceptor).
    pub(crate) relay_url: String,
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

    /// Feed a raw relay text frame from the bunker relay.
    ///
    /// Filters by `relay_url`; stray frames from other relays are silently
    /// ignored (empty `Vec`).
    pub fn on_relay_text(&mut self, relay_url: &str, text: &str, now_secs: u64) -> Vec<Effect> {
        if relay_url != self.relay_url {
            return Vec::new();
        }
        self.state.on_relay_text(text, now_secs)
    }

    /// React to the bunker relay (re)connecting.
    ///
    /// Returns the REQ subscription replay effects so the connected hook can
    /// register them as the worker's reconnect preamble via
    /// `CommandSender::set_reconnect_preamble`.
    ///
    /// The 60 s step deadline is NO LONGER armed here (Guardrail 2).  It is
    /// armed by `on_relay_text` when the relay sends `EOSE` for our
    /// subscription, i.e. the point at which the relay is actually ready to
    /// deliver matching EVENTs.  The handshake-start deadline set by
    /// `start_bunker` / `start_nostrconnect` remains as a fallback floor so a
    /// relay that never sends EOSE still bounds a stuck handshake.
    pub fn on_relay_connected(&mut self, relay_url: &str, is_reconnect: bool, now_secs: u64) -> Vec<Effect> {
        if relay_url != self.relay_url {
            return Vec::new();
        }
        self.state.on_relay_connected(is_reconnect, now_secs)
    }

    /// The relay URL this session is bound to.
    pub fn relay_url(&self) -> &str {
        &self.relay_url
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
/// Returns the initial [`Effect`]s (Subscribe + Progress + SendFrame).
/// The caller MUST:
///
/// 1. Call `kernel.register_persistent_sub(relay_url, sub_id)` (or set
///    `persistent_sub_registered = false` so the interceptor registers on
///    first idle tick).
/// 2. Convert the [`Effect::Subscribe`] and [`Effect::SendFrame`] results
///    into `OutboundMessage`s and deliver them to the relay.
///
/// Returns `Err(String)` when the existing session would be silently dropped
/// (caller must cancel it first) or when the URI is malformed.
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
    let (state, effects) = start_bunker(
        &sub_id,
        local_keys.clone(),
        remote_pubkey,
        relay_url.clone(),
        secret,
        perms,
        now_secs,
    );
    let rt = Nip46Runtime {
        state,
        relay_url,
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
    let (uri, state, effects) = start_nostrconnect(
        &sub_id,
        local_keys.clone(),
        relay_url.clone(),
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
        relay_url,
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
