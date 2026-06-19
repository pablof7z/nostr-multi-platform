use super::*;

fn active_slot(pubkey: &str) -> Arc<Mutex<Option<String>>> {
    Arc::new(Mutex::new(Some(pubkey.to_string())))
}

fn event(author: &str, d_tag: &str, content: &str, created_at: u64, id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_APP_DATA,
        created_at,
        tags: vec![vec!["d".to_string(), d_tag.to_string()]],
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn builder_emits_kind_30078_with_d_tag_first() {
    let built = build_app_data_event(
        "aa",
        "com.example/settings",
        r#"{"theme":"dark"}"#,
        123,
        vec![vec!["client".to_string(), "nmp".to_string()]],
    )
    .unwrap();

    assert_eq!(built.pubkey, "aa");
    assert_eq!(built.kind, KIND_APP_DATA);
    assert_eq!(built.created_at, 123);
    assert_eq!(built.content, r#"{"theme":"dark"}"#);
    assert_eq!(built.tags[0], vec!["d", "com.example/settings"]);
    assert_eq!(built.tags[1], vec!["client", "nmp"]);
}

#[test]
fn builder_rejects_missing_or_duplicate_d_tag() {
    assert_eq!(
        build_app_data_event("aa", "", "", 1, Vec::new()).unwrap_err(),
        AppDataError::EmptyDTag
    );
    assert_eq!(
        build_app_data_event(
            "aa",
            "key",
            "",
            1,
            vec![vec!["d".to_string(), "other".to_string()]]
        )
        .unwrap_err(),
        AppDataError::InvalidExtraTag
    );
    assert_eq!(
        build_app_data_event("aa", "key", "", 1, vec![Vec::new()]).unwrap_err(),
        AppDataError::InvalidExtraTag
    );
}

#[test]
fn projection_ingests_active_account_app_data() {
    let projection = AppDataProjection::new(active_slot("aa"));

    projection.on_kernel_event(&event("aa", "settings", "v1", 10, "10"));

    let snapshot = projection.snapshot();
    assert_eq!(snapshot.owner_pubkey.as_deref(), Some("aa"));
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].d_tag, "settings");
    assert_eq!(snapshot.records[0].content, "v1");
    assert_eq!(projection.get("settings").unwrap().event_id, "10");
}

#[test]
fn projection_filters_non_active_accounts_and_non_app_data() {
    let projection = AppDataProjection::new(active_slot("aa"));
    let mut wrong_kind = event("aa", "settings", "wrong", 10, "10");
    wrong_kind.kind = 1;

    projection.on_kernel_event(&event("bb", "settings", "foreign", 11, "11"));
    projection.on_kernel_event(&wrong_kind);

    assert!(projection.snapshot().records.is_empty());
    assert!(projection.get("settings").is_none());
}

#[test]
fn projection_ignores_events_without_valid_d_tag() {
    let projection = AppDataProjection::new(active_slot("aa"));
    let mut missing = event("aa", "settings", "missing", 10, "10");
    missing.tags.clear();
    let mut empty = event("aa", "", "empty", 11, "11");
    empty.tags = vec![vec!["d".to_string(), String::new()]];

    projection.on_kernel_event(&missing);
    projection.on_kernel_event(&empty);

    assert!(projection.snapshot().records.is_empty());
}

#[test]
fn replaceable_supersession_keeps_latest_record_for_d_tag() {
    let projection = AppDataProjection::new(active_slot("aa"));

    projection.on_kernel_event(&event("aa", "settings", "v1", 10, "20"));
    projection.on_kernel_event(&event("aa", "settings", "stale", 9, "09"));
    projection.on_kernel_event(&event("aa", "settings", "v2", 11, "30"));

    let record = projection.get("settings").unwrap();
    assert_eq!(record.content, "v2");
    assert_eq!(record.created_at, 11);
}

#[test]
fn equal_timestamp_supersession_uses_lowest_event_id() {
    let projection = AppDataProjection::new(active_slot("aa"));

    projection.on_kernel_event(&event("aa", "settings", "id-b", 10, "bb"));
    projection.on_kernel_event(&event("aa", "settings", "id-a", 10, "aa"));
    projection.on_kernel_event(&event("aa", "settings", "id-c", 10, "cc"));

    let record = projection.get("settings").unwrap();
    assert_eq!(record.content, "id-a");
    assert_eq!(record.event_id, "aa");
}

#[test]
fn account_switch_hides_stale_records_until_new_account_arrives() {
    let slot = active_slot("aa");
    let projection = AppDataProjection::new(Arc::clone(&slot));
    projection.on_kernel_event(&event("aa", "settings", "aa-data", 10, "10"));

    *slot.lock().unwrap() = Some("bb".to_string());

    assert!(projection.snapshot().records.is_empty());
    assert!(projection.get("settings").is_none());

    projection.on_kernel_event(&event("bb", "settings", "bb-data", 11, "11"));
    assert_eq!(projection.get("settings").unwrap().content, "bb-data");
}

#[test]
fn projection_is_bounded_and_evicts_stale_records() {
    let projection = AppDataProjection::with_max_records(active_slot("aa"), 2);

    projection.on_kernel_event(&event("aa", "old", "old", 1, "01"));
    projection.on_kernel_event(&event("aa", "middle", "middle", 2, "02"));
    projection.on_kernel_event(&event("aa", "new", "new", 3, "03"));

    let snapshot = projection.snapshot();
    let keys: Vec<_> = snapshot
        .records
        .iter()
        .map(|record| record.d_tag.as_str())
        .collect();
    assert_eq!(keys, vec!["middle", "new"]);
}
