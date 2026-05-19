//! `set-dead <url>[,<url>...]` — mark relays as dead (skipped post-compile).

use crate::error::Result;
use crate::session::Session;
use crate::ws::normalize_url;

pub fn run(session: &mut Session, urls: Vec<String>) -> Result<()> {
    let mut count = 0usize;
    for u in urls {
        let n = normalize_url(&u);
        if !n.is_empty() {
            session.dead_relays.insert(n);
            count += 1;
        }
    }
    println!(
        "  dead_relays: {} total ({} added this command)",
        session.dead_relays.len(),
        count
    );
    Ok(())
}
