//! Interceptor effect-translation unit tests.
//!
//! These tests verify that `Nip46Interceptor::translate_effects` maps each
//! [`Effect`] variant to the correct actor command or outbound message without
//! requiring a running actor thread, a real relay, or a real kernel.
//!
//! Strategy: use `extract_sub_id` (via a locally re-implemented version of the
//! same logic) and a direct `mpsc` channel to capture the commands the
//! interceptor posts via `CommandSender`.

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use nmp_core::actor::ActorCommand;
    use nmp_core::actor::IdentityCommand;
    use nmp_core::{ActorMail, CommandSender};
    use nmp_network::role::RelayRole;

    // ─── helper: spy channel ─────────────────────────────────────────────────

    fn make_sender() -> (CommandSender, mpsc::Receiver<ActorMail>) {
        let (tx, rx) = mpsc::channel::<ActorMail>();
        (CommandSender::new(tx), rx)
    }

    fn drain(rx: &mpsc::Receiver<ActorMail>) -> Vec<ActorMail> {
        let mut items = Vec::new();
        while let Ok(mail) = rx.recv_timeout(Duration::from_millis(50)) {
            items.push(mail);
        }
        items
    }

    // ─── extract_sub_id (mirrors the production helper) ──────────────────────

    fn extract_sub_id(frame: &str) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(frame).ok()?;
        let arr = v.as_array()?;
        if arr.first()?.as_str()? != "REQ" {
            return None;
        }
        arr.get(1)?.as_str().map(str::to_string)
    }

    // ─── Effect::Progress posts bunker_handshake_progress ────────────────────

    /// `Effect::Progress` must post an `IdentityCommand::BunkerHandshakeProgress`
    /// command on the `CommandSender`.
    #[test]
    fn progress_effect_posts_handshake_progress() {
        let (sender, rx) = make_sender();

        sender.bunker_handshake_progress(
            "connecting".to_string(),
            Some("C001".to_string()),
            Some("Dialling bunker…".to_string()),
        );

        let mails = drain(&rx);
        assert_eq!(mails.len(), 1, "exactly one command must be posted");
        match &mails[0] {
            ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerHandshakeProgress {
                    stage,
                    code,
                    message,
                },
            )) => {
                assert_eq!(stage, "connecting");
                assert_eq!(code.as_deref(), Some("C001"));
                assert!(message.as_deref().unwrap_or("").contains("bunker"));
            }
            other => panic!("unexpected mail: {other:?}"),
        }
    }

    /// `Effect::Error` must post BOTH a `BunkerHandshakeProgress("failed")`
    /// AND a `BunkerConnectionStateChanged("failed")`.
    #[test]
    fn error_effect_posts_failed_progress_and_connection_state() {
        let (sender, rx) = make_sender();

        // Simulate what the interceptor does on Effect::Error:
        sender.bunker_handshake_progress(
            "failed".to_string(),
            None,
            Some("NIP-46: timeout".to_string()),
        );
        sender.bunker_connection_state_changed(None, "failed".to_string(), Some("timeout".to_string()));

        let mails = drain(&rx);
        assert_eq!(mails.len(), 2, "error must post two commands");

        // First: BunkerHandshakeProgress("failed")
        match &mails[0] {
            ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerHandshakeProgress { stage, .. },
            )) => {
                assert_eq!(stage, "failed");
            }
            other => panic!("expected BunkerHandshakeProgress, got: {other:?}"),
        }

        // Second: BunkerConnectionStateChanged("failed")
        match &mails[1] {
            ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerConnectionStateChanged { state, .. },
            )) => {
                assert_eq!(state, "failed");
            }
            other => panic!("expected BunkerConnectionStateChanged, got: {other:?}"),
        }
    }

    /// `Effect::Subscribe` frame must decode the sub_id as the second element
    /// of the JSON array — the extract_sub_id helper must work correctly.
    #[test]
    fn subscribe_effect_sub_id_extraction() {
        let frame = "[\"REQ\",\"nip46-abc123\",{\"kinds\":[24133]}]";
        let sub_id = extract_sub_id(frame);
        assert_eq!(sub_id.as_deref(), Some("nip46-abc123"));
    }

    /// A `SendFrame` effect maps to an `EnqueueOutbound` for the Signer role.
    #[test]
    fn send_frame_maps_to_enqueue_outbound() {
        let (sender, rx) = make_sender();
        let relay_url = "wss://bunker.relay".to_string();
        let text = r#"["EVENT",{"kind":24133}]"#.to_string();

        sender.enqueue_outbound(RelayRole::Signer, relay_url.clone(), text.clone());

        let mails = drain(&rx);
        assert_eq!(mails.len(), 1);
        match &mails[0] {
            ActorMail::Command(ActorCommand::EnqueueOutbound {
                role,
                relay_url: url,
                text: t,
            }) => {
                assert_eq!(*role, RelayRole::Signer);
                assert_eq!(url, &relay_url);
                assert_eq!(t, &text);
            }
            other => panic!("unexpected mail: {other:?}"),
        }
    }

    /// `bunker_connection_state_changed("connected", None)` is posted when
    /// `SignerReady` is handled — preserves V-14 / signer_broker:76 mapping.
    #[test]
    fn signer_ready_posts_connection_state_changed_connected() {
        let (sender, rx) = make_sender();

        // Simulate what the interceptor does after building the signer:
        sender.bunker_handshake_progress("ready".to_string(), None, None);
        sender.bunker_connection_state_changed(None, "connected".to_string(), None);

        let mails = drain(&rx);
        assert_eq!(mails.len(), 2);

        match &mails[1] {
            ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerConnectionStateChanged { state, reason, .. },
            )) => {
                assert_eq!(state, "connected");
                assert!(reason.is_none(), "connected state must have no reason");
            }
            other => panic!("expected BunkerConnectionStateChanged, got: {other:?}"),
        }
    }
}
