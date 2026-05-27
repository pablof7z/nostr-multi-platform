//! Generic DM-inbox routing tests — pins the kernel invariant that
//! `#p`-tagged interests carrying [`PTagRouting::Nip17DmRelays`] resolve
//! through the substrate's [`crate::substrate::DmInboxRelayLookup`] seam
//! and NOT through the per-pubkey NIP-65 read-relay list.
//!
//! The kernel itself is NIP-neutral: the `DmInboxRelayLookup` trait is a
//! generic per-pubkey "DM-inbox relays" capability. Concrete NIP bindings
//! (today: NIP-17 / kind:10050) live in downstream protocol crates such as
//! `nmp-nip17`. The `PTagRouting::Nip17DmRelays` discriminant is named in
//! `nmp-planner` (also outside `nmp-core`); the kernel only ever sees it
//! as "look up `#p` recipients through the inbox-lookup capability".
//!
//! These tests live under `kernel/` because the asserted behaviour is the
//! kernel's planner-tick routing decision, not a NIP-17-specific message
//! shape — moving them out of `nmp-core` would force a re-export of the
//! kernel's test fixtures.

use std::collections::BTreeMap;

use super::*;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, PTagRouting,
};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

fn pk(label: &str) -> String {
    format!("{label:0>64}").chars().take(64).collect()
}

fn seed_read_relay_list(kernel: &Kernel, account: &str, read: &[&str]) {
    kernel.seed_mailbox_relay_list(
        account,
        read.iter().map(|s| s.to_string()).collect(),
        Vec::new(),
        Vec::new(),
    );
}

/// Logical interest that requests `#p`-tagged gift-wrap (kind:1059) for the
/// active account. The kernel routes the resulting REQ through the
/// substrate's `DmInboxRelayLookup` (per `PTagRouting::Nip17DmRelays`),
/// not through the NIP-65 read-relay list — the very behaviour this file
/// pins.
fn active_dm_inbox_interest(pubkey: &str) -> LogicalInterest {
    let mut tags = BTreeMap::new();
    tags.insert("p".to_string(), [pubkey.to_string()].into_iter().collect());
    LogicalInterest {
        id: InterestId(1059),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: [1059].into_iter().collect(),
            tags,
            p_tag_routing: PTagRouting::Nip17DmRelays,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

#[test]
fn active_dm_inbox_uses_lookup_relays_not_nip65_read_relays() {
    let account = pk("account");
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_read_relay_list(&kernel, &account, &["wss://public-read.example"]);
    // Seed the kernel's substrate `DmInboxRelayLookup` (`TestDmInboxRelayCache`
    // under the hood). The helper still names kind:10050 because that is the
    // concrete NIP-17 binding the production composition wires in via
    // `nmp_nip17::DmRelayCache`; the kernel itself only sees the generic
    // lookup capability.
    kernel.seed_kind10050_for_test(&account, &["wss://dm-only.example/"]);

    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(active_dm_inbox_interest(&account));
    let frames = kernel.drain_lifecycle_tick();

    let req_relays: Vec<&str> = frames
        .iter()
        .filter_map(|frame| match frame {
            WireFrame::Req {
                relay_url,
                filter_json,
                ..
            } if filter_json.contains("\"kinds\":[1059]") && filter_json.contains("\"#p\"") => {
                Some(relay_url.as_str())
            }
            _ => None,
        })
        .collect();

    assert!(
        req_relays.contains(&"wss://dm-only.example"),
        "active DM-inbox interest must subscribe through the substrate DM-inbox lookup",
    );
    assert!(
        !req_relays.contains(&"wss://public-read.example"),
        "active DM-inbox interest must not fall back to NIP-65 public read relays",
    );
}
