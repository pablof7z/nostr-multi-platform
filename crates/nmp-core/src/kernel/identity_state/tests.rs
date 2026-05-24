use super::*;

fn row(url: &str, role: &str) -> RelayEditRow {
    RelayEditRow::new(url.to_string(), role.to_string())
}

#[test]
fn account_npub_short_returns_value_verbatim_under_20_chars() {
    assert_eq!(account_npub_short(""), "");
    assert_eq!(account_npub_short("abc"), "abc");
    let twenty = "a".repeat(20);
    assert_eq!(account_npub_short(&twenty), twenty);
}

#[test]
fn account_npub_short_truncates_long_value_first10_last8_with_ellipsis() {
    let npub = "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";
    let out = account_npub_short(npub);
    assert_eq!(out, "npub1l2vyh…fqutajft");
    assert!(out.contains('…'));
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

// ── V-26 — avatar display helpers ────────────────────────────────────────

#[test]
fn account_avatar_initials_takes_first_char_of_first_two_words() {
    // Multi-word display name → first char of each of the first two words,
    // uppercased. Mirrors the previous Swift `AccountSummary.avatarInitials`.
    assert_eq!(account_avatar_initials("Alice Smith", "npub1ignored"), "AS");
    assert_eq!(
        account_avatar_initials("alice bob carol", "npub1ignored"),
        "AB",
        "extra words beyond the second are dropped"
    );
    assert_eq!(
        account_avatar_initials("alice", "npub1ignored"),
        "A",
        "single-word display name yields a single initial"
    );
}

#[test]
fn account_avatar_initials_falls_back_to_npub_body_when_display_empty() {
    // Empty display_name → first two chars of bech32 body after `npub1`,
    // uppercased. The Swift helper sliced `npub.dropFirst(5)`.
    assert_eq!(
        account_avatar_initials("", "npub1abcdefgh"),
        "AB",
        "fallback strips the `npub1` prefix and takes 2 chars"
    );
    assert_eq!(
        account_avatar_initials("   ", "npub1xyqrst"),
        "XY",
        "whitespace-only display name is treated as empty"
    );
    // No npub1 prefix → take from the raw value.
    assert_eq!(account_avatar_initials("", "raw-hex"), "RA");
    // Defensive: nothing to use → `"??"` placeholder, never a panic.
    assert_eq!(account_avatar_initials("", ""), "??");
}

// The canonical pinned djb2 vector + deterministic-hex-output + garbage-
// input coverage live in `nmp_core::display::tests` (V-33). The single
// smoke below confirms `account_avatar_color_hex` is wired through to the
// canonical helper, anchoring the Accounts-surface delegation explicitly.

#[test]
fn account_avatar_color_hex_delegates_to_canonical_helper() {
    let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    assert_eq!(account_avatar_color_hex(hex), crate::display::avatar_color_hex(hex));
}
