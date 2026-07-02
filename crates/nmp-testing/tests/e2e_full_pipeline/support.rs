//! Shared fixtures for the `e2e_full_pipeline` suite.
//!
//! Small, deterministic builders used across multiple pipeline-stage
//! scenarios: padded test pubkeys, mailbox-cache seeding, and wire-frame
//! extraction helpers for `SubscriptionLifecycle` compile output.

pub fn padded_pubkey(seed: &str) -> String {
    format!("{seed:0>64}").chars().take(64).collect()
}

pub fn put_write_mailbox(cache: &mut nmp_planner::InMemoryMailboxCache, author: String, relay: &str) {
    cache.put(
        author,
        nmp_planner::MailboxSnapshot {
            write_relays: vec![relay.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
}

pub fn req_relays(frames: &[nmp_core::subs::WireFrame]) -> Vec<&str> {
    frames
        .iter()
        .filter_map(|f| match f {
            nmp_core::subs::WireFrame::Req { relay_url, .. } => Some(relay_url.as_str()),
            _ => None,
        })
        .collect()
}

pub fn req_filters(frames: &[nmp_core::subs::WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            nmp_core::subs::WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect()
}
