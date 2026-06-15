//! Relay diagnostics discovery-kind classification.
//!
//! Renderers receive a pre-classified label; shell UI does not parse REQ
//! filter JSON or switch on Nostr discovery kind numbers.

use serde_json::Value;

use super::WireSubscriptionStatus;

const DISCOVERY_KINDS: &[u64] = &[0, 3, 10002];
const DISCOVERY_LIST_RANGE: std::ops::RangeInclusive<u64> = 10000..=19999;

pub(super) fn discovery_kinds_label_for_subs(subs: &[WireSubscriptionStatus]) -> String {
    let mut found: Vec<u64> = subs
        .iter()
        .filter(|sub| subscription_is_discovery_visible(&sub.state))
        .flat_map(|sub| kinds_from_filter_summary(&sub.filter_summary))
        .filter(|kind| is_discovery_kind(*kind))
        .collect();
    found.sort_unstable();
    found.dedup();

    if found.is_empty() {
        "none".to_string()
    } else {
        found
            .into_iter()
            .map(|kind| format!("{} ({})", discovery_kind_label(kind), kind))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn discovery_kind_label(kind: u64) -> &'static str {
    match kind {
        0 => "profile",
        3 => "follows",
        10002 => "relay-list",
        _ => "list",
    }
}

fn is_discovery_kind(kind: u64) -> bool {
    DISCOVERY_KINDS.contains(&kind) || DISCOVERY_LIST_RANGE.contains(&kind)
}

fn subscription_is_discovery_visible(state: &str) -> bool {
    let state = state.to_ascii_lowercase();
    !state.contains("closed") && !state.contains("closing")
}

fn kinds_from_filter_summary(filter_summary: &str) -> Vec<u64> {
    serde_json::from_str::<Value>(filter_summary)
        .ok()
        .and_then(|v| v.get("kinds").cloned())
        .and_then(|k| k.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|k| k.as_u64())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire_sub(filter_summary: &str, state: &str) -> WireSubscriptionStatus {
        WireSubscriptionStatus {
            wire_id: "sub".to_string(),
            relay_url: "wss://relay.example".to_string(),
            filter_summary: filter_summary.to_string(),
            state: state.to_string(),
            logical_consumer_count: 1,
            events_rx: 0,
            opened_at_ms: 0,
            last_event_at_ms: None,
            eose_at_ms: None,
            close_reason: None,
        }
    }

    #[test]
    fn discovery_kinds_label_classifies_open_filter_kinds() {
        let subs = vec![
            wire_sub(r#"{"kinds":[0,3],"authors":["aa"]}"#, "open"),
            wire_sub(r#"{"kinds":[10002,10003],"authors":["bb"]}"#, "opening"),
        ];

        assert_eq!(
            discovery_kinds_label_for_subs(&subs),
            "profile (0), follows (3), relay-list (10002), list (10003)"
        );
    }

    #[test]
    fn discovery_kinds_label_excludes_closed_and_non_discovery_subs() {
        let subs = vec![
            wire_sub(r#"{"kinds":[0]}"#, "closed"),
            wire_sub(r#"{"kinds":[1,6]}"#, "open"),
        ];

        assert_eq!(discovery_kinds_label_for_subs(&subs), "none");
    }
}
