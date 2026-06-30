//! NIP-57 fail-closed guards before the wallet pays:
//! - `validate_bolt11_amount` — a malicious or buggy LNURL provider can return
//!   a bolt11 whose encoded amount differs from the user-requested amount. It
//!   MUST reject any mismatch and reject amountless invoices (an unverifiable
//!   invoice must never be auto-paid).
//! - `inject_lnurl_tag` — both error paths return `Err` so the caller aborts the
//!   zap rather than proceed without the tag (D6 fix).

use super::*;

/// Build a minimal fake bolt11 invoice string whose HRP encodes `msats`
/// millisatoshis.  The data part ("1pvjluez000") is syntactically sufficient
/// for the BOLT-11 HRP parser (`crate::bolt11::amount_msats`) — we do not need
/// a cryptographically valid invoice for these unit tests.
fn fake_bolt11_for_msats(msats: u64) -> String {
    // Convert msats to the most compact BOLT-11 HRP representation.
    // Use the `n` (nano-BTC) multiplier: 1 nBTC = 100 msat, so any multiple
    // of 100 is exactly representable.  All zap amounts used in the tests are
    // multiples of 100 msat.
    const MSATS_PER_NANOBTC: u64 = 100;
    let n = msats / MSATS_PER_NANOBTC;
    format!("lnbc{n}n1pvjluez000")
}

/// A bolt11 with NO amount in the HRP (amountless, per BOLT-11 optional-amount
/// spec).  The `crate::bolt11::amount_msats` parser returns `None` for this shape.
const AMOUNTLESS_BOLT11: &str = "lnbc1pvjluez000";

#[test]
fn validate_bolt11_amount_accepts_exact_match() {
    // 21_000 msat = 210 nBTC.
    let bolt11 = fake_bolt11_for_msats(21_000);
    assert_eq!(
        validate_bolt11_amount(&bolt11, 21_000),
        Ok(()),
        "an invoice whose decoded amount exactly equals the requested amount must succeed"
    );
}

#[test]
fn validate_bolt11_amount_rejects_higher_amount() {
    // Provider encodes 42_000 msat but user requested 21_000 — would silently
    // double-charge the user.
    let bolt11 = fake_bolt11_for_msats(42_000);
    let result = validate_bolt11_amount(&bolt11, 21_000);
    assert!(
        result.is_err(),
        "invoice encoding MORE than requested must be rejected: {result:?}"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("21000") && msg.contains("42000"),
        "error must name both the requested and actual amounts: {msg}"
    );
}

#[test]
fn validate_bolt11_amount_rejects_lower_amount() {
    // Provider encodes 1_000 msat but user requested 21_000 — still wrong.
    let bolt11 = fake_bolt11_for_msats(1_000);
    let result = validate_bolt11_amount(&bolt11, 21_000);
    assert!(
        result.is_err(),
        "invoice encoding LESS than requested must be rejected: {result:?}"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("21000") && msg.contains("1000"),
        "error must name both amounts: {msg}"
    );
}

#[test]
fn validate_bolt11_amount_rejects_amountless_invoice() {
    // Fail closed — an invoice with no parseable amount must NEVER be auto-paid.
    // The user chose an explicit amount; an amountless invoice gives no proof
    // the provider will charge only that amount.
    let result = validate_bolt11_amount(AMOUNTLESS_BOLT11, 21_000);
    assert!(
        result.is_err(),
        "an amountless invoice must be rejected (fail closed): {result:?}"
    );
}

#[test]
fn validate_bolt11_amount_rejects_malformed_amount_hrp() {
    // An invoice that passes `looks_like_bolt11` (correct prefix) but has a
    // malformed amount HRP (non-digit chars) still fails validation.
    let result = validate_bolt11_amount("lnbc5x0u1pvjluez000", 21_000);
    assert!(
        result.is_err(),
        "an invoice with a malformed amount must be rejected: {result:?}"
    );
}

/// Valid lightning address → injects a bech32 lnurl1… tag.
#[test]
fn inject_lnurl_tag_inserts_tag_for_valid_lightning_address() {
    let mut u = unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    assert!(inject_lnurl_tag("alice@pay.example.com", &mut u).is_ok());
    let row = u
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("lnurl"))
        .expect("lnurl tag must be injected");
    assert!(
        row.len() > 1 && row[1].starts_with("lnurl1"),
        "tag must be bech32: {row:?}"
    );
}

/// Unparseable input → Err (caller aborts the zap), no tag added.
#[test]
fn inject_lnurl_tag_returns_err_for_unparseable_input() {
    let mut u = unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    assert!(
        inject_lnurl_tag("not-a-valid-lnurl-at-all", &mut u).is_err(),
        "unparseable input must return Err"
    );
    assert!(!u
        .tags
        .iter()
        .any(|t| t.first().map(String::as_str) == Some("lnurl")));
}

/// Existing non-empty lnurl tag → no-op (Ok, tag unchanged, no duplicate).
#[test]
fn inject_lnurl_tag_skips_when_tag_already_present() {
    let existing = vec![
        "lnurl".to_string(),
        "lnurl1dp68gurn8ghj7arg9ekxzar9wd6xzarfwfjhgwf3h".to_string(),
    ];
    let mut u = unsigned_for(vec![
        existing.clone(),
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    assert!(inject_lnurl_tag("alice@pay.example.com", &mut u).is_ok());
    let rows: Vec<_> = u
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("lnurl"))
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(*rows[0], existing);
}
