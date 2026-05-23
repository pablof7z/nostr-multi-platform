//! `set-app-relays <url>[,<url>...]` — override the planner's `app_relays`
//! fallback list (default empty).

use crate::error::Result;
use crate::session::Session;
use crate::ws::normalize_url;

pub fn run(session: &mut Session, urls: Vec<String>) -> Result<()> {
    let normalised: Vec<String> = urls
        .iter()
        .map(|u| normalize_url(u))
        .filter(|s| !s.is_empty())
        .collect();
    println!("  app_relays: {}", normalised.join(", "));
    session.app_relays = normalised;
    Ok(())
}
