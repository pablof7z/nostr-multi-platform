//! NIP-60 token event (kind:7375) — encrypted unspent proofs.
//!
//! Each token event stores a batch of unspent Cashu proofs from a single
//! mint, encrypted with NIP-44. Token events are deleted and replaced
//! atomically when proofs are consumed.

use nostr::nips::nip44;
use nostr::{EventBuilder, EventId, Keys, Kind, PublicKey, SecretKey};

use crate::cashu::types::Proof;
use crate::error::Nip60Error;
use crate::kinds::KIND_TOKEN;

/// Decrypted content of a kind:7375 token event.
#[derive(Debug, Clone)]
pub struct TokenRecord {
    /// Mint URL these proofs belong to.
    pub mint_url: String,
    /// Unspent proofs.
    pub proofs: Vec<Proof>,
    /// Token event IDs that were consumed to create these proofs (for history).
    pub del: Vec<String>,
    /// The event id of the token event (for deletion when spent).
    pub event_id: Option<EventId>,
}

impl TokenRecord {
    pub fn new(mint_url: String, proofs: Vec<Proof>) -> Self {
        Self {
            mint_url,
            proofs,
            del: Vec::new(),
            event_id: None,
        }
    }

    /// Total balance of this token event in sats.
    pub fn balance(&self) -> u64 {
        self.proofs.iter().map(|p| p.amount).sum()
    }
}

// ─── Encode ────────────────────────────────────────────────────────────────

/// Build an encrypted kind:7375 token event.
pub fn build_token_event(record: &TokenRecord, keys: &Keys) -> Result<EventBuilder, Nip60Error> {
    let content_obj = serde_json::json!({
        "mint": record.mint_url,
        "proofs": record.proofs,
        "del": record.del,
    });
    let json = serde_json::to_string(&content_obj)?;
    let content =
        nip44::encrypt(keys.secret_key(), &keys.public_key(), json, nip44::Version::V2)
            .map_err(|e| Nip60Error::Nip44(format!("{e}")))?;

    Ok(EventBuilder::new(Kind::from(KIND_TOKEN as u16), content))
}

// ─── Decode ────────────────────────────────────────────────────────────────

/// Decode a kind:7375 token event.
pub fn decode_token_event(
    event: &nostr::Event,
    secret_key: &SecretKey,
    pubkey: &PublicKey,
) -> Result<TokenRecord, Nip60Error> {
    let decrypted =
        nip44::decrypt(secret_key, pubkey, &event.content)
            .map_err(|e| Nip60Error::Nip44(format!("{e}")))?;
    let obj: serde_json::Value = serde_json::from_str(&decrypted)?;

    let mint_url = obj["mint"]
        .as_str()
        .ok_or_else(|| Nip60Error::Event("token event missing mint".into()))?
        .to_string();
    let proofs: Vec<Proof> = serde_json::from_value(obj["proofs"].clone())?;
    let del: Vec<String> = obj
        .get("del")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(TokenRecord {
        mint_url,
        proofs,
        del,
        event_id: Some(event.id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::types::Proof;
    use nostr::Keys;

    fn dummy_proof(amount: u64) -> Proof {
        Proof {
            amount,
            id: "testkeyset".into(),
            secret: hex::encode([42u8; 32]),
            c: hex::encode([0x02u8].iter().chain([0u8; 32].iter()).cloned().collect::<Vec<_>>()),
            dleq: None,
            witness: None,
        }
    }

    #[test]
    fn round_trip_token_event() {
        let keys = Keys::generate();
        let record = TokenRecord::new(
            "https://testnut.cashu.space".into(),
            vec![dummy_proof(64), dummy_proof(32)],
        );
        let builder = build_token_event(&record, &keys).expect("build");
        let event = builder.sign_with_keys(&keys).expect("sign");
        let decoded = decode_token_event(&event, keys.secret_key(), &keys.public_key())
            .expect("decode");
        assert_eq!(decoded.mint_url, record.mint_url);
        assert_eq!(decoded.balance(), 96);
        assert_eq!(decoded.proofs.len(), 2);
    }
}
