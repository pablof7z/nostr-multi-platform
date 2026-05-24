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

fn active_giftwrap_interest(pubkey: &str) -> LogicalInterest {
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
    }
}

#[test]
fn active_giftwrap_inbox_uses_kind10050_relays_not_nip65_read_relays() {
    let account = pk("account");
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_read_relay_list(&kernel, &account, &["wss://public-read.example"]);
    kernel.seed_kind10050_for_test(&account, &["wss://dm-only.example/"]);

    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(active_giftwrap_interest(&account));
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
        "active gift-wrap inbox must subscribe through kind:10050 DM relays",
    );
    assert!(
        !req_relays.contains(&"wss://public-read.example"),
        "active gift-wrap inbox must not fall back to NIP-65 public read relays",
    );
}
