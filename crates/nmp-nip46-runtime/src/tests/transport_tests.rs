//! `ActorLaneTransport` unit tests.
//!
//! Verifies:
//! - `send_rpc` posts an `EnqueueOutbound` frame without blocking
//!   (fire-and-forget contract).
//! - Multiple `send_rpc` calls post frames in the order they were called
//!   (channel FIFO ordering guarantee for multi-step RPCs).
//! - `ActorLaneTransport` is `Send + Sync` (required for `Arc<dyn Nip46Transport>`).

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use nmp_core::actor::ActorCommand;
    use nmp_core::{ActorMail, CommandSender};
    use nmp_network::role::RelayRole;
    use nmp_signer_iface::{Nip46Rpc, Nip46Transport};
    use nostr::Keys;

    use crate::transport::ActorLaneTransport;

    // ─── helper: spy channel ─────────────────────────────────────────────────

    fn make_sender() -> (CommandSender, mpsc::Receiver<ActorMail>) {
        let (tx, rx) = mpsc::channel::<ActorMail>();
        (CommandSender::new(tx), rx)
    }

    fn drain_enqueue_outbound(rx: &mpsc::Receiver<ActorMail>) -> Vec<(RelayRole, String, String)> {
        let mut frames = Vec::new();
        while let Ok(mail) = rx.recv_timeout(Duration::from_millis(100)) {
            if let ActorMail::Command(ActorCommand::EnqueueOutbound { role, relay_url, text }) = mail
            {
                frames.push((role, relay_url, text));
            }
        }
        frames
    }

    // ─── helper: minimal Nip46Rpc ────────────────────────────────────────────

    fn make_rpc(id: &str, body: &str) -> Nip46Rpc {
        Nip46Rpc {
            id: id.to_string(),
            body_json: String::new(),
            body_json_to_encrypt: body.to_string(),
            relays: Vec::new(),
            remote_pubkey_hex: String::new(),
        }
    }

    // ─── tests ───────────────────────────────────────────────────────────────

    /// `send_rpc` must post an `EnqueueOutbound` frame to the actor inbox and
    /// return `Ok(())` without blocking.
    #[test]
    fn send_rpc_fire_and_forget() {
        let (sender, rx) = make_sender();
        let local_keys = Keys::generate();
        let remote_pubkey = local_keys.public_key(); // use same key as self-encryption for test
        let relay_url = "wss://bunker.relay".to_string();

        let transport = ActorLaneTransport::new(
            sender,
            local_keys,
            remote_pubkey,
            relay_url.clone(),
        );

        let rpc = make_rpc("rpc-001", r#"{"method":"get_public_key","params":[]}"#);
        let result = transport.send_rpc(rpc);
        assert!(result.is_ok(), "send_rpc must return Ok(()) for valid keys");

        let frames = drain_enqueue_outbound(&rx);
        assert_eq!(frames.len(), 1, "exactly one EnqueueOutbound frame expected");

        let (role, url, text) = &frames[0];
        assert_eq!(*role, RelayRole::Signer);
        assert_eq!(url, &relay_url);
        // The frame must be a valid JSON array starting with "EVENT"
        assert!(
            text.starts_with('['),
            "frame must be a JSON array; got: {text}"
        );
        assert!(
            text.contains("EVENT"),
            "frame must contain EVENT; got: {text}"
        );
    }

    /// Multiple `send_rpc` calls must arrive in the order they were sent
    /// (channel FIFO — the invariant that multi-step NIP-46 sign flows rely on).
    #[test]
    fn send_rpc_ordering() {
        let (sender, rx) = make_sender();
        let local_keys = Keys::generate();
        let remote_pubkey = local_keys.public_key();
        let relay_url = "wss://bunker.relay".to_string();

        let transport = ActorLaneTransport::new(
            sender,
            local_keys,
            remote_pubkey,
            relay_url.clone(),
        );

        for i in 0..3u8 {
            let rpc = make_rpc(
                &format!("rpc-{i:03}"),
                &format!(r#"{{"method":"sign_event_{i}","params":[]}}"#),
            );
            assert!(transport.send_rpc(rpc).is_ok());
        }

        let frames = drain_enqueue_outbound(&rx);
        assert_eq!(frames.len(), 3, "all 3 RPCs must produce EnqueueOutbound frames");

        // All must target the Signer role and correct relay URL.
        for (i, (role, url, _)) in frames.iter().enumerate() {
            assert_eq!(*role, RelayRole::Signer, "frame {i}: role must be Signer");
            assert_eq!(url, &relay_url, "frame {i}: URL must match");
        }
    }

    /// `ActorLaneTransport` must implement `Send + Sync` so it can be wrapped
    /// in `Arc<dyn Nip46Transport>` and handed to the signer handle.
    #[test]
    fn transport_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ActorLaneTransport>();
    }

    /// `Arc<dyn Nip46Transport>` wrapping an `ActorLaneTransport` must compile
    /// and send an RPC — this verifies the object-safety contract the
    /// `ActorLaneSignerHandle` depends on.
    #[test]
    fn transport_as_dyn_nip46_transport() {
        let (sender, rx) = make_sender();
        let local_keys = Keys::generate();
        let remote_pubkey = local_keys.public_key();
        let relay_url = "wss://bunker.relay".to_string();

        let transport: Arc<dyn Nip46Transport> = Arc::new(ActorLaneTransport::new(
            sender,
            local_keys,
            remote_pubkey,
            relay_url.clone(),
        ));

        let rpc = make_rpc("dyn-rpc", r#"{"method":"get_public_key","params":[]}"#);
        let result = transport.send_rpc(rpc);
        assert!(result.is_ok());

        let frames = drain_enqueue_outbound(&rx);
        assert_eq!(frames.len(), 1);
    }
}
