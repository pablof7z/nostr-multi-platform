//! `set-indexer <url>[,<url>...]` — override the indexer relay set used
//! for kind:3 / kind:10002 discovery.

use crate::error::Result;
use crate::session::Session;
use crate::ws::normalize_url;

pub fn run(session: &mut Session, urls: Vec<String>) -> Result<()> {
    let normalised: Vec<String> = urls
        .iter()
        .map(|u| normalize_url(u))
        .filter(|s| !s.is_empty())
        .collect();
    if normalised.is_empty() {
        println!("  (no valid URLs; indexer set unchanged)");
        return Ok(());
    }
    println!("  indexer: {}", normalised.join(", "));
    session.indexer_relays = normalised;
    Ok(())
}
