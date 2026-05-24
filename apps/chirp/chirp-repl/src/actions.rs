use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{Keys, SecretKey};
use serde_json::{json, Value};

use crate::command::Command;
use crate::marmot;
use crate::render;
use crate::session::{LastRun, Session};
use crate::Result;

pub fn run(session: &mut Session, command: Command) -> Result<bool> {
    match command {
        Command::Help => render::help(),
        Command::Parity => render::parity(),
        Command::Diagnostics => render::diagnostics(session),
        Command::SetRelays(relays) => set_relays(session, relays)?,
        Command::SetIndexers(indexers) => set_indexers(session, indexers)?,
        Command::LoadKey(input) => load_key(session, &input)?,
        Command::CreateAccount(name) => create_account(session, &name)?,
        Command::SyncFollows => open_home(session, "sync-follows")?,
        Command::Home => open_home(session, "home")?,
        Command::Notifications => unsupported("notifications")?,
        Command::Profile(author) => profile(session, &author)?,
        Command::Thread(id) => thread(session, &id)?,
        Command::Search(tag) => search(session, &tag)?,
        Command::Compose(text) => publish_note(session, &text, None)?,
        Command::Reply(id, text) => publish_note(session, &text, Some(normalize_event_id(&id)?))?,
        Command::React(id, reaction) => react(session, &id, &reaction)?,
        Command::Follow(pubkey) => follow(session, &pubkey, true)?,
        Command::Unfollow(pubkey) => follow(session, &pubkey, false)?,
        Command::SendDm(recipient, text) => send_dm(session, &recipient, &text)?,
        Command::MlsInit => mls_init(session)?,
        Command::MlsStatus => mls_status(session)?,
        Command::MlsCreate(name, invitees) => mls_create(session, &name, &invitees)?,
        Command::MlsInvite(group_id, invitee) => mls_invite(session, &group_id, &invitee)?,
        Command::MlsAccept(welcome_id) => mls_accept(session, &welcome_id)?,
        Command::MlsSend(group_id, text) => mls_send(session, &group_id, &text)?,
        Command::MlsMessages(group_id) => mls_messages(session, &group_id)?,
        Command::RawReq(_) => unsupported("raw-req")?,
        Command::RoutingTrace => routing_trace(session),
        Command::Quit => return Ok(true),
        Command::Noop => {}
    }
    Ok(false)
}

fn set_relays(session: &mut Session, relays: Vec<String>) -> Result<()> {
    let old = std::mem::replace(&mut session.relays, relays);
    session.app.reset_relays(&old, &session.relays, "both")?;
    render::status_ok("updated app relays");
    Ok(())
}

fn set_indexers(session: &mut Session, indexers: Vec<String>) -> Result<()> {
    let old = std::mem::replace(&mut session.indexers, indexers);
    session
        .app
        .reset_relays(&old, &session.indexers, "indexer")?;
    render::status_ok("updated indexer relays");
    Ok(())
}

fn load_key(session: &mut Session, input: &str) -> Result<()> {
    let secret = parse_secret(input)?;
    let keys = Keys::new(secret);
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|e| format!("encode nsec: {e}"))?;
    session.app.sign_in_nsec_with_marmot(&nsec)?;
    set_identity_label(session, &keys)?;
    render::status_ok("loaded identity through NmpApp");
    Ok(())
}

fn create_account(session: &mut Session, name: &str) -> Result<()> {
    let profile = json!({ "name": name, "display_name": name }).to_string();
    let mut relays = session
        .relays
        .iter()
        .map(|url| (url.clone(), "both".to_string()))
        .collect::<Vec<_>>();
    relays.extend(
        session
            .indexers
            .iter()
            .filter(|url| !session.relays.contains(url))
            .map(|url| (url.clone(), "indexer".to_string())),
    );
    let relays_json = serde_json::to_string(&relays).map_err(|e| e.to_string())?;
    session.app.create_account(&profile, &relays_json, false)?;
    session.pubkey_hex = None;
    render::status_ok("queued account creation through NmpApp");
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn open_home(session: &mut Session, label: &str) -> Result<()> {
    session.app.open_timeline();
    render_snapshot(session, label);
    Ok(())
}

fn profile(session: &mut Session, input: &str) -> Result<()> {
    let author = normalize_pubkey(input)?;
    session.app.open_author(&author)?;
    render_snapshot(session, "profile");
    Ok(())
}

fn thread(session: &mut Session, input: &str) -> Result<()> {
    let id = normalize_event_id(input)?;
    session.app.open_thread(&id)?;
    render_snapshot(session, "thread");
    Ok(())
}

fn search(session: &mut Session, input: &str) -> Result<()> {
    let tag = input.trim_start_matches('#');
    session.app.open_tag(tag)?;
    render_snapshot(session, "search");
    Ok(())
}

fn publish_note(session: &mut Session, text: &str, reply_to: Option<String>) -> Result<()> {
    session.app.publish_note(text, reply_to.as_deref())?;
    render::status_ok("queued note publish through NmpApp");
    Ok(())
}

fn react(session: &mut Session, input: &str, reaction: &str) -> Result<()> {
    let id = normalize_event_id(input)?;
    session.app.react(&id, reaction)?;
    render::status_ok("queued reaction through NmpApp");
    Ok(())
}

fn follow(session: &mut Session, input: &str, add: bool) -> Result<()> {
    let target = normalize_pubkey(input)?;
    session.app.follow(&target, add)?;
    render::status_ok(if add {
        "queued follow through NmpApp"
    } else {
        "queued unfollow through NmpApp"
    });
    Ok(())
}

fn send_dm(session: &mut Session, recipient: &str, text: &str) -> Result<()> {
    let recipient = normalize_pubkey(recipient)?;
    session.app.send_dm(&recipient, text)?;
    render::status_ok("queued NIP-17 gift-wrapped DM through NmpApp");
    Ok(())
}

fn mls_init(session: &mut Session) -> Result<()> {
    let result = marmot::init(&mut session.app, &session.relays)?;
    render::marmot_result("mls key package", &result);
    Ok(())
}

fn mls_status(session: &mut Session) -> Result<()> {
    let snapshot = session.app.marmot_snapshot()?;
    render::marmot_snapshot(&snapshot);
    Ok(())
}

fn mls_create(session: &mut Session, name: &str, invitees: &[String]) -> Result<()> {
    let invitees = invitees
        .iter()
        .map(|s| normalize_pubkey(s))
        .collect::<Result<Vec<_>>>()?;
    let result = marmot::create_group(&session.app, name, &invitees, &session.relays)?;
    render::marmot_result("mls create", &result);
    Ok(())
}

fn mls_invite(session: &mut Session, group_id: &str, invitee: &str) -> Result<()> {
    let invitee = normalize_pubkey(invitee)?;
    let result = marmot::invite(&session.app, group_id, &[invitee])?;
    render::marmot_result("mls invite", &result);
    Ok(())
}

fn mls_accept(session: &mut Session, welcome_id: &str) -> Result<()> {
    let welcome_id = if welcome_id == "first" {
        let snapshot = session.app.marmot_snapshot()?;
        marmot::first_pending_welcome_id(&snapshot)
            .ok_or_else(|| "no pending welcome to accept".to_string())?
    } else {
        welcome_id.to_string()
    };
    let result = marmot::accept(&session.app, &welcome_id)?;
    render::marmot_result("mls accept", &result);
    Ok(())
}

fn mls_send(session: &mut Session, group_id: &str, text: &str) -> Result<()> {
    let result = marmot::send(&session.app, group_id, text)?;
    render::marmot_result("mls send", &result);
    Ok(())
}

fn mls_messages(session: &mut Session, group_id: &str) -> Result<()> {
    let rows = marmot::group_messages(&session.app, group_id)?;
    render::marmot_messages(&rows);
    Ok(())
}

fn unsupported(command: &str) -> Result<()> {
    Err(format!(
        "{command} is not exposed by the Chirp app runtime yet; refusing to bypass NMP"
    ))
}

/// V-51 phase 4 — dump the kernel's routing-trace projection. Reads through
/// `NmpApp::routing_trace`, which returns `None` until the actor has built
/// the kernel; once `Some`, the projection is the live ring buffer the
/// default `Nip65WriteSetRouter` writes into on every routing decision.
fn routing_trace(session: &Session) {
    let projection = session.app.routing_trace();
    render::routing_trace(projection.as_ref());
}

fn render_snapshot(session: &mut Session, label: &str) {
    let Some(snapshot) = session.app.chirp_snapshot() else {
        render::status_warn("no Chirp snapshot available yet");
        return;
    };
    let blocks = snapshot
        .get("blocks")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let cards = snapshot
        .get("cards")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    session.last_run = Some(LastRun {
        label: label.to_string(),
        relays: session.relays.len(),
        events: cards,
        new_events: 0,
    });
    render::chirp_snapshot(&snapshot, blocks, cards);
}

fn parse_secret(input: &str) -> Result<SecretKey> {
    if input.starts_with("nsec1") {
        SecretKey::from_bech32(input).map_err(|e| format!("bad nsec: {e}"))
    } else if input.len() == 64 && input.chars().all(|c| c.is_ascii_hexdigit()) {
        SecretKey::from_hex(input).map_err(|e| format!("bad hex secret: {e}"))
    } else {
        Err("load-key expects nsec1... or 64-hex".into())
    }
}

fn set_identity_label(session: &mut Session, keys: &Keys) -> Result<()> {
    let npub = keys
        .public_key()
        .to_bech32()
        .map_err(|e| format!("encode npub: {e}"))?;
    println!("  npub: {npub}");
    session.pubkey_hex = Some(keys.public_key().to_hex());
    Ok(())
}

fn normalize_pubkey(input: &str) -> Result<String> {
    let value = input.trim();
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(value.to_lowercase());
    }
    match nmp_core::nip19::parse(value).map_err(|e| e.to_string())? {
        nmp_core::nip19::Nip19Entity::Npub(hex) => Ok(hex),
        nmp_core::nip19::Nip19Entity::Nprofile(data) => Ok(data.pubkey),
        other => Err(format!("expected npub/nprofile/pubkey, got {other:?}")),
    }
}

fn normalize_event_id(input: &str) -> Result<String> {
    let value = input.trim();
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(value.to_lowercase());
    }
    match nmp_core::nip19::parse(value).map_err(|e| e.to_string())? {
        nmp_core::nip19::Nip19Entity::Note(hex) => Ok(hex),
        nmp_core::nip19::Nip19Entity::Nevent(data) => Ok(data.event_id),
        other => Err(format!("expected note/nevent/event id, got {other:?}")),
    }
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod actions_tests;
