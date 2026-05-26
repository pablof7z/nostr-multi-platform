//! NIP-46 broker C-ABI adapter.
//!
//! `nmp-signer-broker` owns app-neutral transport and emits `BrokerEvent`s.
//! This module is the app/core adapter: it registers the kernel bunker hook,
//! translates broker events into actor commands, and keeps the existing C
//! symbol names stable for native shells.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nmp_core::{
    register_bunker_hook, ActorCommand, BunkerHookRequest, RemoteSignerHandle,
    NOSTRCONNECT_DEFAULT_RELAY_URL,
};
use nmp_signer_broker::{percent_encode_query_value, BrokerEvent, BunkerBroker};
use nmp_signer_iface::SignerOp;
use nmp_signers::Nip46Signer;

use super::{app_ref, NmpApp};

/// Process-global broker handle. The bunker hook closure also holds a strong
/// `Arc<BunkerBroker>`; this exists so the cancel and URI symbols can reach
/// the broker without a second registration mechanism.
static GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>> = OnceLock::new();

/// Initialise the NIP-46 broker. After this call, any `nmp_app_signin_bunker`
/// dispatch routes through the broker's handshake state machine. Idempotent:
/// repeated calls after the first keep the existing process-global broker.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()` and not yet
/// freed via `nmp_app_free`. Passing null is safe: the function is a no-op.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_signer_broker_init(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let tx = app.actor_sender();
    let _ = GLOBAL_BROKER.get_or_init(|| {
        let broker = BunkerBroker::new(Arc::new(move |event| {
            handle_broker_event(&tx, event);
        }));
        let broker_for_hook = Arc::clone(&broker);
        register_bunker_hook(Arc::new(move |request| match request {
            BunkerHookRequest::Connect { uri } => broker_for_hook.start_handshake(uri),
            BunkerHookRequest::Restore { payload_json } => {
                broker_for_hook.restore_session(payload_json);
            }
        }));
        broker
    });
}

fn handle_broker_event(tx: &Sender<ActorCommand>, event: BrokerEvent) {
    let cmd = match event {
        BrokerEvent::Progress { stage, message } => {
            ActorCommand::BunkerHandshakeProgress { stage, message }
        }
        BrokerEvent::SignerReady { signer } => ActorCommand::AddRemoteSigner {
            handle: Box::new(ArcRemoteSigner(signer)),
        },
    };
    let _ = tx.send(cmd);
}

/// Cancel an in-flight bunker handshake, if any. Idempotent and null-safe.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()`. Passing null is
/// safe. The argument is retained for ABI stability and future per-app brokers.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_cancel_bunker_handshake(_app: *mut NmpApp) {
    if let Some(broker) = GLOBAL_BROKER.get() {
        broker.cancel();
    }
}

/// Return a freshly generated `nostrconnect://` URI string. The caller must
/// free the returned pointer via `nmp_broker_free_string`. Returns null if the
/// broker is not yet initialised or if string allocation fails.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_nostrconnect_uri(
    app: *mut NmpApp,
    relay_url: *const c_char,
    callback_scheme: *const c_char,
) -> *mut c_char {
    let relay = relay_url_from_arg_or_app(app, relay_url);
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
    let Some(broker) = GLOBAL_BROKER.get() else {
        return std::ptr::null_mut();
    };
    let mut uri = broker.start_nostrconnect_handshake(relay);
    if let Some(scheme) = callback {
        uri.push_str("&callback=");
        uri.push_str(&percent_encode_query_value(scheme));
    }
    match CString::new(uri) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn relay_url_from_arg_or_app(app: *mut NmpApp, relay_url: *const c_char) -> String {
    if !relay_url.is_null() {
        // SAFETY: caller guarantees non-null means a valid C string for the
        // call duration.
        if let Ok(relay) = unsafe { CStr::from_ptr(relay_url).to_str() } {
            if !relay.is_empty() {
                return relay.to_string();
            }
        }
    }
    app_ref(app).map_or_else(
        || NOSTRCONNECT_DEFAULT_RELAY_URL.to_string(),
        NmpApp::nostrconnect_relay_url,
    )
}

/// Free a string returned by `nmp_app_nostrconnect_uri`. Null-safe.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_broker_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was created by CString::into_raw() in this module.
    unsafe { drop(CString::from_raw(ptr)) };
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

    fn deliver_rpc_response(&self, response_json: &str) {
        self.0.ingest_rpc_response(response_json);
    }

    fn disconnect(&self) {
        self.0.drain_pending_with_error("signer disconnected");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::time::Duration;

    use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

    #[test]
    fn explicit_relay_arg_still_overrides_kernel_selection() {
        let relay = CString::new("wss://explicit.example").expect("valid CString");

        assert_eq!(
            relay_url_from_arg_or_app(std::ptr::null_mut(), relay.as_ptr()),
            "wss://explicit.example"
        );
    }

    #[test]
    fn null_app_null_relay_uses_core_default() {
        assert_eq!(
            relay_url_from_arg_or_app(std::ptr::null_mut(), std::ptr::null()),
            NOSTRCONNECT_DEFAULT_RELAY_URL
        );
    }

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
