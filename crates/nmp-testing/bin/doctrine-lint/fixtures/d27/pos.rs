// D27 positive fixture — banned presentation-formatting helpers and
// precomputed label/display fields in projection code. Each marked section
// must fire a D27 finding. Canonical bech32 codec use (`to_npub`) is
// deliberately NOT in this fixture — see neg.rs, where it is proven
// compliant (#3113, ADR-0077).
//
// Placed under fixtures/ so the walker never scans it during a real nmp-core
// or nmp-nip* sweep. Opted into D27 scope via --d27-extra-scope in the smoke
// test.

// ── Part A: banned display-helper function calls ─────────────────────────────

fn build_wallet_status(pk: &str, npub: &str, now: u64, then: u64) -> WalletStatus {
    WalletStatus {
        // Each call below must fire a D27 finding.
        wallet_npub_short: short_npub(pk),          // D27: short_npub banned
        short_id: short_hex(pk),                    // D27: short_hex banned
        initials: avatar_initials(npub),            // D27: avatar_initials banned
        name_initials: display_name_initials("Alice Smith"), // D27: display_name_initials banned
        color: avatar_color_hex(pk),                // D27: avatar_color_hex banned
        ago: format_ago_secs(now, then),            // D27: format_ago_secs banned
    }
}

// ── Part B: precomputed *_label / *_display String struct fields ──────────────

/// Typed projection that carries precomputed display strings — violates D27.
#[derive(Debug)]
pub struct BadProjectionRow {
    /// Raw protocol field — fine.
    pub pubkey: String,
    /// Precomputed display label — D27 violation.
    pub signer_label: String,
    /// Precomputed display label (option) — D27 violation.
    pub status_label: Option<String>,
    /// Precomputed display field — D27 violation.
    pub wallet_npub_display: String,
}
