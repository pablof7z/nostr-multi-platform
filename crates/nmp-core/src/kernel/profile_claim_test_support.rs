//! Shared test helpers for the M2 registry-backed profile-claim test suites
//! (`profile_claim_tests` + `profile_claim_discovery_tests`).
//!
//! Split out of `profile_claim_tests.rs` (file-size gate, 500 LOC hard ceiling):
//! the discovery/probe/reconnect tests moved into `profile_claim_discovery_tests`,
//! so the REQ-inspection helpers both files use live here as one source of truth.

use super::*;

/// A 64-char lowercase-hex pubkey seeded from a short `prefix`.
pub(super) fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// Drain the planner and return only the REQ `OutboundMessage`s.
pub(super) fn drain_reqs(kernel: &mut Kernel) -> Vec<OutboundMessage> {
    kernel
        .drain_lifecycle_outbound()
        .into_iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect()
}

/// Relay URLs of REQ frames whose filter targets `pubkey` with kinds == [0].
pub(super) fn kind0_req_relays_for(reqs: &[OutboundMessage], pubkey: &str) -> Vec<String> {
    reqs.iter()
        .filter_map(|m| {
            let v: serde_json::Value = serde_json::from_str(&m.text).ok()?;
            let arr = v.as_array()?;
            if arr.first()?.as_str()? != "REQ" {
                return None;
            }
            let filter = arr.get(2)?;
            let kinds = filter.get("kinds")?.as_array()?;
            let is_kind0 = kinds.len() == 1 && kinds[0].as_u64() == Some(0);
            let authors = filter.get("authors")?.as_array()?;
            let has_author = authors.iter().any(|a| a.as_str() == Some(pubkey));
            (is_kind0 && has_author).then(|| m.relay_url.clone())
        })
        .collect()
}

/// True iff `reqs` contains a kind:10002 probe REQ whose authors include `pubkey`.
pub(super) fn has_10002_probe_for(reqs: &[OutboundMessage], pubkey: &str) -> bool {
    reqs.iter().any(|m| {
        let v: serde_json::Value = serde_json::from_str(&m.text).unwrap_or(serde_json::Value::Null);
        let Some(filter) = v.get(2) else { return false };
        let is_10002 = filter
            .get("kinds")
            .and_then(|k| k.as_array())
            .map(|k| k.iter().any(|x| x.as_u64() == Some(10002)))
            .unwrap_or(false);
        let has_author = filter
            .get("authors")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().any(|x| x.as_str() == Some(pubkey)))
            .unwrap_or(false);
        is_10002 && has_author
    })
}
