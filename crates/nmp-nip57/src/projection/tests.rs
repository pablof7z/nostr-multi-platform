use super::*;
use std::sync::Arc;

/// Build a kind:9735 receipt with an explicit target `e` tag, sender, and
/// a bolt11 stub whose HRP encodes `msats` (matches the `bolt11::amount_msats`
/// helper conventions used elsewhere in the crate's tests).
fn receipt(id: &str, target: &str, msats: u64, sender: Option<&str>) -> KernelEvent {
    let mut tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["e".into(), target.into()],
        // `lnbc<n>n…` — `n` is the nano-BTC suffix; `amount_msats` reads
        // the integer prefix and scales. We use the same shape the `view`
        // tests use so the decoded amount equals `msats`.
        vec!["bolt11".into(), format!("lnbc{}n1pvj...", msats / 100)],
    ];
    if let Some(s) = sender {
        tags.push(vec!["P".into(), s.into()]);
    }
    KernelEvent {
        id: id.into(),
        author: "ln_node".into(),
        kind: 9735,
        created_at: 1,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn receipt_with_request_provider(
    id: &str,
    target: &str,
    request_id: &str,
    provider: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: provider.into(),
        kind: 9735,
        created_at: 1,
        tags: vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), target.into()],
            vec!["bolt11".into(), "lnbc10n1pvj...".into()],
            vec![
                "description".into(),
                format!(r#"{{"id":"{request_id}","pubkey":"sender","tags":[["amount","1000"]]}}"#),
            ],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn fresh_projection_yields_empty_snapshot() {
    let proj = ZapsAggregateProjection::new();
    assert_eq!(proj.snapshot(), ZapsAggregateSnapshot::empty());
    let json = proj.snapshot_json();
    assert_eq!(json, serde_json::json!({ "totals": {} }));
}

#[test]
fn one_receipt_is_indexed_under_its_target() {
    let proj = ZapsAggregateProjection::new();
    proj.on_kernel_event(&receipt("Z1", "NOTE", 15_000, Some("alice")));

    let snap = proj.snapshot();
    let count = snap
        .totals
        .get("NOTE")
        .expect("NOTE must be present after one receipt");
    assert_eq!(count.count, 1);
    assert_eq!(count.total_msats, 15_000);
}

#[test]
fn multiple_receipts_to_same_target_sum_and_count() {
    let proj = ZapsAggregateProjection::new();
    proj.on_kernel_event(&receipt("Z1", "NOTE", 10_000, Some("alice")));
    proj.on_kernel_event(&receipt("Z2", "NOTE", 20_000, Some("bob")));
    proj.on_kernel_event(&receipt("Z3", "NOTE", 30_000, Some("carol")));

    let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
    assert_eq!(count.count, 3);
    assert_eq!(count.total_msats, 60_000);
}

#[test]
fn receipts_to_different_targets_are_indexed_separately() {
    let proj = ZapsAggregateProjection::new();
    proj.on_kernel_event(&receipt("Z1", "NOTE_A", 10_000, Some("alice")));
    proj.on_kernel_event(&receipt("Z2", "NOTE_B", 25_000, Some("bob")));
    proj.on_kernel_event(&receipt("Z3", "NOTE_A", 5_000, Some("carol")));

    let snap = proj.snapshot();
    let a = snap.totals.get("NOTE_A").expect("NOTE_A present");
    let b = snap.totals.get("NOTE_B").expect("NOTE_B present");
    assert_eq!(a.count, 2);
    assert_eq!(a.total_msats, 15_000);
    assert_eq!(b.count, 1);
    assert_eq!(b.total_msats, 25_000);
}

#[test]
fn duplicate_receipt_id_does_not_double_count() {
    let proj = ZapsAggregateProjection::new();
    let r = receipt("Z1", "NOTE", 15_000, Some("alice"));
    proj.on_kernel_event(&r);
    proj.on_kernel_event(&r);
    proj.on_kernel_event(&r);

    let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
    assert_eq!(count.count, 1, "re-delivered receipt must not duplicate");
    assert_eq!(count.total_msats, 15_000);
}

#[test]
fn receipt_from_unexpected_provider_is_not_aggregated() {
    let request_id = "projection-provider-mismatch";
    crate::pending::active_pending_zap_registry()
        .remember_expected_provider(request_id, "a".repeat(64))
        .expect("valid expected provider");
    let proj = ZapsAggregateProjection::new();
    proj.on_kernel_event(&receipt_with_request_provider(
        "Z-provider-mismatch",
        "NOTE",
        request_id,
        &"b".repeat(64),
    ));
    assert!(
        proj.snapshot().totals.is_empty(),
        "receipt author that differs from LNURL nostrPubkey must not count"
    );
}

#[test]
fn non_receipt_kinds_are_ignored() {
    let proj = ZapsAggregateProjection::new();
    let note = KernelEvent {
        id: "N1".into(),
        author: "alice".into(),
        kind: 1,
        created_at: 1,
        tags: vec![vec!["e".into(), "NOTE".into()]],
        content: "hello".into(),
        relay_provenance: Vec::new(),
    };
    let request = KernelEvent {
        id: "ZR".into(),
        author: "alice".into(),
        kind: 9734,
        created_at: 1,
        tags: vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), "NOTE".into()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&note);
    proj.on_kernel_event(&request);

    assert!(
        proj.snapshot().totals.is_empty(),
        "non-9735 events must not contribute"
    );
}

#[test]
fn receipt_without_e_tag_is_ignored() {
    let proj = ZapsAggregateProjection::new();
    let profile_zap = KernelEvent {
        id: "ZP".into(),
        author: "ln_node".into(),
        kind: 9735,
        created_at: 1,
        tags: vec![
            vec!["p".into(), "recipient".into()],
            vec!["bolt11".into(), "lnbc10n1pvj...".into()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&profile_zap);
    assert!(proj.snapshot().totals.is_empty());
}

#[test]
fn receipt_with_no_parseable_amount_counts_but_contributes_zero_msats() {
    let proj = ZapsAggregateProjection::new();
    let no_amount = KernelEvent {
        id: "ZN".into(),
        author: "ln_node".into(),
        kind: 9735,
        created_at: 1,
        tags: vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), "NOTE".into()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&no_amount);

    let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
    assert_eq!(count.count, 1);
    assert_eq!(count.total_msats, 0);
}

#[test]
fn snapshot_json_shape_is_a_named_totals_field() {
    let proj = ZapsAggregateProjection::new();
    proj.on_kernel_event(&receipt("Z1", "NOTE", 15_000, Some("alice")));

    let json = proj.snapshot_json();
    let totals = json
        .get("totals")
        .and_then(|t| t.as_object())
        .expect("snapshot json has a `totals` object");
    let note = totals
        .get("NOTE")
        .and_then(|n| n.as_object())
        .expect("totals contains NOTE");
    assert_eq!(
        note.get("total_msats").and_then(|v| v.as_u64()),
        Some(15_000)
    );
    assert_eq!(note.get("count").and_then(|v| v.as_u64()), Some(1));
}

#[test]
fn round_trips_through_serde() {
    let proj = ZapsAggregateProjection::new();
    proj.on_kernel_event(&receipt("Z1", "NOTE", 15_000, Some("alice")));
    proj.on_kernel_event(&receipt("Z2", "NOTE", 25_000, Some("bob")));
    let snap = proj.snapshot();
    let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
    let decoded: ZapsAggregateSnapshot =
        serde_json::from_str(&encoded).expect("snapshot deserialises");
    assert_eq!(snap, decoded);
}

#[test]
fn drives_through_observer_trait_object() {
    let proj = Arc::new(ZapsAggregateProjection::new());
    let observer: Arc<dyn KernelEventObserver> = Arc::clone(&proj) as _;
    observer.on_kernel_event(&receipt("Z1", "NOTE", 10_000, Some("alice")));
    let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
    assert_eq!(count.count, 1);
    assert_eq!(count.total_msats, 10_000);
}
