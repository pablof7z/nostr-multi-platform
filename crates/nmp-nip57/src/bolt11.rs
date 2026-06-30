//! Minimal BOLT-11 parser. We decode only the two fields NIP-57 needs:
//!
//! 1. the human-readable **amount** preamble ([`amount_msats`]) that zap
//!    receipts pin in the `bolt11` tag, and
//! 2. the **description hash** (`h` / tagged field type 23) data field
//!    ([`description_hash`]) that NIP-57 requires equal `SHA-256(zap request)`
//!    before a wallet auto-pays the invoice.
//!
//! We do **not** decode the full invoice (no signature recovery, no expiry,
//! no routing hints) — only what the zap-pay safety check consumes.
//!
//! BOLT-11 HRP shape: `ln<network><amount?><multiplier?>` followed by `1` and
//! the bech32 data part. Because bech32 forbids `1` in the data, the **last**
//! `1` in the lowercase invoice is unambiguously the HRP/data separator —
//! everything before it (after the network) is the amount HRP.
//!
//! Multipliers (per BOLT-11):
//! - `m` → 10⁻³ BTC
//! - `u` → 10⁻⁶ BTC
//! - `n` → 10⁻⁹ BTC
//! - `p` → 10⁻¹² BTC
//!
//! Result in **millisats** (1 BTC = `100_000_000_000` msat). Sub-msat amounts
//! (e.g. `1p` = 0.001 msat) round down. Returns `None` on missing/empty
//! amount, unknown multiplier, or any parse failure.

use bech32::primitives::decode::UncheckedHrpstring;
use bech32::Fe32;

const SUPPORTED_NETWORKS: &[&str] = &["lnbcrt", "lntbs", "lnbc", "lntb"];
const MSATS_PER_BTC: u128 = 100_000_000_000;

/// BOLT-11 tagged-field type for the description hash (`h`). Per BOLT-11 the
/// 5-bit field type is `23`.
const TAG_DESCRIPTION_HASH: u8 = 23;

/// Number of 5-bit groups in the timestamp prefix of the BOLT-11 data part
/// (35 bits / 5).
const TIMESTAMP_FE32_GROUPS: usize = 7;

/// Decode the millisats amount from a BOLT-11 invoice's HRP.
#[must_use]
pub fn amount_msats(invoice: &str) -> Option<u64> {
    let lower = invoice.trim().to_ascii_lowercase();
    let body = strip_network(&lower)?;

    // Bech32 forbids '1' in the data part, so the last '1' is the
    // unambiguous HRP/data separator.
    let sep = body.rfind('1')?;
    let hrp_amount = &body[..sep];
    if hrp_amount.is_empty() {
        return None;
    }

    let bytes = hrp_amount.as_bytes();
    let last = *bytes.last()?;
    let (digit_end, multiplier) = match last {
        b'm' | b'u' | b'n' | b'p' => (bytes.len() - 1, Some(last as char)),
        _ => (bytes.len(), None),
    };
    if digit_end == 0 {
        return None;
    }
    let digits = &hrp_amount[..digit_end];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let amount: u128 = digits.parse().ok()?;

    let msats: u128 = match multiplier {
        None => amount.checked_mul(MSATS_PER_BTC)?,
        Some('m') => amount.checked_mul(MSATS_PER_BTC)? / 1_000,
        Some('u') => amount.checked_mul(MSATS_PER_BTC)? / 1_000_000,
        Some('n') => amount.checked_mul(MSATS_PER_BTC)? / 1_000_000_000,
        Some('p') => amount.checked_mul(MSATS_PER_BTC)? / 1_000_000_000_000,
        Some(_) => return None,
    };

    u64::try_from(msats).ok()
}

/// Extract the 32-byte description hash (`h` / tagged field type 23) from a
/// BOLT-11 invoice, or `None` when the invoice carries no `h` field, is
/// malformed, or is not a parseable bech32 string.
///
/// NIP-57 requires the zap invoice to commit to the zap request: the `h` field
/// must equal `SHA-256(zap_request_json)`. This function returns the raw
/// 256-bit commitment so the caller can compare it; it does **no** hashing and
/// **no** crypto itself — it only walks the BOLT-11 tagged-field layout.
///
/// Decoding strategy (no full-invoice library): BOLT-11 packs its data part as
/// a stream of 5-bit groups (bech32 "field elements"). We read the raw groups
/// via [`UncheckedHrpstring`] — which gives the data part *without* the 8-bit
/// regrouping that [`bech32::decode`] performs and that would scramble the
/// tagged-field boundaries — then:
///
/// 1. skip the [`TIMESTAMP_FE32_GROUPS`]-group timestamp prefix,
/// 2. walk tagged fields (`type` 1 group, `length` 2 groups big-endian, then
///    `length` data groups),
/// 3. on the [`TAG_DESCRIPTION_HASH`] field, repack its 5-bit groups into the
///    256-bit hash (the field is 52 groups = 260 bits; the low 4 bits are
///    zero-padding and are dropped).
///
/// The final 6 groups of the data part are the bech32 checksum; the walk stops
/// before consuming them because a well-formed field never spans into the
/// checksum, and any length that would overrun the data yields `None`.
#[must_use]
pub fn description_hash(invoice: &str) -> Option<[u8; 32]> {
    let trimmed = invoice.trim();
    // DoS guard: real BOLT-11 invoices are well under 2 KiB; a relay supplying
    // a multi-megabyte string would force an O(n) allocation before we discover
    // it is invalid. Reject anything over 8 KiB early.
    if trimmed.len() > 8192 {
        return None;
    }
    let unchecked = UncheckedHrpstring::new(trimmed).ok()?;
    // Raw 5-bit data-part groups (HRP + separator already stripped). Includes
    // the trailing 6-group checksum, which the field walk never reaches.
    let fes: Vec<Fe32> = unchecked
        .data_part_ascii()
        .iter()
        .map(|&b| Fe32::from_char_unchecked(b))
        .collect();

    // The last 6 groups are the bech32 checksum — never part of a field body.
    let body_len = fes.len().checked_sub(BECH32_CHECKSUM_GROUPS)?;
    if body_len < TIMESTAMP_FE32_GROUPS {
        return None;
    }
    let mut idx = TIMESTAMP_FE32_GROUPS;

    while idx < body_len {
        // Each tagged field is `type` (1) + `length` (2, big-endian 5-bit).
        let field_type = fes.get(idx)?.to_u8();
        let len_hi = fes.get(idx + 1)?.to_u8() as usize;
        let len_lo = fes.get(idx + 2)?.to_u8() as usize;
        let data_len = (len_hi << 5) | len_lo;
        let data_start = idx + 3;
        let data_end = data_start.checked_add(data_len)?;
        if data_end > body_len {
            // A field claiming more data than remains is a malformed invoice.
            return None;
        }
        if field_type == TAG_DESCRIPTION_HASH {
            return fe32_groups_to_hash(&fes[data_start..data_end]);
        }
        idx = data_end;
    }
    None
}

/// Number of 5-bit groups occupied by the bech32 checksum at the end of the
/// data part.
const BECH32_CHECKSUM_GROUPS: usize = 6;

/// Repack a run of 5-bit groups into a 32-byte hash, MSB-first. The description
/// hash is 256 bits encoded as 52 groups (260 bits); the trailing 4 zero-pad
/// bits are discarded. Returns `None` if `groups` is not exactly 52 long.
fn fe32_groups_to_hash(groups: &[Fe32]) -> Option<[u8; 32]> {
    // 256 bits / 5 bits-per-group, rounded up = 52 groups.
    const EXPECTED_GROUPS: usize = 52;
    if groups.len() != EXPECTED_GROUPS {
        return None;
    }
    let mut out = [0u8; 32];
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut byte_idx = 0;
    for fe in groups {
        acc = (acc << 5) | u32::from(fe.to_u8());
        bits += 5;
        while bits >= 8 && byte_idx < 32 {
            bits -= 8;
            out[byte_idx] = ((acc >> bits) & 0xff) as u8;
            byte_idx += 1;
        }
    }
    if byte_idx != 32 {
        return None;
    }
    Some(out)
}

fn strip_network(invoice: &str) -> Option<&str> {
    // Longest prefix first ("lnbcrt" before "lnbc") so a regtest invoice
    // isn't mis-stripped to "rt..." by the mainnet prefix.
    for prefix in SUPPORTED_NETWORKS {
        if let Some(rest) = invoice.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lnbc2500u_is_2500_micro_btc_in_msats() {
        // 2500 * 10⁻⁶ BTC = 0.0025 BTC = 250_000 sats = 250_000_000 msat
        assert_eq!(amount_msats("lnbc2500u1pvjluez000"), Some(250_000_000));
    }

    #[test]
    fn lnbc1m_is_one_milli_btc() {
        // 1 mBTC = 0.001 BTC = 100_000 sats = 100_000_000 msat
        assert_eq!(amount_msats("lnbc1m1pvjluez000"), Some(100_000_000));
    }

    #[test]
    fn lnbc20n_is_twenty_nano_btc() {
        // 20 * 10⁻⁹ BTC = 2000 msat
        assert_eq!(amount_msats("lnbc20n1pvjluez000"), Some(2_000));
    }

    #[test]
    fn lnbc1500n_typical_zap_amount() {
        // 1500 nBTC = 0.0000015 BTC = 150 sats = 150_000 msat
        assert_eq!(amount_msats("lnbc1500n1pvjluez000"), Some(150_000));
    }

    #[test]
    fn pico_btc_below_msat_rounds_down() {
        // 1 pBTC = 0.000001 msat → rounds to 0.
        assert_eq!(amount_msats("lnbc1p1pvjluez000"), Some(0));
    }

    #[test]
    fn testnet_prefix_lntb_is_supported() {
        assert_eq!(amount_msats("lntb500u1pvjluez000"), Some(50_000_000));
    }

    #[test]
    fn regtest_prefix_lnbcrt_is_supported() {
        assert_eq!(amount_msats("lnbcrt500u1pvjluez000"), Some(50_000_000));
    }

    #[test]
    fn no_amount_returns_none() {
        // `lnbc1<data>` with no digits between network and separator → no amount.
        assert_eq!(amount_msats("lnbc1pvjluez000"), None);
    }

    #[test]
    fn missing_prefix_returns_none() {
        assert_eq!(amount_msats("garbage"), None);
        assert_eq!(amount_msats("100u1pvjluez000"), None);
    }

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(amount_msats(""), None);
        assert_eq!(amount_msats("   "), None);
    }

    #[test]
    fn uppercase_invoice_is_normalised() {
        assert_eq!(amount_msats("LNBC1M1PVJLUEZ000"), Some(100_000_000));
    }

    #[test]
    fn malformed_amount_with_non_digit_chars_returns_none() {
        assert_eq!(amount_msats("lnbc5x0u1pvjluez000"), None);
    }

    #[test]
    fn unknown_multiplier_returns_none() {
        // 'x' is not a valid BOLT-11 multiplier.
        assert_eq!(amount_msats("lnbc500x1pvjluez000"), None);
    }

    #[test]
    fn bare_amount_without_multiplier_is_whole_btc() {
        // `lnbc2<data>` — no multiplier suffix means whole BTC.
        // 2 BTC = 200_000_000_000 msat.
        assert_eq!(amount_msats("lnbc21pvjluez000"), Some(200_000_000_000));
    }

    #[test]
    fn overflow_amount_returns_none_not_panic() {
        // A digit string large enough that `amount * MSATS_PER_BTC` overflows
        // u128 must yield `None` via `checked_mul`, never panic.
        let huge = "9".repeat(40);
        let invoice = format!("lnbc{huge}1pvjluez000");
        assert_eq!(amount_msats(&invoice), None);
    }

    #[test]
    fn amount_exceeding_u64_but_within_u128_returns_none() {
        // 1_000_000_000 BTC in msats overflows u64 (the return type) but not
        // the u128 intermediate — the `u64::try_from` guard must catch it.
        assert_eq!(amount_msats("lnbc10000000001pvjluez000"), None);
    }

    #[test]
    fn separator_at_position_zero_returns_none() {
        // `lnbc1...` — the only `1` is immediately after the network prefix,
        // leaving an empty HRP amount slice.
        assert_eq!(amount_msats("lnbc1abcdef"), None);
    }

    #[test]
    fn no_separator_at_all_returns_none() {
        // No `1` anywhere after the network prefix → no HRP/data boundary.
        assert_eq!(amount_msats("lnbc500u"), None);
    }

    #[test]
    fn multiplier_with_no_digits_returns_none() {
        // `lnbcm1...` — a lone multiplier with no preceding digits.
        assert_eq!(amount_msats("lnbcm1pvjluez000"), None);
    }

    // ---- description_hash (`h` / tag 23) extraction ------------------------

    use bech32::{Fe32, Hrp};

    /// Pack a 32-byte hash into the 52 five-bit groups BOLT-11 uses for the
    /// `h` field (256 bits MSB-first, low 4 bits zero-padded). Inverse of the
    /// production [`fe32_groups_to_hash`].
    fn hash_to_fe32_groups(hash: &[u8; 32]) -> Vec<Fe32> {
        let mut groups = Vec::with_capacity(52);
        let mut acc: u32 = 0;
        let mut bits: u32 = 0;
        for &b in hash {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                let v = ((acc >> bits) & 0x1f) as u8;
                groups.push(Fe32::try_from(v).unwrap());
            }
        }
        // 256 bits → 51 full groups + 1 bits remaining; pad the last group.
        if bits > 0 {
            let v = ((acc << (5 - bits)) & 0x1f) as u8;
            groups.push(Fe32::try_from(v).unwrap());
        }
        assert_eq!(groups.len(), 52, "256-bit hash packs into 52 fe32 groups");
        groups
    }

    /// Build a synthetic `lnbc…` invoice whose only tagged field is an `h`
    /// (description hash) field carrying `hash`. Timestamp groups are arbitrary;
    /// the bech32 checksum is computed so the string is a valid bech32 word.
    fn synth_invoice_with_h(hash: &[u8; 32]) -> String {
        use bech32::primitives::iter::Fe32IterExt;
        let zero = Fe32::try_from(0u8).unwrap();
        let mut data: Vec<Fe32> = Vec::new();
        // 7-group timestamp prefix (value irrelevant to the H walk).
        data.extend(std::iter::repeat_n(zero, TIMESTAMP_FE32_GROUPS));
        // Tagged field: type 23, length 52 (big-endian 5-bit: 1,20 → 1<<5|20).
        data.push(Fe32::try_from(TAG_DESCRIPTION_HASH).unwrap());
        let len = 52usize;
        data.push(Fe32::try_from(((len >> 5) & 0x1f) as u8).unwrap());
        data.push(Fe32::try_from((len & 0x1f) as u8).unwrap());
        data.extend(hash_to_fe32_groups(hash));

        let hrp = Hrp::parse("lnbc").unwrap();
        data.into_iter()
            .with_checksum::<bech32::Bech32>(&hrp)
            .chars()
            .collect()
    }

    #[test]
    fn description_hash_round_trips_through_synthetic_invoice() {
        let hash: [u8; 32] = std::array::from_fn(|i| i as u8);
        let invoice = synth_invoice_with_h(&hash);
        assert_eq!(
            description_hash(&invoice),
            Some(hash),
            "the H-field hash must extract byte-for-byte"
        );
    }

    #[test]
    fn description_hash_handles_uppercase_and_whitespace() {
        let hash: [u8; 32] = std::array::from_fn(|i| (255 - i) as u8);
        let invoice = synth_invoice_with_h(&hash);
        let messy = format!("  {}  ", invoice.to_ascii_uppercase());
        assert_eq!(description_hash(&messy), Some(hash));
    }

    #[test]
    fn description_hash_absent_when_no_h_field() {
        // A real amount-only fixture from the amount tests has no `h` field.
        assert_eq!(description_hash("lnbc2500u1pvjluez000"), None);
    }

    #[test]
    fn description_hash_none_on_garbage() {
        assert_eq!(description_hash("not a bolt11"), None);
        assert_eq!(description_hash(""), None);
    }

    // ---- DoS guard: oversized invoice must be rejected fast ------------------

    #[test]
    fn description_hash_rejects_invoice_over_8kib() {
        // Real BOLT-11 invoices are always under 2 KiB.  A relay supplying a
        // 8 KiB+ blob must be rejected before any allocation, not processed.
        let huge = "lnbc".to_string() + &"a".repeat(8192);
        assert_eq!(
            description_hash(&huge),
            None,
            "an 8 KiB+ invoice must be rejected before allocation"
        );
    }

    #[test]
    fn description_hash_still_works_at_exactly_8192_bytes() {
        // 8192 bytes is the boundary; the guard is `> 8192`, so exactly 8192
        // bytes must still be attempted (and will return None because it's
        // garbage, but it must not be short-circuited by the guard).
        // We just verify it doesn't panic and produces None.
        let at_limit = "x".repeat(8192);
        // This is garbage (no valid bech32 HRP) so None is expected,
        // but it must reach the bech32 parser rather than the early guard.
        // (The result is None either way; the test just ensures no panic.)
        let _ = description_hash(&at_limit);
    }

    #[test]
    fn description_hash_skips_preceding_fields_to_reach_h() {
        // Build an invoice with a junk tagged field BEFORE the H field, to
        // prove the field walk advances by length rather than assuming H is
        // first.
        use bech32::primitives::iter::Fe32IterExt;
        let zero = Fe32::try_from(0u8).unwrap();
        let hash: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(7));
        let mut data: Vec<Fe32> = Vec::new();
        data.extend(std::iter::repeat_n(zero, TIMESTAMP_FE32_GROUPS));
        // Junk field: type 1, length 3, three zero data groups.
        data.push(Fe32::try_from(1u8).unwrap());
        data.push(zero);
        data.push(Fe32::try_from(3u8).unwrap());
        data.extend(std::iter::repeat_n(zero, 3));
        // Then the real H field.
        data.push(Fe32::try_from(TAG_DESCRIPTION_HASH).unwrap());
        data.push(Fe32::try_from(1u8).unwrap());
        data.push(Fe32::try_from(20u8).unwrap()); // 1<<5 | 20 = 52
        data.extend(hash_to_fe32_groups(&hash));

        let hrp = Hrp::parse("lnbc").unwrap();
        let invoice: String = data
            .into_iter()
            .with_checksum::<bech32::Bech32>(&hrp)
            .chars()
            .collect();
        assert_eq!(description_hash(&invoice), Some(hash));
    }
}
