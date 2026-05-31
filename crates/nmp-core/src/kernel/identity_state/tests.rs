use super::*;

fn row(url: &str, role: &str) -> AppRelay {
    AppRelay::new(url.to_string(), role.to_string())
}

#[test]
fn read_eligible_relay_urls_accepts_read_and_both() {
    let rows = vec![
        row("wss://read.example", "read"),
        row("wss://both.example", "both"),
        row("wss://write.example", "write"),
        row("wss://index.example", "indexer"),
    ];
    assert_eq!(
        read_eligible_relay_urls(&rows),
        vec!["wss://read.example", "wss://both.example"]
    );
}

#[test]
fn read_eligible_relay_urls_uses_canonical_role_tokens() {
    let rows = vec![
        row("wss://composite.example", "write + indexer + read"),
        row("wss://upper.example", "BOTH,INDEXER"),
        row("wss://not-read.example", "writer"),
    ];
    assert_eq!(
        read_eligible_relay_urls(&rows),
        vec!["wss://composite.example", "wss://upper.example"]
    );
}
