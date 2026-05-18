//! Minimal BOLT-11 **amount** parser. We do not decode the full invoice — only
//! the human-readable amount preamble that NIP-57 zap receipts pin in the
//! `bolt11` tag.
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
//! Result in **millisats** (1 BTC = 100_000_000_000 msat). Sub-msat amounts
//! (e.g. `1p` = 0.001 msat) round down. Returns `None` on missing/empty
//! amount, unknown multiplier, or any parse failure.

const SUPPORTED_NETWORKS: &[&str] = &["lnbcrt", "lntbs", "lnbc", "lntb"];
const MSATS_PER_BTC: u128 = 100_000_000_000;

/// Decode the millisats amount from a BOLT-11 invoice's HRP.
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
}
