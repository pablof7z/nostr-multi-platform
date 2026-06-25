use super::*;

fn entry(url: &str, role: &str) -> RelayBootstrapEntry {
    RelayBootstrapEntry {
        url: url.to_string(),
        role: role.to_string(),
    }
}

#[test]
fn both_indexer_collapses_to_one_driver_recorded_as_content() {
    let plans = plan_drivers(&[entry("wss://relay.primal.net", "both,indexer")]);
    assert_eq!(plans.len(), 1, "both,indexer must be a single socket");
    assert_eq!(plans[0].url, "wss://relay.primal.net");
    assert_eq!(plans[0].primary_role, RelayRole::Content);
    assert_eq!(plans[0].roles, vec![RelayRole::Content, RelayRole::Indexer]);
}

#[test]
fn indexer_only_relay_is_one_indexer_driver() {
    let plans = plan_drivers(&[entry("wss://purplepag.es", "indexer")]);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].primary_role, RelayRole::Indexer);
    assert_eq!(plans[0].roles, vec![RelayRole::Indexer]);
}

#[test]
fn duplicate_url_distinct_roles_unions_into_one_driver() {
    let plans = plan_drivers(&[
        entry("wss://nos.lol", "content"),
        entry("wss://nos.lol", "indexer"),
    ]);
    assert_eq!(plans.len(), 1, "same URL must not open two sockets");
    assert_eq!(plans[0].primary_role, RelayRole::Content);
    assert_eq!(plans[0].roles, vec![RelayRole::Content, RelayRole::Indexer]);
}

#[test]
fn distinct_urls_get_distinct_drivers_in_first_seen_order() {
    let plans = plan_drivers(&[
        entry("wss://relay.primal.net", "both,indexer"),
        entry("wss://purplepag.es", "indexer"),
        entry("wss://nos.lol", "both,indexer"),
    ]);
    let urls: Vec<&str> = plans.iter().map(|p| p.url.as_str()).collect();
    assert_eq!(
        urls,
        vec![
            "wss://relay.primal.net",
            "wss://purplepag.es",
            "wss://nos.lol",
        ],
    );
    assert_eq!(plans.len(), 3);
}

#[test]
fn startup_admission_canonicalizes_before_dedupe() {
    let admitted = admit_startup_relays(
        vec!["WSS://Relay.Example/".to_string()],
        vec![
            entry("WSS://Relay.Example/", "content"),
            entry("wss://relay.example", "indexer"),
        ],
    )
    .expect("canonical duplicates are admitted");
    assert_eq!(admitted, vec![entry("wss://relay.example", "both")]);

    let plans = plan_drivers(&admitted);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].url, "wss://relay.example");
    assert_eq!(plans[0].roles, vec![RelayRole::Content, RelayRole::Indexer]);
}

#[test]
fn startup_admission_rejects_unknown_role() {
    let err = admit_startup_relays(
        vec!["wss://relay.example".to_string()],
        vec![entry("wss://relay.example", "totally-new-role")],
    )
    .expect_err("unknown role must fail closed");
    assert!(err.reason().contains("unknown relay role"));
}

#[test]
fn startup_admission_rejects_invalid_url() {
    let err = admit_startup_relays(vec!["https://relay.example".to_string()], vec![])
        .expect_err("non-ws relay URL must fail closed");
    assert!(err.reason().contains("ws:// or wss://"));
}

#[test]
fn startup_admission_rejects_unbounded_count() {
    let relays = (0..=MAX_STARTUP_RELAY_COUNT)
        .map(|idx| format!("wss://relay-{idx}.example"))
        .collect();
    let err = admit_startup_relays(relays, vec![]).expect_err("count cap must hold");
    assert!(err.reason().contains("relays exceeds"));
}

#[test]
fn startup_admission_rejects_oversized_url() {
    let url = format!("wss://{}.example", "a".repeat(MAX_RELAY_URL_BYTES));
    let err = admit_startup_relays(vec![url], vec![]).expect_err("URL cap must hold");
    assert!(err.reason().contains("relays contains"));
}

#[test]
fn startup_admission_bounds_relays_even_when_bootstrap_is_authoritative() {
    let url = format!("wss://{}.example", "a".repeat(MAX_RELAY_URL_BYTES));
    let err = admit_startup_relays(vec![url], vec![entry("wss://relay.example", "both")])
        .expect_err("relays URL cap must hold");
    assert!(err.reason().contains("relays contains"));
}
