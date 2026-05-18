//! `show <state|relays|budget|seen>` — dump session state.

use crate::ast::ShowTopic;
use crate::error::Result;
use crate::session::Session;

pub fn run(session: &Session, topic: ShowTopic) -> Result<()> {
    match topic {
        ShowTopic::State => print_state(session),
        ShowTopic::Relays => print_relays(session),
        ShowTopic::Budget => print_budget(session),
        ShowTopic::Seen => print_seen(session),
    }
    Ok(())
}

fn print_state(session: &Session) {
    println!("  seed_hex:        {}", session.seed_hex.as_deref().unwrap_or("<none>"));
    let f = session
        .follows_cache
        .as_ref()
        .map(|s| s.len().to_string())
        .unwrap_or_else(|| "<none>".to_string());
    println!("  follows_cache:   {f}");
    println!("  mailbox_cache:   {} authors", session.mailbox_cache.len());
    println!("  indexer_relays:  {}", session.indexer_relays.join(", "));
    println!(
        "  app_relays:      {}",
        if session.app_relays.is_empty() {
            "<none>".to_string()
        } else {
            session.app_relays.join(", ")
        }
    );
    println!(
        "  dead_relays:     {}",
        if session.dead_relays.is_empty() {
            "<none>".to_string()
        } else {
            session.dead_relays.iter().cloned().collect::<Vec<_>>().join(", ")
        }
    );
    println!("  max_connections: {}", session.max_connections);
    println!("  max_per_user:    {}", session.max_per_user);
    println!("  wall:            {:?}", session.wall);
    println!("  seen_ids:        {} events", session.seen_ids.len());
    match &session.last_run {
        Some(r) => {
            println!(
                "  last_run:        {} relays, {} authors-on-wire, {} unroutable, {} events ({} new) in {:?}",
                r.relays_used,
                r.authors_on_wire,
                r.unroutable,
                r.events_total,
                r.events_new,
                r.wall
            );
        }
        None => println!("  last_run:        <none>"),
    }
}

fn print_relays(session: &Session) {
    println!("  indexer: {}", session.indexer_relays.join(", "));
    println!(
        "  app:     {}",
        if session.app_relays.is_empty() {
            "<none>".to_string()
        } else {
            session.app_relays.join(", ")
        }
    );
    println!(
        "  dead:    {}",
        if session.dead_relays.is_empty() {
            "<none>".to_string()
        } else {
            session.dead_relays.iter().cloned().collect::<Vec<_>>().join(", ")
        }
    );
}

fn print_budget(session: &Session) {
    println!("  max_connections: {}", session.max_connections);
    println!("  max_per_user:    {}", session.max_per_user);
    println!("  wall:            {:?}", session.wall);
}

fn print_seen(session: &Session) {
    println!("  seen_ids: {} unique event ids this session", session.seen_ids.len());
}
