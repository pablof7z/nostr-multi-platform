use super::*;

#[test]
fn local_key_provider_brokers_sign_inline() {
    let (mut handle, _pubkey_hex) = handle_with_local_key_signer();
    let sender = handle.command_sender();

    sender
        .send(ActorCommand::Publish(PublishCommand::Profile {
            fields: serde_json::Map::new(),
            correlation_id: Some("lk-inline-cid".to_string()),
        }))
        .expect("send through command inbox");

    let out = handle.pump();
    let sign_requests: Vec<_> = out
        .events
        .iter()
        .filter(|e| matches!(e, BrowserRuntimeEvent::SignRequest { .. }))
        .collect();
    assert!(
        sign_requests.is_empty(),
        "LocalKey provider must not emit SignRequest (auto-brokered inline): \
         events = {:?}",
        out.events
    );
    assert_eq!(
        handle.pending_sign_count(),
        0,
        "LocalKey must resolve the sign inline - pending must be 0 after pump()"
    );
}

#[test]
fn capability_envelope_reflects_local_key_signer() {
    let (handle, pubkey_hex) = handle_with_local_key_signer();

    let env = handle
        .capability_envelope(&pubkey_hex)
        .expect("envelope must exist for registered pubkey");
    assert!(env.sign_event, "sign_event always true");
    assert!(env.nip04, "LocalKeySigner advertises nip04");
    assert!(env.nip44, "LocalKeySigner advertises nip44");
    assert!(
        matches!(env.backend, nmp_signers::SignerBackend::LocalKey),
        "backend must be LocalKey"
    );
    assert!(
        handle.capability_envelope("deadbeef").is_none(),
        "unregistered pubkey must return None"
    );
}

#[test]
fn nip46_bunker_start_emits_signer_relay_frames_and_progress_state() {
    let mut handle = started_handle();
    let remote = nostr::Keys::generate();
    let uri = format!(
        "bunker://{}?relay=wss://relay.example.com",
        remote.public_key().to_hex()
    );

    let outbound = handle.begin_nip46_bunker(&uri).expect("start bunker");

    assert!(
        outbound
            .iter()
            .any(|msg| msg.role() == nmp_network::role::RelayRole::Signer
                && msg.text().starts_with("[\"REQ\"")),
        "bunker start must emit a Signer-lane REQ: {outbound:?}"
    );
    assert!(
        outbound
            .iter()
            .any(|msg| msg.role() == nmp_network::role::RelayRole::Signer
                && msg.text().starts_with("[\"EVENT\"")),
        "bunker start must emit a Signer-lane connect EVENT: {outbound:?}"
    );
    let diagnostics = handle.diagnostics();
    assert_eq!(diagnostics.signer_kind.as_deref(), Some("nip46"));
    assert!(
        matches!(
            diagnostics.signer_state.as_deref(),
            Some("reconnecting" | "awaiting_approval")
        ),
        "start must expose in-flight NIP-46 signer state: {diagnostics:?}"
    );
    assert!(
        handle.active_account_pubkey_inner().is_none(),
        "NIP-46 active account is learned only after SignerReady"
    );
}

#[derive(Debug, Default)]
struct RuntimeStubTransport;

impl nmp_signer_iface::Nip46Transport for RuntimeStubTransport {
    fn send_rpc(
        &self,
        _rpc: nmp_signer_iface::Nip46Rpc,
    ) -> Result<(), nmp_signer_iface::SignerError> {
        Ok(())
    }
}

fn completed_nip46_signer(remote_user: &LocalKeySigner) -> nmp_signers::Nip46Signer {
    let uri = format!(
        "bunker://{}?relay=wss://relay.example.com",
        remote_user.pubkey().to_hex()
    );
    let handle = nmp_signers::Nip46SignerHandle::from_bunker_uri(&uri).expect("bunker uri");
    handle.complete(Arc::new(RuntimeStubTransport), remote_user.pubkey())
}

#[test]
fn nip46_signer_ready_installs_provider_and_selects_active_account() {
    let mut handle = started_handle();
    let remote_user = LocalKeySigner::generate();
    let pubkey = remote_user.pubkey().to_hex();
    let signer = Arc::new(completed_nip46_signer(&remote_user));

    handle
        .runtime
        .nip46
        .push_event_for_test(crate::signer::nip46::BrowserNip46Event::InstallSigner { signer });
    let (outbound, events) = handle.runtime.drain_nip46_events_and_completions();

    assert!(
        outbound.is_empty(),
        "install alone should not emit relay frames"
    );
    assert!(
        events.is_empty(),
        "install alone should not emit host events"
    );
    assert_eq!(
        handle.active_account_pubkey_inner().as_deref(),
        Some(pubkey.as_str())
    );
    let envelope = handle
        .capability_envelope(&pubkey)
        .expect("NIP-46 provider registered");
    assert!(matches!(
        envelope.backend,
        nmp_signers::SignerBackend::Nip46
    ));
    let diagnostics = handle.diagnostics();
    assert_eq!(diagnostics.signer_kind.as_deref(), Some("nip46"));
    assert_eq!(diagnostics.signer_state.as_deref(), Some("ready"));
}

#[test]
fn deliver_signer_response_failure_enqueues_fires_wake_and_applies_on_pump() {
    let mut handle = started_handle();
    handle.set_active_account_for_test("ab".repeat(32));
    let wake_count = install_counting_wake(&mut handle);

    let (corr, _unsigned) = park_host_brokered_sign(&mut handle, "host-broker-fail-cid");
    assert_eq!(handle.pending_sign_count(), 1, "one publish parked");

    handle.deliver_signer_response(corr, Err("user rejected".to_string()));
    assert_eq!(
        wake_count.get(),
        1,
        "deliver_signer_response must fire the wake (D8 re-entry)"
    );
    assert_eq!(
        handle.pending_sign_count(),
        1,
        "D4: reducer untouched until pump() - parked publish still present"
    );

    let out = handle.pump();
    assert_eq!(
        handle.pending_sign_count(),
        0,
        "pump must apply the enqueued completion and clear the parked publish"
    );
    // Failure must surface as SignFailed (not CommandFailed) so the
    // main-thread broker can resolve any pending sign promise keyed on
    // the correlation id (#2139 BLOCKER 2 — was incorrectly CommandFailed).
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, BrowserRuntimeEvent::SignFailed { .. })),
        "failure delivery must surface SignFailed on the applying pump: {:?}",
        out.events
    );
}

#[test]
fn deliver_signer_response_success_applies_via_success_branch() {
    let signer = LocalKeySigner::from_secret_hex(&"f1".repeat(32)).expect("valid secret");
    let pubkey_hex = signer.pubkey().to_hex();

    let mut handle = started_handle();
    handle.set_active_account_for_test(pubkey_hex.clone());
    let wake_count = install_counting_wake(&mut handle);

    let (corr, unsigned_json) = park_host_brokered_sign(&mut handle, "host-broker-ok-cid");
    assert_eq!(handle.pending_sign_count(), 1, "one publish parked");

    let unsigned: UnsignedEvent =
        serde_json::from_str(&unsigned_json).expect("unsigned json must parse");
    let signed_json = match signer.sign(unsigned) {
        SignerOp::Ready(Ok(signed)) => signed.to_nip01_json(),
        other => panic!("local-key sign must be Ready(Ok): {other:?}"),
    };

    handle.deliver_signer_response(corr, Ok(signed_json));
    assert_eq!(wake_count.get(), 1, "success delivery must fire the wake");
    assert_eq!(
        handle.pending_sign_count(),
        1,
        "D4: reducer untouched until pump()"
    );

    let out = handle.pump();
    assert_eq!(
        handle.pending_sign_count(),
        0,
        "pump must apply the signed completion and clear the parked publish"
    );
    assert!(
        !out.events.iter().any(|e| matches!(
            e,
            BrowserRuntimeEvent::CommandFailed { .. } | BrowserRuntimeEvent::SignFailed { .. }
        )),
        "a valid signed delivery must take the success branch (no failure event): {:?}",
        out.events
    );
}
