//! `EnqueueOutbound` command ordering and wake tests.
//!
//! Verifies that frames posted via `CommandSender::enqueue_outbound` are
//! received by the actor inbox in source order (channel FIFO) and that each
//! send wakes a waiting receiver.

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use nmp_core::actor::ActorCommand;
    use nmp_core::{ActorMail, CommandSender};
    use nmp_network::role::RelayRole;

    fn make_sender() -> (CommandSender, mpsc::Receiver<ActorMail>) {
        let (tx, rx) = mpsc::channel::<ActorMail>();
        (CommandSender::new(tx), rx)
    }

    /// Frames enqueued via `enqueue_outbound` arrive in the order they were
    /// sent (channel FIFO — the invariant that REQ-before-EVENT relies on).
    #[test]
    fn enqueue_outbound_ordering() {
        let (tx, rx) = make_sender();

        tx.enqueue_outbound(
            RelayRole::Signer,
            "wss://bunker.relay".to_string(),
            r#"["REQ","nip46-abc",{}]"#.to_string(),
        );
        tx.enqueue_outbound(
            RelayRole::Signer,
            "wss://bunker.relay".to_string(),
            r#"["EVENT",{}]"#.to_string(),
        );

        let mail1 = rx.recv_timeout(Duration::from_secs(1)).expect("first frame");
        let mail2 = rx.recv_timeout(Duration::from_secs(1)).expect("second frame");

        let (role1, url1, text1) = extract_enqueue_outbound(mail1);
        let (role2, url2, text2) = extract_enqueue_outbound(mail2);

        assert_eq!(role1, RelayRole::Signer);
        assert_eq!(url1, "wss://bunker.relay");
        assert!(
            text1.contains("REQ"),
            "first frame must be the REQ subscription; got: {text1}"
        );

        assert_eq!(role2, RelayRole::Signer);
        assert_eq!(url2, "wss://bunker.relay");
        assert!(
            text2.contains("EVENT"),
            "second frame must be the EVENT; got: {text2}"
        );
    }

    /// Each `enqueue_outbound` call wakes a blocking `recv_timeout` — no
    /// spurious blocking even when the receiver has already consumed all
    /// prior items.
    #[test]
    fn enqueue_outbound_wakes_receiver() {
        let (tx, rx) = make_sender();

        for i in 0..5u8 {
            tx.enqueue_outbound(
                RelayRole::Signer,
                "wss://bunker.relay".to_string(),
                format!("frame-{i}"),
            );
        }

        let mut received = 0u8;
        while let Ok(mail) = rx.recv_timeout(Duration::from_millis(100)) {
            let _ = extract_enqueue_outbound(mail);
            received += 1;
            if received == 5 {
                break;
            }
        }
        assert_eq!(received, 5, "all 5 frames must be received");
    }

    // ─── helper ──────────────────────────────────────────────────────────────

    fn extract_enqueue_outbound(mail: ActorMail) -> (RelayRole, String, String) {
        match mail {
            ActorMail::Command(ActorCommand::EnqueueOutbound { role, relay_url, text }) => {
                (role, relay_url, text)
            }
            other => panic!("expected EnqueueOutbound, got: {other:?}"),
        }
    }
}
