//! NIP-46 actor-lane native runtime adapter (PR-B2: broker deleted).
//!
//! ## Design
//!
//! `NmpApp::init_signer_broker` is the **config-phase** entry point:
//!
//! 1. Calls `ensure_prestart_config` to guard against post-start calls.
//! 2. Calls `register_nip46(app, tx)` to install the `Nip46Interceptor` and
//!    `Nip46ConnectedHook` on the app's substrate registrar slots. Returns a
//!    `Nip46RuntimeHandle` stored on the `NmpApp`.
//! 3. Installs a bunker hook that the actor calls on `StartBunkerHandshake`:
//!    - `Connect { uri }` → `init_bunker` + `deliver_init_effects`.
//!    - `Restore { payload_json }` → `restore_nip46_from_payload`.
//!
//! `NmpApp::cancel_bunker_handshake` calls `cancel_nip46_session` (clears the
//! runtime + posts `UnregisterPersistentSub` for each relay).
//!
//! `NmpApp::nostrconnect_uri` calls `init_nostrconnect` + `deliver_init_effects`
//! and returns the `nostrconnect://` URI to the caller synchronously.

use std::sync::Arc;

use nmp_core::BunkerHookRequest;
use nmp_nip46::percent_encode_query_value;

/// Wall-clock Unix seconds for NIP-44 timestamps.  Must not block or touch
/// the actor; all callers are on the hook thread (D8-safe).
fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
use nmp_nip46_runtime::{
    cancel_nip46_session, deliver_init_effects, init_bunker, init_nostrconnect, make_sub_id,
    register_nip46, restore_nip46_from_payload, Nip46RuntimeHandle,
};
use nmp_signers::parse_bunker_uri;
use nostr::{Keys, PublicKey};

use super::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// Initialise the NIP-46 actor-lane runtime for this app.
    ///
    /// After this call, any bunker sign-in dispatch routes through the
    /// actor-lane runtime's handshake state machine. The method is idempotent
    /// and first-writer-wins per app; a second pre-start call returns
    /// [`NmpConfigStatus::Ok`] without re-registering hooks. A call after
    /// start returns [`NmpConfigStatus::AlreadyStarted`].
    pub fn init_signer_broker(&self) -> NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("signer_broker", "bunker_hook", "init_signer_broker")
        {
            return status;
        }

        if self
            .nip46_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            return NmpConfigStatus::Ok;
        }

        let tx = self.actor_sender();
        let handle = register_nip46(self, tx.clone());

        *self
            .nip46_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle.clone());

        let handle_for_hook = handle;
        let tx_for_hook = tx.clone();
        self.install_bunker_hook(Arc::new(move |request| match request {
            BunkerHookRequest::Connect { uri } => {
                start_bunker_connect(&handle_for_hook, &tx_for_hook, uri);
            }
            BunkerHookRequest::Restore { payload_json } => {
                let now = now_unix_secs();
                if let Err(e) = restore_nip46_from_payload(
                    &handle_for_hook,
                    &payload_json,
                    tx_for_hook.clone(),
                    now,
                ) {
                    tracing::warn!(error = %e, "nip46-runtime: restore from payload failed");
                }
            }
        }));

        NmpConfigStatus::Ok
    }

    /// Cancel an in-flight bunker handshake, if any.
    pub fn cancel_bunker_handshake(&self) {
        let handle = self
            .nip46_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(handle) = handle else { return };
        let tx = self.actor_sender();
        cancel_nip46_session(&handle, &tx);
    }

    /// Return a freshly generated `nostrconnect://` URI string.
    ///
    /// D3: relay selection is Rust-owned. The optional `callback_scheme` is
    /// platform callback metadata only; it does not choose the relay.
    pub fn nostrconnect_uri(&self, callback_scheme: Option<&str>) -> Option<String> {
        let relay = self.nostrconnect_relay_url()?;
        let handle = self
            .nip46_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(handle) = handle else {
            tracing::warn!("nostrconnect_uri: called before init_signer_broker");
            return None;
        };

        let perms = self.nostrconnect_perms();
        let local_keys = Keys::generate();
        let sub_id = make_sub_id(local_keys.public_key());
        let expected_secret: String = Keys::generate().public_key().to_hex()[..16].to_string();
        let tx = self.actor_sender();
        let now = now_unix_secs();

        match init_nostrconnect(
            &handle,
            sub_id,
            local_keys,
            relay,
            expected_secret,
            perms,
            "nmp",
            now,
        ) {
            Ok((mut uri, effects)) => {
                deliver_init_effects(effects, &tx);
                if let Some(scheme) = callback_scheme.filter(|scheme| !scheme.is_empty()) {
                    uri.push_str("&callback=");
                    uri.push_str(&percent_encode_query_value(scheme));
                }
                Some(uri)
            }
            Err(e) => {
                tracing::warn!(error = %e, "nostrconnect_uri: init_nostrconnect failed");
                None
            }
        }
    }
}

/// Parse `uri` as a `bunker://` URI, initialise the runtime, and deliver the
/// initial effects as actor commands.  Called synchronously on the actor thread
/// from the bunker hook — must be fast and non-blocking.
fn start_bunker_connect(
    handle: &Nip46RuntimeHandle,
    sender: &nmp_core::CommandSender,
    uri: String,
) {
    let parsed = match parse_bunker_uri(&uri) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %format!("{e:?}"), "nip46-runtime: bad bunker URI");
            sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("invalid bunker URI: {e:?}")),
            );
            return;
        }
    };
    let local_keys = Keys::generate();
    let sub_id = make_sub_id(local_keys.public_key());
    let remote_pubkey = match PublicKey::from_hex(&parsed.remote_pubkey_hex) {
        Ok(pk) => pk,
        Err(e) => {
            tracing::warn!(error = %e, "nip46-runtime: bad remote pubkey in bunker URI");
            sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("invalid remote pubkey: {e}")),
            );
            return;
        }
    };
    let relay_urls = parsed.relays.clone();
    let secret: Option<String> = parsed.secret.as_ref().map(|zs| zs.as_str().to_string());
    let perms = parsed.permissions.clone();
    let now = now_unix_secs();

    match init_bunker(
        handle,
        sub_id,
        local_keys,
        remote_pubkey,
        relay_urls,
        secret.as_deref(),
        perms.as_deref(),
        now,
    ) {
        Ok(effects) => deliver_init_effects(effects, sender),
        Err(e) => {
            tracing::warn!(error = %e, "nip46-runtime: init_bunker failed");
            sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("bunker init error: {e}")),
            );
        }
    }
}
