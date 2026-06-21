// D27 negative fixture — compliant code that must NOT fire any D27 findings.
//
// Exercises the allowlist paths: projection code that emits raw protocol data
// without calling banned display helpers or storing precomputed display strings.

/// Clean projection row — only raw fields.
#[derive(Debug)]
pub struct CleanProjectionRow {
    /// Raw hex pubkey — fine.
    pub pubkey: String,
    /// Raw unix timestamp — fine.
    pub created_at: u64,
    /// Semantic tone token (NOT a display string) — fine, D27 never fires on `_tone`.
    pub status_tone: String,
    /// Boolean raw flag — fine.
    pub is_verified: bool,
}

fn build_clean_status(pk: &str, ts: u64) -> CleanProjectionRow {
    CleanProjectionRow {
        pubkey: pk.to_string(),
        created_at: ts,
        status_tone: "warning".to_string(),
        is_verified: false,
    }
}

/// Field names that end with `_label` or `_display` but carry non-String types
/// must NOT fire D27.
struct MixedTypes {
    pub status_label: u8,         // u8, not String — should not fire
    pub wallet_npub_display: bool, // bool, not String — should not fire
}

/// `let` bindings with `_label:` type annotation must NOT fire D27.
fn parse_status(raw: &str) -> String {
    let status_label: String = raw.to_ascii_lowercase();
    status_label
}

/// A struct field value that happens to call a method named like a banned
/// helper on a non-display object must NOT fire.  The rule checks function
/// names as tokens — these are different symbols.
fn other_fn(x: &str) -> &str {
    // "short_hex" as part of a comment about short_hex is fine.
    x
}

// Inline doc mentioning short_npub or to_npub in comments is fine.
// format_ago_secs is documented here too.
