use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::profiles;
use crate::session::Session;

const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn banner() {
    println!("{BLUE}chirp-repl{RESET} standalone Chirp diagnostics");
    println!("type {GREEN}help{RESET}, {GREEN}parity{RESET}, or {GREEN}quit{RESET}");
}

pub fn help() {
    println!("{BLUE}commands{RESET}");
    println!("  identity:     create-account [name], load-key <nsec|hex>");
    println!("  relays:       set-relays <url...>, set-indexers <url...>");
    println!("  read:         sync-follows, home, notifications, profile <npub>, thread <note>, search #tag");
    println!("  write:        compose <text>, reply <note> <text>, react <note> [reaction]");
    println!("  social graph: follow <npub>, unfollow <npub>");
    println!("  diagnostics:  diagnostics, parity, raw-req <json-filter>");
}

pub fn parity() {
    println!("{BLUE}chirp surface parity{RESET}");
    println!("  {GREEN}Home feed{RESET}:       home");
    println!("  {GREEN}Compose{RESET}:         compose <text>");
    println!("  {GREEN}Replies{RESET}:         reply <note|nevent|id> <text>");
    println!("  {GREEN}Reactions{RESET}:       react <note|nevent|id> [+, -, emoji]");
    println!("  {GREEN}Profiles{RESET}:        profile <npub|nprofile|hex>");
    println!("  {GREEN}Threads{RESET}:         thread <note|nevent|id>");
    println!("  {GREEN}Search{RESET}:          search #tag");
    println!("  {GREEN}Notifications{RESET}:   notifications");
    println!("  {GREEN}Relays/settings{RESET}: set-relays, set-indexers, diagnostics");
    println!("  {GREEN}Accounts{RESET}:        create-account, load-key");
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
    println!("  follows:    {}", session.follows.len());
    println!("  seen ids:   {}", session.seen_ids.len());
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

pub fn event_with_profiles(session: &Session, value: &Value) {
    if value.get("kind").and_then(Value::as_u64) == Some(1) {
        let author = profiles::event_author_label(session, value);
        print!("{}", format_note_event(value, &author, now_secs()));
    } else {
        event(value);
    }
}

pub fn publish_result(label: &str, id: &str, ok: usize, fail: usize) {
    println!("{BLUE}{label}{RESET} {}", short(id));
    println!("  {GREEN}accepted/sent:{RESET} {ok}   {YELLOW}failed:{RESET} {fail}");
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

fn format_note_event(event: &Value, author: &str, now: u64) -> String {
    let id = short(event.get("id").and_then(Value::as_str).unwrap_or("?"));
    let created_at = event.get("created_at").and_then(Value::as_u64).unwrap_or(0);
    let content = event.get("content").and_then(Value::as_str).unwrap_or("");
    format!(
        "{GREEN}{author}{RESET}   {DIM}{}{RESET}\n\n{}\n{DIM}{id}{RESET}\n\n",
        relative_age(created_at, now),
        content.trim_end()
    )
}

fn relative_age(created_at: u64, now: u64) -> String {
    let elapsed = now.saturating_sub(created_at);
    match elapsed {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m ago", elapsed / 60),
        3_600..=86_399 => format!("{}h ago", elapsed / 3_600),
        86_400..=2_591_999 => format!("{}d ago", elapsed / 86_400),
        2_592_000..=31_535_999 => format!("{}mo ago", elapsed / 2_592_000),
        _ => format!("{}y ago", elapsed / 31_536_000),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_notes_with_author_and_relative_age() {
        let event = json!({
            "id":"2222222222222222222222222222222222222222222222222222222222222222",
            "kind":1,
            "created_at":86_400,
            "content":"hello\nworld"
        });

        let rendered = format_note_event(&event, "@pablof7z", 172_800);

        assert!(rendered.contains("@pablof7z"));
        assert!(rendered.contains("1d ago"));
        assert!(rendered.contains("hello\nworld"));
    }

    #[test]
    fn relative_age_uses_compact_units() {
        assert_eq!(relative_age(100, 100), "now");
        assert_eq!(relative_age(40, 100), "1m ago");
        assert_eq!(relative_age(3_600, 7_200), "1h ago");
    }
}
