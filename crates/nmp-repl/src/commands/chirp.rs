//! `chirp ...` - high-level Chirp app parity and diagnostics commands.
//!
//! The REPL is the operator surface for testing Chirp without Swift. Read
//! commands delegate to the existing planner/fanout `req` path; write commands
//! sign simple NIP-01 events with the session key and publish to app relays.

use std::collections::BTreeMap;

use nmp_core::planner::MailboxCache;
use nostr::{EventBuilder, Kind, Tag};

use crate::ast::{ChirpCommand, FilterAst, Value};
use crate::commands::req;
use crate::error::{ReplError, Result};
use crate::publish;
use crate::session::Session;

const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn run(session: &mut Session, cmd: ChirpCommand) -> Result<()> {
    match cmd {
        ChirpCommand::Home => run_req(session, home_filter(), "home timeline"),
        ChirpCommand::Notifications => {
            run_req(session, notifications_filter(), "notifications / mentions")
        }
        ChirpCommand::Profile(input) => {
            let author = normalize_pubkey(&input)?;
            run_req(session, profile_filter(author), "profile metadata")
        }
        ChirpCommand::Thread(input) => {
            let event_id = normalize_event_id(&input)?;
            run_req(session, thread_filter(event_id), "thread context")
        }
        ChirpCommand::SearchTag(tag) => run_req(session, tag_filter(tag), "hashtag search"),
        ChirpCommand::Compose(text) => publish_note(session, text, None),
        ChirpCommand::Reply(event_id, text) => {
            publish_note(session, text, Some(normalize_event_id(&event_id)?))
        }
        ChirpCommand::React(event_id, reaction) => publish_reaction(session, event_id, reaction),
        ChirpCommand::Follow(pubkey) => publish_follow(session, pubkey, true),
        ChirpCommand::Unfollow(pubkey) => publish_follow(session, pubkey, false),
        ChirpCommand::Diagnostics => {
            print_diagnostics(session);
            Ok(())
        }
        ChirpCommand::Parity => {
            print_parity();
            Ok(())
        }
    }
}

fn run_req(session: &mut Session, filter: FilterAst, label: &str) -> Result<()> {
    println!("{BLUE}chirp:{RESET} {label} {DIM}(planner + live relay fanout){RESET}");
    req::run(session, filter)
}

fn home_filter() -> FilterAst {
    FilterAst {
        kinds: Some(vec![1]),
        authors: Some(vec![Value::Var("follows".into())]),
        limit: Some(100),
        ..FilterAst::default()
    }
}

fn notifications_filter() -> FilterAst {
    let mut tags = BTreeMap::new();
    tags.insert('p', vec![Value::Var("me".into())]);
    FilterAst {
        kinds: Some(vec![1, 7]),
        tags,
        limit: Some(100),
        ..FilterAst::default()
    }
}

fn profile_filter(author: String) -> FilterAst {
    FilterAst {
        kinds: Some(vec![0]),
        authors: Some(vec![Value::Lit(author)]),
        limit: Some(1),
        ..FilterAst::default()
    }
}

fn thread_filter(event_id: String) -> FilterAst {
    let mut tags = BTreeMap::new();
    tags.insert('e', vec![Value::Lit(event_id.clone())]);
    FilterAst {
        kinds: Some(vec![1]),
        ids: Some(vec![Value::Lit(event_id)]),
        tags,
        limit: Some(100),
        ..FilterAst::default()
    }
}

fn tag_filter(tag: String) -> FilterAst {
    let mut tags = BTreeMap::new();
    tags.insert(
        't',
        vec![Value::Lit(tag.trim_start_matches('#').to_string())],
    );
    FilterAst {
        kinds: Some(vec![1]),
        tags,
        limit: Some(100),
        ..FilterAst::default()
    }
}

fn publish_note(session: &Session, text: String, reply_to: Option<String>) -> Result<()> {
    let keys = active_keys(session)?;
    let mut tags = Vec::new();
    if let Some(id) = reply_to {
        tags.push(parse_tag(["e", id.as_str(), "", "root"])?);
        tags.push(parse_tag(["e", id.as_str(), "", "reply"])?);
    }
    let event = EventBuilder::new(Kind::TextNote, text)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| ReplError::Other(format!("sign note: {e}")))?;
    publish_and_report(session, &event, "kind:1 note")
}

fn publish_reaction(session: &Session, event_id: String, reaction: String) -> Result<()> {
    let keys = active_keys(session)?;
    let id = normalize_event_id(&event_id)?;
    let event = EventBuilder::new(Kind::from_u16(7), reaction)
        .tag(parse_tag(["e", id.as_str()])?)
        .sign_with_keys(keys)
        .map_err(|e| ReplError::Other(format!("sign reaction: {e}")))?;
    publish_and_report(session, &event, "kind:7 reaction")
}

fn publish_follow(session: &mut Session, input: String, add: bool) -> Result<()> {
    let keys = active_keys(session)?;
    let target = normalize_pubkey(&input)?;
    let mut follows = session.follows_cache.clone().unwrap_or_default();
    if add {
        follows.insert(target.clone());
    } else {
        follows.remove(&target);
    }
    let tags: Vec<Tag> = follows
        .iter()
        .map(|p| parse_tag(["p", p.as_str()]))
        .collect::<Result<Vec<_>>>()?;
    let event = EventBuilder::new(Kind::from_u16(3), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| ReplError::Other(format!("sign contact list: {e}")))?;
    publish_and_report(
        session,
        &event,
        if add {
            "kind:3 follow"
        } else {
            "kind:3 unfollow"
        },
    )?;
    session.follows_cache = Some(follows);
    Ok(())
}

fn publish_and_report(session: &Session, event: &nostr::Event, label: &str) -> Result<()> {
    if session.app_relays.is_empty() {
        return Err(ReplError::Other(
            "no app relays - run 'set-app-relays wss://...' first".into(),
        ));
    }
    println!("{BLUE}chirp:{RESET} publishing {label}");
    println!("  id:   {}", event.id.to_hex());
    println!("  kind: {}", event.kind.as_u16());
    println!("  to:   {}", session.app_relays.join(", "));
    let (ok, fail) = publish::publish_event(event, &session.app_relays);
    println!("{GREEN}  ok:{RESET} {ok}   {YELLOW}fail:{RESET} {fail}");
    Ok(())
}

fn active_keys(session: &Session) -> Result<&nostr::Keys> {
    session.mls_keys.as_ref().ok_or_else(|| {
        ReplError::Other(
            "no active signing key - run 'load-key <nsec>' or 'create-account' first".into(),
        )
    })
}

fn parse_tag<const N: usize>(parts: [&str; N]) -> Result<Tag> {
    Tag::parse(parts).map_err(|e| ReplError::Other(format!("build tag: {e}")))
}

fn normalize_pubkey(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(trimmed.to_lowercase());
    }
    match nmp_core::nip19::parse(trimmed) {
        Ok(nmp_core::nip19::Nip19Entity::Npub(hex)) => Ok(hex),
        Ok(nmp_core::nip19::Nip19Entity::Nprofile(data)) => Ok(data.pubkey),
        Ok(other) => Err(ReplError::Other(format!(
            "expected npub/nprofile/pubkey, got {other:?}"
        ))),
        Err(e) => Err(ReplError::Other(format!("decode pubkey: {e}"))),
    }
}

fn normalize_event_id(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(trimmed.to_lowercase());
    }
    match nmp_core::nip19::parse(trimmed) {
        Ok(nmp_core::nip19::Nip19Entity::Note(hex)) => Ok(hex),
        Ok(nmp_core::nip19::Nip19Entity::Nevent(data)) => Ok(data.event_id),
        Ok(other) => Err(ReplError::Other(format!(
            "expected note/nevent/event id, got {other:?}"
        ))),
        Err(e) => Err(ReplError::Other(format!("decode event id: {e}"))),
    }
}

fn print_diagnostics(session: &Session) {
    println!("{BLUE}chirp diagnostics{RESET}");
    println!("  identity:        {}", session.prompt_label());
    println!(
        "  signing key:     {}",
        if session.mls_keys.is_some() {
            "loaded"
        } else {
            "missing"
        }
    );
    println!("  app relays:      {}", display_list(&session.app_relays));
    println!(
        "  indexer relays:  {}",
        display_list(&session.indexer_relays)
    );
    println!(
        "  follows cache:   {}",
        session.follows_cache.as_ref().map(|v| v.len()).unwrap_or(0)
    );
    println!(
        "  mailbox cache:   {}",
        session.mailbox_cache.snapshot_all().len()
    );
    println!(
        "  probed authors:  {}",
        session.lifecycle.probed_mailboxes().len()
    );
    println!("  seen events:     {}", session.seen_ids.len());
    if let Some(run) = &session.last_run {
        println!(
            "  last run:        {} relays, {} authors, {} unroutable, {} events ({} new), {:?}",
            run.relays_used,
            run.authors_on_wire,
            run.unroutable,
            run.events_total,
            run.events_new,
            run.wall
        );
    } else {
        println!("  last run:        <none>");
    }
}

fn print_parity() {
    println!("{BLUE}chirp repl parity map{RESET}");
    println!("  {GREEN}Home feed{RESET}:       chirp home");
    println!("  {GREEN}Compose{RESET}:         chirp compose <text>");
    println!("  {GREEN}Replies{RESET}:         chirp reply <note|nevent|id> <text>");
    println!("  {GREEN}Reactions{RESET}:       chirp react <note|nevent|id> [+, -, emoji]");
    println!("  {GREEN}Profiles{RESET}:        chirp profile <npub|nprofile|hex>");
    println!("  {GREEN}Threads{RESET}:         chirp thread <note|nevent|id>");
    println!("  {GREEN}Search{RESET}:          chirp search #tag");
    println!("  {GREEN}Notifications{RESET}:   chirp notifications");
    println!(
        "  {GREEN}Relays/settings{RESET}: set-app-relays, set-indexer, set-budget, show relays"
    );
    println!("  {GREEN}Accounts{RESET}:        create-account, load-key, show state");
    println!(
        "  {GREEN}Marmot/MLS{RESET}:      mls-init, mls-status, mls-create, mls-invite, mls-send"
    );
    println!("  {YELLOW}Rule{RESET}: when Chirp gains a reusable surface, add a `chirp` alias or document the platform exception here.");
}

fn display_list(items: &[String]) -> String {
    if items.is_empty() {
        "<none>".to_string()
    } else {
        items.join(", ")
    }
}
