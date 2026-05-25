use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;

use nmp_core::{
    substrate::{
        AppRelayMode, ClassRoutingPath, Direction, EventClass, RoutingRelayUrl, RoutingSource,
        UserConfiguredCategory,
    },
    PublishTraceEntry, RoutingTraceProjection, SubscriptionTraceEntry,
};

use crate::session::Session;

const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn banner() {
    println!("{BLUE}chirp-repl{RESET} Chirp app runtime console");
    println!("type {GREEN}help{RESET}, {GREEN}parity{RESET}, or {GREEN}quit{RESET}");
}

pub fn help() {
    println!("{BLUE}commands{RESET}");
    println!("  identity:     create-account [name], load-key <nsec|hex>");
    println!("  relays:       set-relays <url...>, set-indexers <url...>");
    println!("  read:         sync-follows, home, profile <npub>, thread <note>, search #tag");
    println!("  write:        compose <text>, reply <note> <text>, react <note> [reaction]");
    println!("  social graph: follow <npub>, unfollow <npub>");
    println!("  dms:          send-dm <npub|nprofile|hex> <text> (alias: dm)");
    println!("  mls:          mls-init, mls-status, mls-create, mls-invite");
    println!("                mls-accept, mls-send, mls-messages");
    println!("  diagnostics:  diagnostics, parity, routing-trace");
}

pub fn parity() {
    println!("{BLUE}chirp app surface parity{RESET}");
    println!("  {GREEN}Home feed{RESET}:       home");
    println!("  {GREEN}Compose{RESET}:         compose <text>");
    println!("  {GREEN}Replies{RESET}:         reply <note|nevent|id> <text>");
    println!("  {GREEN}Reactions{RESET}:       react <note|nevent|id> [+, -, emoji]");
    println!("  {GREEN}NIP-17 DMs{RESET}:      send-dm <npub|nprofile|hex> <text>");
    println!("  {GREEN}Profiles{RESET}:        profile <npub|nprofile|hex>");
    println!("  {GREEN}Threads{RESET}:         thread <note|nevent|id>");
    println!("  {GREEN}Search{RESET}:          search #tag");
    println!("  {YELLOW}Notifications{RESET}:   not exposed by Chirp runtime yet");
    println!("  {GREEN}Relays/settings{RESET}: set-relays, set-indexers, diagnostics");
    println!("  {GREEN}Accounts{RESET}:        create-account, load-key");
    println!("  {GREEN}MLS groups{RESET}:      mls-* via Chirp Marmot projection");
    println!(
        "  {YELLOW}Contract{RESET}: new Chirp app surfaces should add chirp-repl coverage too."
    );
}

pub fn diagnostics(session: &Session) {
    println!("{BLUE}diagnostics{RESET}");
    println!(
        "  identity:   {}",
        session.pubkey_hex.as_deref().unwrap_or("<none>")
    );
    println!("  relays:     {}", list(&session.relays));
    println!("  indexers:   {}", list(&session.indexers));
    println!("  wall:       {:?}", session.wall);
    if let Some(last) = &session.last_run {
        println!(
            "  last run:   {}: {} relays, {} events, {} new",
            last.label, last.relays, last.events, last.new_events
        );
    } else {
        println!("  last run:   <none>");
    }
}

pub fn status_ok(message: &str) {
    println!("{GREEN}ok{RESET} {message}");
}

pub fn status_warn(message: &str) {
    println!("{YELLOW}warn{RESET} {message}");
}

pub fn status_err(message: &str) {
    eprintln!("{RED}error{RESET} {message}");
}

pub fn event(event: &Value) {
    let id = short(event.get("id").and_then(Value::as_str).unwrap_or("?"));
    let author = short(event.get("pubkey").and_then(Value::as_str).unwrap_or("?"));
    let kind = event.get("kind").and_then(Value::as_u64).unwrap_or(0);
    let content = event.get("content").and_then(Value::as_str).unwrap_or("");
    println!(
        "{DIM}{id}{RESET} kind:{kind:<5} author:{author} {}",
        compact(content)
    );
}

pub fn chirp_snapshot(snapshot: &Value, blocks: usize, cards: usize) {
    println!("{BLUE}chirp snapshot{RESET} blocks:{blocks} cards:{cards}");
    if let Some(items) = snapshot.get("cards").and_then(Value::as_array) {
        for item in items.iter().take(20) {
            event(item);
        }
    }
}

pub fn marmot_snapshot(snapshot: &Value) {
    let groups = snapshot
        .get("groups")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let welcomes = snapshot
        .get("pending_welcomes")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    println!("{BLUE}marmot snapshot{RESET} groups:{groups} pending_welcomes:{welcomes}");
    if let Some(items) = snapshot.get("groups").and_then(Value::as_array) {
        for group in items {
            let id = short(group.get("id_hex").and_then(Value::as_str).unwrap_or("?"));
            let name = group.get("name").and_then(Value::as_str).unwrap_or("");
            let members = group
                .get("members")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            println!("  {DIM}{id}{RESET} {name} members:{members}");
        }
    }
}

pub fn marmot_result(label: &str, value: &Value) {
    println!("{BLUE}{label}{RESET}");
    if let Some(group_id) = value.get("group_id_hex").and_then(Value::as_str) {
        println!("  group_id: {}", short(group_id));
    }
    if let Some(event_id) = value.get("event_id").and_then(Value::as_str) {
        println!("  event_id: {}", short(event_id));
    }
    if let Some(needs) = value.get("needs").and_then(Value::as_array) {
        let needs = needs
            .iter()
            .filter_map(Value::as_str)
            .map(short)
            .collect::<Vec<_>>();
        println!("  needs:    {}", needs.join(", "));
    }
    if let Some(events) = value.get("events").and_then(Value::as_array) {
        println!("  events:   {}", events.len());
    }
    if let Some(welcomes) = value.get("welcome_rumors").and_then(Value::as_array) {
        println!("  welcomes: {}", welcomes.len());
    }
    println!(
        "  ok:       {}",
        value.get("ok").and_then(Value::as_bool).unwrap_or(true)
    );
}

pub fn marmot_messages(rows: &Value) {
    let Some(items) = rows.as_array() else {
        println!("{BLUE}marmot messages{RESET} <invalid>");
        return;
    };
    println!("{BLUE}marmot messages{RESET} count:{}", items.len());
    for row in items.iter().take(20) {
        let sender = short(
            row.get("sender_npub")
                .and_then(Value::as_str)
                .unwrap_or("?"),
        );
        let content = row.get("content").and_then(Value::as_str).unwrap_or("");
        println!("  {DIM}{sender}{RESET} {}", compact(content));
    }
}

pub fn publish_result(label: &str, id: &str, ok: usize, fail: usize) {
    println!("{BLUE}{label}{RESET} {}", short(id));
    println!("  {GREEN}accepted/sent:{RESET} {ok}   {YELLOW}failed:{RESET} {fail}");
}

/// V-51 phase 4 — pretty-print the kernel's routing-trace ring buffers.
/// Each entry: kind, author (truncated), resolved relay URL, per-URL lane
/// attribution. Empty rings render as `<no recent X>`.
pub fn routing_trace(projection: Option<&Arc<RoutingTraceProjection>>) {
    let Some(projection) = projection else {
        println!(
            "{YELLOW}routing-trace{RESET} kernel not started yet (no projection slot bound)"
        );
        return;
    };
    let publishes = projection.snapshot_publishes();
    let subs = projection.snapshot_subscriptions();
    println!(
        "{BLUE}routing-trace{RESET} publishes:{} subscriptions:{} (capacity:{})",
        publishes.len(),
        subs.len(),
        projection.capacity()
    );
    render_publishes(&publishes);
    render_subscriptions(&subs);
}

fn render_publishes(entries: &[PublishTraceEntry]) {
    println!("{BLUE}  recent publishes{RESET}");
    if entries.is_empty() {
        println!("    {DIM}<no recent publishes>{RESET}");
        return;
    }
    for entry in entries.iter().rev() {
        let author = short(&entry.trace.author);
        let explicit = if entry.trace.explicit_targets_set {
            " [explicit-targets]"
        } else {
            ""
        };
        println!(
            "    kind:{:<5} author:{author}{explicit}",
            entry.trace.kind
        );
        render_urls(&entry.urls);
    }
}

fn render_subscriptions(entries: &[SubscriptionTraceEntry]) {
    println!("{BLUE}  recent subscriptions{RESET}");
    if entries.is_empty() {
        println!("    {DIM}<no recent subscriptions>{RESET}");
        return;
    }
    for entry in entries.iter().rev() {
        let explicit = if entry.trace.explicit_targets_set {
            " [explicit-targets]"
        } else {
            ""
        };
        println!(
            "    interest:{} kinds:{:?} authors:{}{explicit}",
            entry.trace.interest_id, entry.trace.kinds, entry.trace.authors_count
        );
        render_urls(&entry.urls);
    }
}

fn render_urls(urls: &[(RoutingRelayUrl, BTreeSet<RoutingSource>)]) {
    if urls.is_empty() {
        println!("      {YELLOW}(no relays resolved){RESET}");
        return;
    }
    for (url, sources) in urls {
        let lanes = sources
            .iter()
            .map(format_lane)
            .collect::<Vec<_>>()
            .join(", ");
        println!("      {GREEN}{url}{RESET}  {DIM}[{lanes}]{RESET}");
    }
}

/// Pretty-print a single [`RoutingSource`] for the trace output. Compact,
/// greppable form: `Nip65/Write`, `AppRelay/Fallback`, `ClassRouted/Explicit`,
/// etc. The shell smoke (`scripts/validate-routing.sh`) grep-asserts on these
/// labels so the format is load-bearing — do not casually rename a lane.
fn format_lane(source: &RoutingSource) -> String {
    match source {
        RoutingSource::Nip65 { direction } => format!(
            "Nip65/{}",
            match direction {
                Direction::Read => "Read",
                Direction::Write => "Write",
            }
        ),
        RoutingSource::Hint => "Hint".into(),
        RoutingSource::Provenance => "Provenance".into(),
        RoutingSource::UserConfigured(cat) => format!(
            "UserConfigured/{}",
            match cat {
                UserConfiguredCategory::ActiveAccountRead => "ActiveAccountRead",
                UserConfiguredCategory::ActiveAccountWrite => "ActiveAccountWrite",
                UserConfiguredCategory::Debug => "Debug",
            }
        ),
        RoutingSource::ClassRouted { class, via } => format!(
            "ClassRouted/{}/{}",
            match class {
                EventClass::Search => "Search".into(),
                EventClass::Draft => "Draft".into(),
                EventClass::Wiki => "Wiki".into(),
                EventClass::Other(name) => format!("Other({name})"),
            },
            match via {
                ClassRoutingPath::Explicit => "Explicit",
                ClassRoutingPath::Nip51 => "Nip51",
            }
        ),
        RoutingSource::Indexer => "Indexer".into(),
        RoutingSource::AppRelay { mode } => format!(
            "AppRelay/{}",
            match mode {
                AppRelayMode::Fallback => "Fallback",
                AppRelayMode::Always => "Always",
            }
        ),
    }
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "<none>".into()
    } else {
        items.join(", ")
    }
}

fn short(input: &str) -> String {
    if input.len() <= 12 {
        input.into()
    } else {
        format!("{}..{}", &input[..8], &input[input.len() - 4..])
    }
}

fn compact(input: &str) -> String {
    let s = input.replace('\n', " ");
    if s.len() > 120 {
        format!("{}...", &s[..117])
    } else {
        s
    }
}
