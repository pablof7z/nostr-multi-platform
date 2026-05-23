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
