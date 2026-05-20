use std::collections::BTreeSet;

use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{EventBuilder, Keys, Kind, SecretKey, Tag};
use serde_json::{json, Value};

use crate::command::Command;
use crate::render;
use crate::session::{LastRun, Session};
use crate::{wire, Result};

pub fn run(session: &mut Session, command: Command) -> Result<bool> {
    match command {
        Command::Help => render::help(),
        Command::Parity => render::parity(),
        Command::Diagnostics => render::diagnostics(session),
        Command::SetRelays(relays) => {
            session.relays = relays;
            render::status_ok("updated app relays");
        }
        Command::SetIndexers(indexers) => {
            session.indexers = indexers;
            render::status_ok("updated indexer relays");
        }
        Command::LoadKey(input) => load_key(session, &input)?,
        Command::CreateAccount(name) => create_account(session, &name)?,
        Command::SyncFollows => sync_follows(session)?,
        Command::Home => home(session)?,
        Command::Notifications => notifications(session)?,
        Command::Profile(author) => profile(session, &author)?,
        Command::Thread(id) => thread(session, &id)?,
        Command::Search(tag) => search(session, &tag)?,
        Command::Compose(text) => publish_note(session, &text, None)?,
        Command::Reply(id, text) => publish_note(session, &text, Some(normalize_event_id(&id)?))?,
        Command::React(id, reaction) => react(session, &id, &reaction)?,
        Command::Follow(pubkey) => follow(session, &pubkey, true)?,
        Command::Unfollow(pubkey) => follow(session, &pubkey, false)?,
        Command::RawReq(filter) => raw_req(session, &filter)?,
        Command::Quit => return Ok(true),
        Command::Noop => {}
    }
    Ok(false)
}

fn load_key(session: &mut Session, input: &str) -> Result<()> {
    let secret = if input.starts_with("nsec1") {
        SecretKey::from_bech32(input).map_err(|e| format!("bad nsec: {e}"))?
    } else if input.len() == 64 && input.chars().all(|c| c.is_ascii_hexdigit()) {
        SecretKey::from_hex(input).map_err(|e| format!("bad hex secret: {e}"))?
    } else {
        return Err("load-key expects nsec1... or 64-hex".into());
    };
    let keys = Keys::new(secret);
    set_identity(session, keys)?;
    render::status_ok("loaded identity");
    Ok(())
}

fn create_account(session: &mut Session, name: &str) -> Result<()> {
    let keys = Keys::generate();
    let profile = json!({ "name": name, "display_name": name }).to_string();
    let kind0 = EventBuilder::new(Kind::Metadata, profile)
        .sign_with_keys(&keys)
        .map_err(|e| format!("sign kind:0: {e}"))?;
    let relay_tags = session
        .relays
        .iter()
        .map(|url| tag(["r", url.as_str(), "write"]))
        .collect::<Result<Vec<_>>>()?;
    let kind10002 = EventBuilder::new(Kind::RelayList, "")
        .tags(relay_tags)
        .sign_with_keys(&keys)
        .map_err(|e| format!("sign kind:10002: {e}"))?;
    set_identity(session, keys)?;
    let (ok0, fail0) = wire::publish(&kind0, &session.relays, session.wall);
    render::publish_result("kind:0 profile", &kind0.id.to_hex(), ok0, fail0);
    let (ok1, fail1) = wire::publish(&kind10002, &session.relays, session.wall);
    render::publish_result("kind:10002 relay list", &kind10002.id.to_hex(), ok1, fail1);
    Ok(())
}

fn set_identity(session: &mut Session, keys: Keys) -> Result<()> {
    let npub = keys
        .public_key()
        .to_bech32()
        .map_err(|e| format!("encode npub: {e}"))?;
    println!("  npub: {npub}");
    session.pubkey_hex = Some(keys.public_key().to_hex());
    session.keys = Some(keys);
    session.follows.clear();
    Ok(())
}

fn sync_follows(session: &mut Session) -> Result<()> {
    let me = session.active_pubkey()?.to_string();
    let events = run_fetch(
        session,
        "sync-follows",
        json!({"kinds":[3], "authors":[me], "limit":1}),
    );
    let mut follows = BTreeSet::new();
    for event in events {
        for tag in event
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if tag.get(0).and_then(Value::as_str) == Some("p") {
                if let Some(pubkey) = tag.get(1).and_then(Value::as_str) {
                    if pubkey.len() == 64 && pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
                        follows.insert(pubkey.to_lowercase());
                    }
                }
            }
        }
    }
    session.follows = follows;
    render::status_ok(&format!("loaded {} follows", session.follows.len()));
    Ok(())
}

fn home(session: &mut Session) -> Result<()> {
    if session.follows.is_empty() {
        render::status_warn("follow cache is empty; showing active account only");
    }
    let authors: Vec<String> = if session.follows.is_empty() {
        vec![session.active_pubkey()?.into()]
    } else {
        session.follows.iter().cloned().collect()
    };
    print_events(run_fetch(
        session,
        "home",
        json!({"kinds":[1], "authors":authors, "limit":100}),
    ));
    Ok(())
}

fn notifications(session: &mut Session) -> Result<()> {
    let me = session.active_pubkey()?.to_string();
    print_events(run_fetch(
        session,
        "notifications",
        json!({"kinds":[1,7], "#p":[me], "limit":100}),
    ));
    Ok(())
}

fn profile(session: &mut Session, input: &str) -> Result<()> {
    let author = normalize_pubkey(input)?;
    print_events(run_fetch(
        session,
        "profile",
        json!({"kinds":[0], "authors":[author], "limit":1}),
    ));
    Ok(())
}

fn thread(session: &mut Session, input: &str) -> Result<()> {
    let id = normalize_event_id(input)?;
    print_events(run_fetch(
        session,
        "thread",
        json!({"kinds":[1], "ids":[id.clone()], "#e":[id], "limit":100}),
    ));
    Ok(())
}

fn search(session: &mut Session, input: &str) -> Result<()> {
    let tag = input.trim_start_matches('#');
    print_events(run_fetch(
        session,
        "search",
        json!({"kinds":[1], "#t":[tag], "limit":100}),
    ));
    Ok(())
}

fn raw_req(session: &mut Session, filter: &str) -> Result<()> {
    let value: Value = serde_json::from_str(filter).map_err(|e| format!("bad JSON filter: {e}"))?;
    print_events(run_fetch(session, "raw-req", value));
    Ok(())
}

fn publish_note(session: &mut Session, text: &str, reply_to: Option<String>) -> Result<()> {
    let keys = session.active_keys()?;
    let mut tags = Vec::new();
    if let Some(id) = reply_to {
        tags.push(tag(["e", id.as_str(), "", "root"])?);
        tags.push(tag(["e", id.as_str(), "", "reply"])?);
    }
    let event = EventBuilder::new(Kind::TextNote, text)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| format!("sign note: {e}"))?;
    publish(session, &event, "kind:1 note")
}

fn react(session: &mut Session, input: &str, reaction: &str) -> Result<()> {
    let keys = session.active_keys()?;
    let id = normalize_event_id(input)?;
    let event = EventBuilder::new(Kind::from_u16(7), reaction)
        .tag(tag(["e", id.as_str()])?)
        .sign_with_keys(keys)
        .map_err(|e| format!("sign reaction: {e}"))?;
    publish(session, &event, "kind:7 reaction")
}

fn follow(session: &mut Session, input: &str, add: bool) -> Result<()> {
    let target = normalize_pubkey(input)?;
    if add {
        session.follows.insert(target);
    } else {
        session.follows.remove(&target);
    }
    let keys = session.active_keys()?;
    let tags = session
        .follows
        .iter()
        .map(|pubkey| tag(["p", pubkey.as_str()]))
        .collect::<Result<Vec<_>>>()?;
    let event = EventBuilder::new(Kind::from_u16(3), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| format!("sign contact list: {e}"))?;
    publish(
        session,
        &event,
        if add {
            "kind:3 follow"
        } else {
            "kind:3 unfollow"
        },
    )
}

fn publish(session: &mut Session, event: &nostr::Event, label: &str) -> Result<()> {
    let (ok, fail) = wire::publish(event, &session.relays, session.wall);
    render::publish_result(label, &event.id.to_hex(), ok, fail);
    Ok(())
}

fn run_fetch(session: &mut Session, label: &str, filter: Value) -> Vec<Value> {
    let events = wire::fetch(&session.relays, filter, session.wall);
    let mut new_events = 0;
    for event in &events {
        if let Some(id) = event.get("id").and_then(Value::as_str) {
            if session.seen_ids.insert(id.into()) {
                new_events += 1;
            }
        }
    }
    session.last_run = Some(LastRun {
        label: label.into(),
        relays: session.relays.len(),
        events: events.len(),
        new_events,
    });
    events
}

fn print_events(events: Vec<Value>) {
    for event in &events {
        render::event(event);
    }
    render::status_ok(&format!("{} events", events.len()));
}

fn tag<const N: usize>(parts: [&str; N]) -> Result<Tag> {
    Tag::parse(parts).map_err(|e| format!("bad tag: {e}"))
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
