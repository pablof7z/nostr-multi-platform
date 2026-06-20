//! NIP-46 broker C-ABI adapter.
//!
//! `nmp-signer-broker` owns app-neutral transport and emits `BrokerEvent`s.
//! This module is the app/core adapter: it registers the kernel bunker hook,
//! translates broker events into actor commands, and keeps the existing C
//! symbol names stable for native shells.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Arc;

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nmp_core::{ActorCommand, BunkerHookRequest, RemoteSignerHandle};
use nmp_signer_broker::{percent_encode_query_value, BrokerEvent, BunkerBroker};
use nmp_signer_iface::SignerOp;
use nmp_signers::Nip46Signer;

use super::{app_ref, NmpApp, NmpConfigStatus};

/// Initialise the NIP-46 broker for `app`. After this call, any
/// `nmp_app_signin_bunker` dispatch routes through the broker's handshake state
/// machine. Idempotent per app: repeated calls keep the existing per-app
/// broker. ADR-0052 §D3 — the broker handle and the bunker hook are **per-app**
/// (no `GLOBAL_BROKER` / `register_bunker_hook` process-global), so two
/// `NmpApp`s in one process have independent brokers and a freed-then-recreated
/// app re-initialises cleanly.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()` and not yet
/// freed via `nmp_app_free`. Passing null is safe: returns
/// [`NmpConfigStatus::NullApp`].
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_signer_broker_init(app: *mut NmpApp) -> u32 {
    let Some(app) = app_ref(app) else {
        return NmpConfigStatus::NullApp.code();
    };
    if let Err(status) =
        app.ensure_prestart_config("signer_broker", "bunker_hook", "nmp_signer_broker_init")
    {
        return status.code();
    }
    let tx = app.actor_sender();
    let broker = app.signer_broker_get_or_init(|| {
        let event_tx = tx.clone();
        let broker = BunkerBroker::new(Arc::new(move |event| {
            handle_broker_event(&event_tx, event);
        }));
        // ADR-0050 §D3b — install the completion sink: every decrypted
        // steady-state kind:24133 RPC reply is routed back to the actor as a
        // `DeliverSignerResponse` command (waking the actor via the single
        // inbox, §D3a) instead of resolving the parked op on the broker's
        // dispatcher thread. nmp-core's dispatch arm fans the body out to the
        // remote handles. D0: the broker sees only this opaque `Fn(String)`.
        let sink_tx = tx.clone();
        broker.set_completion_sink(Arc::new(move |response_json: String| {
            let _ = sink_tx.send(ActorCommand::DeliverSignerResponse { response_json });
        }));
        broker
    });
    // ADR-0052 §D3 — install the broker hook into THIS app's per-app slot
    // (the actor's `IdentityRuntime` reads the matching `Arc` clone). The
    // broker response routes back to the originating app structurally via the
    // per-app `event_tx`/`sink_tx` captured above — no correlation token.
    let broker_for_hook = Arc::clone(&broker);
    app.install_bunker_hook(Arc::new(move |request| match request {
        BunkerHookRequest::Connect { uri } => broker_for_hook.start_handshake(uri),
        BunkerHookRequest::Restore { payload_json } => {
            broker_for_hook.restore_session(payload_json);
        }
    }));
    NmpConfigStatus::Ok.code()
}

fn handle_broker_event(tx: &nmp_core::CommandSender, event: BrokerEvent) {
    let cmd = match event {
        BrokerEvent::Progress { stage, message } => {
            ActorCommand::BunkerHandshakeProgress { stage, message }
        }
        // The broker completed a NIP-46 handshake. Route the resolved signer
        // back through the unified `AddSigner` command. The broker adapter
        // cannot see the `make_active` flag the originating `BunkerUri` command
        // stashed in the actor's `IdentityRuntime`; it requests activation, and
        // the actor reconciles that with the stashed value (taking either
        // signal, and always activating when no account is active).
        BrokerEvent::SignerReady { signer } => ActorCommand::AddSigner {
            source: nmp_core::SignerSource::RemoteHandle(Box::new(ArcRemoteSigner(signer))),
            make_active: true,
        },
        // V-14 step b: relay-layer connection state. Routes through the actor
        // (D4 — actor is sole writer of the `bunker_connection_state` slot).
        BrokerEvent::ConnectionStateChanged { state, reason } => {
            ActorCommand::BunkerConnectionStateChanged { state, reason }
        }
    };
    let _ = tx.send(cmd);
}

/// Cancel an in-flight bunker handshake, if any. Idempotent and null-safe.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()`. Passing null is
/// safe. ADR-0052 §D3 — reads THIS app's per-app broker (no process-global).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_cancel_bunker_handshake(app: *mut NmpApp) {
    if let Some(broker) = app_ref(app).and_then(NmpApp::signer_broker) {
        broker.cancel();
    }
}

/// Return a freshly generated `nostrconnect://` URI string. The caller must
/// free the returned pointer via `nmp_free_string`. Returns null if the
/// broker is not yet initialised, no write relay is configured, or string
/// allocation fails.
///
/// D3: relay selection is Rust-owned — the URI embeds the first
/// write-capable relay from the kernel's relay config
/// (`NmpApp::nostrconnect_relay_url`). The caller supplies only optional
/// platform callback information; it does not choose the relay.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_nostrconnect_uri(
    app: *mut NmpApp,
    callback_scheme: *const c_char,
) -> *mut c_char {
    // D3 / V-65: relay is always Rust-chosen; there is no caller override.
    // `None` means no write relay is configured — return null so the UI can
    // surface a "add a relay first" prompt rather than using any hardcoded URL.
    let Some(relay) = app_ref(app).and_then(NmpApp::nostrconnect_relay_url) else {
        return std::ptr::null_mut();
    };
    let callback: Option<&str> = if callback_scheme.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null means a valid C string for the
        // call duration. Invalid UTF-8 degrades to no callback.
        match unsafe { CStr::from_ptr(callback_scheme).to_str() } {
            Ok(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    };
    let Some(broker) = app_ref(app).and_then(NmpApp::signer_broker) else {
        return std::ptr::null_mut();
    };
    // #1493 P9 — the NIP-46 perm request is APP-SUPPLIED. NMP supplies no
    // default: when the host registered none, `perms` is `None` and the broker
    // omits the `&perms=` parameter entirely rather than baking in a
    // framework-chosen kind set.
    let perms = app_ref(app).and_then(NmpApp::nostrconnect_perms);
    let mut uri = broker.start_nostrconnect_handshake(relay, perms);
    if let Some(scheme) = callback {
        uri.push_str("&callback=");
        uri.push_str(&percent_encode_query_value(scheme));
    }
    match CString::new(uri) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Adapter: `Box<dyn RemoteSignerHandle>` from an `Arc<Nip46Signer>`.
#[derive(Debug)]
struct ArcRemoteSigner(Arc<Nip46Signer>);

impl RemoteSignerHandle for ArcRemoteSigner {
    fn pubkey_hex(&self) -> String {
        RemoteSignerHandle::pubkey_hex(&*self.0)
    }

    fn signer_kind(&self) -> &'static str {
        RemoteSignerHandle::signer_kind(&*self.0)
    }

    fn persistence_payload_json(&self) -> Option<String> {
        RemoteSignerHandle::persistence_payload_json(&*self.0)
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        RemoteSignerHandle::sign(&*self.0, unsigned)
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        RemoteSignerHandle::nip44_encrypt(&*self.0, recipient_pubkey, plaintext)
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        RemoteSignerHandle::nip44_decrypt(&*self.0, sender_pubkey, ciphertext)
    }

    fn deliver_response(&self, response_json: &str) {
        self.0.ingest_rpc_response(response_json);
    }

    fn disconnect(&self) {
        self.0.drain_pending_with_error("signer disconnected");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

    #[derive(Debug, Default)]
    struct AcceptingTransport;

    impl Nip46Transport for AcceptingTransport {
        fn send_rpc(&self, _rpc: Nip46Rpc) -> Result<(), SignerError> {
            Ok(())
        }
    }

    #[test]
    fn arc_remote_signer_disconnect_drains_pending_sign() {
        let local = nmp_signers::SecretKey::from_hex(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .expect("valid secret hex");
        let remote_user = nmp_signers::SecretKey::from_hex(
            "0000000000000000000000000000000000000000000000000000000000000002",
        )
        .expect("valid secret hex");
        let remote_user_pubkey = nostr::Keys::new(remote_user).public_key();
        let uri = format!(
            "bunker://{}?relay=wss://relay.example.com",
            nostr::Keys::new(local.clone()).public_key().to_hex()
        );
        let handle = nmp_signers::Nip46SignerHandle::from_bunker_uri_with_local_key(&uri, local)
            .expect("parse bunker uri");
        let signer = Arc::new(handle.complete(Arc::new(AcceptingTransport), remote_user_pubkey));

        let wrapper = ArcRemoteSigner(Arc::clone(&signer));
        let unsigned = UnsignedEvent {
            pubkey: remote_user_pubkey.to_hex(),
            kind: 1,
            tags: vec![],
            content: "in flight".to_string(),
            created_at: 1_700_000_000,
        };
        let op = RemoteSignerHandle::sign(&wrapper, &unsigned);

        RemoteSignerHandle::disconnect(&wrapper);

        let err = op
            .wait(Duration::from_millis(200))
            .expect_err("disconnect must surface as Err, not a timeout");
        assert!(
            matches!(err, SignerError::Rejected(ref m) if m.contains("disconnect")),
            "expected Rejected(disconnect...), got {err:?}"
        );
    }
}
