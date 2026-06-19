//! LNURL-pay metadata parsing used by the fetcher.

use nostr::PublicKey;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnurlInvoice {
    pub bolt11: String,
    pub provider_pubkey: String,
}

pub(super) fn nostr_provider_pubkey(metadata: &serde_json::Value) -> Result<String, String> {
    let allows_nostr = metadata
        .get("allowsNostr")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !allows_nostr {
        return Err(
            "receiver's LNURL-pay endpoint does not advertise NIP-57 support \
             (`allowsNostr` is false or missing)"
                .to_string(),
        );
    }
    let raw = metadata
        .get("nostrPubkey")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "receiver's LNURL-pay endpoint advertises NIP-57 but is missing `nostrPubkey`"
                .to_string()
        })?
        .trim();
    if raw.is_empty() {
        return Err(
            "receiver's LNURL-pay endpoint advertises NIP-57 but has an empty `nostrPubkey`"
                .to_string(),
        );
    }
    PublicKey::from_hex(raw)
        .map(|pk| pk.to_hex())
        .map_err(|e| format!("receiver's LNURL-pay endpoint returned invalid `nostrPubkey`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_canonical_nostr_pubkey() {
        let pubkey = "a".repeat(64);
        let metadata = serde_json::json!({
            "allowsNostr": true,
            "nostrPubkey": pubkey,
        });
        assert_eq!(nostr_provider_pubkey(&metadata).unwrap(), "a".repeat(64));
    }

    #[test]
    fn rejects_missing_allows_nostr() {
        let metadata = serde_json::json!({ "nostrPubkey": "a".repeat(64) });
        assert!(nostr_provider_pubkey(&metadata).is_err());
    }

    #[test]
    fn rejects_missing_provider_pubkey() {
        let metadata = serde_json::json!({ "allowsNostr": true });
        assert!(nostr_provider_pubkey(&metadata).is_err());
    }

    #[test]
    fn rejects_invalid_provider_pubkey() {
        let metadata = serde_json::json!({
            "allowsNostr": true,
            "nostrPubkey": "not-a-pubkey",
        });
        assert!(nostr_provider_pubkey(&metadata).is_err());
    }
}
