//! NIP-60 wallet event (kind:17375) — encrypted wallet configuration.
//!
//! The wallet event stores the wallet's Cashu private key and the list of
//! mint URLs. It is encrypted with NIP-44 keyed to the owner's pubkey (so
//! only the owner can read it).
//!
//! # Relay policy (canonical statement — link here, don't restate)
//!
//! A kind:17375 wallet event *may* carry legacy `relay` tags, inherited from
//! wallets written before kind:10019 adoption. Those tags are **never**
//! authoritative relay selection: the active user's kind:10019 `relay` tags,
//! with NIP-65 fallback, are the sole source of truth for wallet relay
//! scoping (see `docs/architecture/nip60-nip61-wallet-design.md`, Relay
//! Acquisition). That resolution policy is owned by `nmp-wallet`, not this
//! crate.
//!
//! This module reflects that split in its API:
//!
//! - [`decode_wallet_event`] surfaces any `relay` tags on a *decoded* event as
//!   [`WalletConfig::legacy_relay_hint`] — a read-only, non-authoritative
//!   compatibility signal for wallets that predate kind:10019.
//! - [`build_wallet_event`] never writes `relay` tags on a *newly built*
//!   event. New kind:17375s carry no relay tags at all, so the legacy hint
//!   ages out as a decode-only compatibility surface rather than being
//!   perpetuated by every wallet this crate creates.
//! - No code path in this crate feeds `legacy_relay_hint` back into a
//!   kind:10019 publish. Callers that need to publish kind:10019 (e.g.
//!   [`crate::nip60_wallet::Nip60WalletHandle::publish_nutzap_info`]) must
//!   supply their own resolved, authoritative relay set.

use nostr::nips::nip44;
use nostr::{EventBuilder, EventId, Keys, Kind, PublicKey, SecretKey, TagKind};

use crate::error::Nip60Error;
use crate::kinds::KIND_NIP60_WALLET;

// ─── Wire content ──────────────────────────────────────────────────────────

/// Decrypted content of a kind:17375 wallet event.
#[derive(Debug, Clone)]
pub struct WalletConfig {
    /// The wallet's Cashu private key (hex). Used for P2PK receiving (NIP-61).
    pub privkey_hex: String,
    /// Mint URLs this wallet uses (in preference order).
    pub mints: Vec<String>,
    /// Human-readable wallet name (optional).
    pub name: Option<String>,
    /// Relay URLs extracted from a *decoded* event's `relay` tags — a
    /// non-authoritative compatibility hint only (see module docs). Always
    /// empty on a freshly [`generate`](Self::generate)d config, since
    /// [`build_wallet_event`] never writes these tags.
    pub legacy_relay_hint: Vec<String>,
    /// The event id of the wallet event that was decoded (for deletion/updates).
    pub event_id: Option<EventId>,
}

impl WalletConfig {
    /// Create a new wallet config with a freshly generated Cashu private key.
    ///
    /// The resulting config carries no relay hint — new kind:17375 events
    /// never advertise `relay` tags (see module docs).
    pub fn generate(mint_urls: Vec<String>) -> Self {
        let privkey =
            nostr::secp256k1::SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        Self {
            privkey_hex: hex::encode(privkey.secret_bytes()),
            mints: mint_urls,
            name: None,
            legacy_relay_hint: Vec::new(),
            event_id: None,
        }
    }

    /// Return the Cashu pubkey (compressed hex) corresponding to the wallet privkey.
    pub fn pubkey_hex(&self) -> Result<String, Nip60Error> {
        let bytes = hex::decode(&self.privkey_hex)
            .map_err(|e| Nip60Error::Crypto(format!("wallet privkey hex: {e}")))?;
        let sk = nostr::secp256k1::SecretKey::from_slice(&bytes)
            .map_err(|e| Nip60Error::Crypto(format!("wallet privkey parse: {e}")))?;
        let secp = nostr::secp256k1::Secp256k1::new();
        let pk = nostr::secp256k1::PublicKey::from_secret_key(&secp, &sk);
        Ok(hex::encode(pk.serialize()))
    }
}

// ─── Encode ────────────────────────────────────────────────────────────────

/// Build an encrypted kind:17375 wallet event from a [`WalletConfig`].
///
/// Never emits `relay` tags — see module docs. `config.legacy_relay_hint` is
/// a decode-side-only field; it is intentionally ignored here so a wallet
/// re-published from a decoded config doesn't resurrect the legacy tags.
pub fn build_wallet_event(
    config: &WalletConfig,
    keys: &Keys,
) -> Result<EventBuilder, Nip60Error> {
    // Content: NIP-44 encrypted JSON array of [key, value] pairs.
    let mut pairs: Vec<Vec<String>> = vec![
        vec!["privkey".into(), config.privkey_hex.clone()],
    ];
    for mint in &config.mints {
        pairs.push(vec!["mint".into(), mint.clone()]);
    }
    if let Some(ref name) = config.name {
        pairs.push(vec!["name".into(), name.clone()]);
    }
    let json = serde_json::to_string(&pairs)?;
    let content =
        nip44::encrypt(keys.secret_key(), &keys.public_key(), json, nip44::Version::V2)
            .map_err(|e| Nip60Error::Nip44(format!("{e}")))?;

    Ok(EventBuilder::new(Kind::from(KIND_NIP60_WALLET as u16), content))
}

// ─── Decode ────────────────────────────────────────────────────────────────

/// Decode a kind:17375 wallet event into a [`WalletConfig`].
pub fn decode_wallet_event(
    event: &nostr::Event,
    secret_key: &SecretKey,
    pubkey: &PublicKey,
) -> Result<WalletConfig, Nip60Error> {
    let decrypted =
        nip44::decrypt(secret_key, pubkey, &event.content)
            .map_err(|e| Nip60Error::Nip44(format!("{e}")))?;
    let pairs: Vec<Vec<String>> = serde_json::from_str(&decrypted)?;

    let mut privkey_hex = None;
    let mut mints = Vec::new();
    let mut name = None;

    for pair in pairs {
        if pair.len() < 2 {
            continue;
        }
        match pair[0].as_str() {
            "privkey" => privkey_hex = Some(pair[1].clone()),
            "mint" => mints.push(pair[1].clone()),
            "name" => name = Some(pair[1].clone()),
            _ => {}
        }
    }

    let privkey_hex = privkey_hex.ok_or_else(|| {
        Nip60Error::Event("wallet event missing privkey".into())
    })?;
    if mints.is_empty() {
        return Err(Nip60Error::Event("wallet event has no mints".into()));
    }

    // Extract the legacy relay compatibility hint (non-authoritative — see
    // module docs). Foreign or pre-this-fix wallet events may still carry
    // `relay` tags; this crate reads them back only as a hint, never truth.
    let legacy_relay_hint: Vec<String> = event
        .tags
        .iter()
        .filter(|t| t.kind() == TagKind::custom("relay"))
        .filter_map(|t| t.content().map(str::to_owned))
        .collect();

    Ok(WalletConfig {
        privkey_hex,
        mints,
        name,
        legacy_relay_hint,
        event_id: Some(event.id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Keys, Tag, TagKind};

    #[test]
    fn round_trip_wallet_event() {
        let keys = Keys::generate();
        let config = WalletConfig::generate(vec!["https://testnut.cashu.space".into()]);
        let builder = build_wallet_event(&config, &keys).expect("build");
        let event = builder
            .sign_with_keys(&keys)
            .expect("sign");
        let decoded = decode_wallet_event(&event, keys.secret_key(), &keys.public_key())
            .expect("decode");
        assert_eq!(decoded.mints, config.mints);
        assert_eq!(decoded.privkey_hex, config.privkey_hex);
    }

    /// A freshly built kind:17375 must never carry `relay` tags, even if the
    /// config it was built from somehow carries a non-empty
    /// `legacy_relay_hint` (e.g. cloned from a decoded config).
    #[test]
    fn build_wallet_event_never_emits_relay_tags() {
        let keys = Keys::generate();
        let mut config = WalletConfig::generate(vec!["https://testnut.cashu.space".into()]);
        config.legacy_relay_hint = vec!["wss://stale-relay.example".into()];

        let builder = build_wallet_event(&config, &keys).expect("build");
        let event = builder.sign_with_keys(&keys).expect("sign");

        assert!(
            !event.tags.iter().any(|t| t.kind() == TagKind::custom("relay")),
            "build_wallet_event must never write legacy relay tags"
        );
    }

    /// Decoding a foreign/legacy kind:17375 that *does* carry `relay` tags
    /// must still surface them as the non-authoritative hint (decode-side
    /// compat is intentionally preserved).
    #[test]
    fn decode_wallet_event_surfaces_legacy_relay_tags_as_hint() {
        let keys = Keys::generate();
        let config = WalletConfig::generate(vec!["https://testnut.cashu.space".into()]);
        let builder = build_wallet_event(&config, &keys)
            .expect("build")
            .tag(Tag::custom(TagKind::custom("relay"), ["wss://legacy-relay.example"]));
        let event = builder.sign_with_keys(&keys).expect("sign");

        let decoded = decode_wallet_event(&event, keys.secret_key(), &keys.public_key())
            .expect("decode");
        assert_eq!(decoded.legacy_relay_hint, vec!["wss://legacy-relay.example".to_string()]);
    }
}
