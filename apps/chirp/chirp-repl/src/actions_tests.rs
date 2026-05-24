use super::*;
use nostr::nips::nip19::ToBech32;

const SECRET_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PUBKEY_HEX: &str = "4f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa";
const EVENT_ID_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

#[test]
fn load_key_sets_active_identity_label() {
    let mut session = Session::default();

    run(&mut session, Command::LoadKey(SECRET_HEX.into())).unwrap();

    assert_eq!(session.pubkey_hex.as_deref(), Some(PUBKEY_HEX));
}

#[test]
fn app_commands_dispatch_without_live_relays() {
    let mut session = Session::default();
    run(&mut session, Command::SetRelays(Vec::new())).unwrap();
    run(&mut session, Command::LoadKey(SECRET_HEX.into())).unwrap();

    run(&mut session, Command::Home).unwrap();
    run(&mut session, Command::Compose("hello".into())).unwrap();
    run(
        &mut session,
        Command::Reply(EVENT_ID_HEX.into(), "reply".into()),
    )
    .unwrap();
    run(
        &mut session,
        Command::React(EVENT_ID_HEX.into(), "+".into()),
    )
    .unwrap();
    run(&mut session, Command::Follow(PUBKEY_HEX.into())).unwrap();
    run(&mut session, Command::Unfollow(PUBKEY_HEX.into())).unwrap();
    run(
        &mut session,
        Command::SendDm(PUBKEY_HEX.into(), "hi dm".into()),
    )
    .unwrap();
}

#[test]
fn old_bypass_commands_are_explicitly_rejected() {
    let mut session = Session::default();

    assert!(run(&mut session, Command::Notifications).is_err());
    assert!(run(&mut session, Command::RawReq("{}".into())).is_err());
}

#[test]
fn normalizes_nip19_inputs() {
    let npub = nmp_core::nip19::encode_npub(PUBKEY_HEX).unwrap();
    let note = nmp_core::nip19::encode_note(EVENT_ID_HEX).unwrap();

    assert_eq!(normalize_pubkey(&npub).unwrap(), PUBKEY_HEX);
    assert_eq!(normalize_event_id(&note).unwrap(), EVENT_ID_HEX);
    assert_eq!(normalize_pubkey(PUBKEY_HEX).unwrap(), PUBKEY_HEX);
    assert_eq!(normalize_event_id(EVENT_ID_HEX).unwrap(), EVENT_ID_HEX);
}

#[test]
fn load_key_accepts_nsec() {
    let keys = Keys::new(SecretKey::from_hex(SECRET_HEX).unwrap());
    let nsec = keys.secret_key().to_bech32().unwrap();
    let mut session = Session::default();

    run(&mut session, Command::LoadKey(nsec)).unwrap();

    assert_eq!(session.pubkey_hex.as_deref(), Some(PUBKEY_HEX));
}

/// V-51 phase 4 — the `routing-trace` action MUST succeed regardless of
/// kernel state (it's a read-only diagnostic; "kernel not started yet"
/// renders as a `<no projection slot bound>` line rather than an error).
/// This pins the contract: the user can run `routing-trace` at any point
/// without surprises.
#[test]
fn routing_trace_action_never_errors() {
    let mut session = Session::default();
    // Cold-start: projection slot may still be empty (no actor command sent).
    run(&mut session, Command::RoutingTrace).unwrap();
    // After loading a key the actor has constructed the kernel; the slot is
    // now populated and the projection rings render with their (likely empty)
    // contents.
    run(&mut session, Command::LoadKey(SECRET_HEX.into())).unwrap();
    run(&mut session, Command::RoutingTrace).unwrap();
}

/// V-51 phase 4 — after the actor has built the kernel (any `ActorCommand`
/// triggers this), `AppRuntime::routing_trace()` MUST return a non-`None`
/// projection. Without this contract the chirp-repl `routing-trace`
/// subcommand can never render real data.
#[test]
fn routing_trace_projection_is_published_after_actor_starts() {
    let mut session = Session::default();
    // Any command pushes through the actor and forces kernel construction.
    run(&mut session, Command::LoadKey(SECRET_HEX.into())).unwrap();
    // Give the actor thread a brief moment to publish the projection slot.
    // The slot write happens synchronously right after `Kernel::with_storage_path`
    // returns inside `run_actor_with_observers`, so a short wait suffices.
    for _ in 0..20 {
        if session.app.routing_trace().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("routing_trace projection slot never populated after actor command");
}
