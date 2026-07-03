//! `/v1/keys` + `/v1/keysets` (NUT-01/NUT-02) — request builders, response
//! parsing, and the denomination→pubkey map `blinded`/`mint`/`swap` need to
//! verify a mint's signatures.

use std::collections::HashMap;

use nostr::secp256k1::PublicKey;

use super::{
    parse_json_response, MintHttpMethod, MintHttpOperation, MintHttpRequest, MintRawResponse,
};
use crate::cashu::types::KeySet;
use crate::cashu::types::KeysResponse;
use crate::error::Nip60Error;

#[must_use]
pub fn build_get_keys_request() -> MintHttpRequest {
    MintHttpRequest {
        operation: MintHttpOperation::GetKeys,
        method: MintHttpMethod::Get,
        path: "/v1/keys".to_string(),
        body: Vec::new(),
    }
}

#[must_use]
pub fn build_get_keysets_request() -> MintHttpRequest {
    MintHttpRequest {
        operation: MintHttpOperation::GetKeysets,
        method: MintHttpMethod::Get,
        path: "/v1/keysets".to_string(),
        body: Vec::new(),
    }
}

pub fn parse_keys_response(raw: &MintRawResponse) -> Result<KeysResponse, Nip60Error> {
    parse_json_response(raw, "mint keys")
}

/// Build a map from denomination to mint public key from a keyset. Fails
/// closed on a malformed hex/pubkey entry rather than silently skipping it —
/// a missing denomination would otherwise surface much later as an obscure
/// "no mint pubkey for amount" error during unblinding.
pub(crate) fn build_pubkey_map(keyset: &KeySet) -> Result<HashMap<u64, PublicKey>, Nip60Error> {
    let mut map = HashMap::new();
    for (amount_str, pubkey_hex) in &keyset.keys {
        let amount: u64 = amount_str
            .parse()
            .map_err(|_| Nip60Error::Crypto(format!("invalid amount key: {amount_str}")))?;
        let bytes =
            hex::decode(pubkey_hex).map_err(|e| Nip60Error::Crypto(format!("keyset pk: {e}")))?;
        let pk = PublicKey::from_slice(&bytes)
            .map_err(|e| Nip60Error::Crypto(format!("keyset pk parse: {e}")))?;
        map.insert(amount, pk);
    }
    Ok(map)
}
