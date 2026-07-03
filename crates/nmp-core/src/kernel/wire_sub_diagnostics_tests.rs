use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use crate::relay::{CanonicalRelayUrl, DEFAULT_VISIBLE_LIMIT};
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope, WireFrame};
use crate::time::Instant;
use nmp_network::role::RelayRole;

use super::super::wire_sub::WireSub;
use super::super::Kernel;

const RELAY: &str = "wss://relay.example/";

fn kernel() -> Kernel {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.start();
    kernel
}

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

fn open_sub(kernel: &mut Kernel, sub_id: &str, state: &str) {
    kernel.insert_wire_sub(
        RelayRole::Content,
        CanonicalRelayUrl::parse_or_raw(RELAY),
        sub_id.to_string(),
        "kinds=[1]".to_string(),
        state,
        None,
    );
}

/// Directly seed lifecycle state (EOSE / events / close) on an already-open
/// wire sub. The real ingest path sets these via EOSE / EVENT / CLOSED frame
/// handlers; the seam only reads them, so seeding is faithful for this test.
fn mutate_sub(kernel: &mut Kernel, sub_id: &str, f: impl Fn(&mut WireSub)) {
    for sub in kernel.wire.subs.values_mut() {
        if sub.id == sub_id {
            f(sub);
        }
    }
}

fn row<'a>(
    rows: &'a [super::WireSubscriptionDiagnosticSnapshot],
    wire_id: &str,
) -> &'a super::WireSubscriptionDiagnosticSnapshot {
    rows.iter()
        .find(|r| r.wire_id == wire_id)
        .expect("row present")
}

fn follow_interest(id: u64, authors: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|author| pubkey(author)).collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

fn register_owner(
    kernel: &mut Kernel,
    key: SubKey,
    owner: &str,
    interest: LogicalInterest,
) -> SubIdentity {
    let identity = SubIdentity::new(SubOwnerKey::new(owner), key, SubScope::Global);
    let token = RegistryWriteToken::for_test();
    let _ = kernel.lifecycle.registry_mut().apply(
        &token,
        InterestWrite::EnsureAbsent,
        identity.clone(),
        interest,
    );
    identity
}

#[test]
fn disabled_returns_empty_without_walking_subs() {
    let mut kernel = kernel();
    open_sub(&mut kernel, "sub-a", "open");
    assert!(kernel.wire_subscription_diagnostics(false).is_empty());
}

#[test]
fn acceptance_scenarios_produce_neutral_rows() {
    let mut kernel = kernel();

    open_sub(&mut kernel, "sub-a", "open");

    open_sub(&mut kernel, "sub-b", "open");
    mutate_sub(&mut kernel, "sub-b", |s| s.events_rx = 3);

    open_sub(&mut kernel, "sub-c", "opening");
    mutate_sub(&mut kernel, "sub-c", |s| {
        s.state = "closed".to_string();
        s.close_reason = Some("last-owner-dropped".to_string());
    });

    open_sub(&mut kernel, "sub-d", "open");
    mutate_sub(&mut kernel, "sub-d", |s| s.eose_at = Some(Instant::now()));

    let rows = kernel.wire_subscription_diagnostics(true);
    assert_eq!(rows.len(), 4);

    let a = row(&rows, "sub-a");
    assert_eq!(a.state, "open");
    assert!(!a.eose_observed);
    assert_eq!(a.events_rx, 0);
    assert!(a.close_reason.is_none());
    assert_eq!(a.relay_url, "wss://relay.example");

    let b = row(&rows, "sub-b");
    assert_eq!(b.state, "open");
    assert!(b.close_reason.is_none());
    assert_eq!(b.events_rx, 3);

    let c = row(&rows, "sub-c");
    assert_eq!(c.state, "closed");
    assert_eq!(c.close_reason.as_deref(), Some("last-owner-dropped"));

    let d = row(&rows, "sub-d");
    assert_eq!(d.state, "open");
    assert!(d.eose_observed);
    assert_eq!(d.events_rx, 0);
}

#[test]
fn diagnostics_join_current_plan_origins_to_registry_owner_counts() {
    let mut kernel = kernel();
    kernel
        .lifecycle
        .set_selection_budget(usize::MAX, usize::MAX);

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("aa01"),
        MailboxSnapshot {
            write_relays: vec!["wss://relay-a.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    cache.put(
        pubkey("bb02"),
        MailboxSnapshot {
            write_relays: vec!["wss://relay-b.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let interest = follow_interest(991, &["aa01", "bb02"]);
    let key = SubKey::new("xray-refcounted-follow-interest");
    let owner_a = register_owner(&mut kernel, key, "feed-owner-a", interest.clone());
    let owner_b = register_owner(&mut kernel, key, "feed-owner-b", interest);

    let frames = kernel
        .lifecycle
        .recompile_and_diff(&cache)
        .expect("compile");
    assert!(
        frames
            .iter()
            .any(|frame| matches!(frame, WireFrame::Req { .. })),
        "compile must produce real content REQ frames"
    );
    for frame in kernel.lifecycle.current_plan_frames() {
        let WireFrame::Req {
            relay_url,
            sub_id,
            filter_json,
            ..
        } = frame
        else {
            continue;
        };
        kernel.insert_wire_sub(
            RelayRole::Content,
            CanonicalRelayUrl::parse_or_raw(&relay_url),
            sub_id,
            filter_json,
            "open",
            None,
        );
    }

    let rows = kernel.wire_subscription_diagnostics(true);
    assert_eq!(
        rows.len(),
        2,
        "author partitioning should produce one wire row per write relay"
    );
    for row in &rows {
        assert_eq!(row.originating_interest_ids, vec![991]);
        assert_eq!(row.consumer_count, 2);
    }

    assert!(!kernel.lifecycle.registry_mut().drop_owner(&owner_a));
    let rows = kernel.wire_subscription_diagnostics(true);
    for row in &rows {
        assert_eq!(row.originating_interest_ids, vec![991]);
        assert_eq!(row.consumer_count, 1);
    }

    assert!(kernel.lifecycle.registry_mut().drop_owner(&owner_b));
    let rows = kernel.wire_subscription_diagnostics(true);
    for row in &rows {
        assert_eq!(row.originating_interest_ids, vec![991]);
        assert_eq!(row.consumer_count, 0);
    }
}
