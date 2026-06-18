//! NIP-61 NutZap — send and receive Cashu ecash via Nostr.
//!
//! # Send flow
//!
//! 1. Look up recipient's kind:10019 (NutZap info) to find their accepted mints
//!    and P2PK receiving pubkey.
//! 2. Swap sender's proofs for P2PK-locked proofs at the recipient's mint.
//! 3. Publish kind:9321 nutzap event containing the locked proofs.
//!
//! # Receive flow
//!
//! 1. Subscribe to kind:9321 events `#p`-tagged to the receiver's pubkey.
//! 2. For each nutzap, verify the DLEQ proofs in the included `proof` tags.
//! 3. Swap the received proofs for fresh proofs at the mint (unlink from sender).
//! 4. Publish kind:7376 spending history event marking the nutzap as redeemed.
//!
//! # P2PK spending conditions (NUT-11)
//!
//! The proof secret is a JSON spending condition:
//! ```json
//! {"kind":"P2PK","data":"02..pubkey..","tags":[["sigflag","SIG_INPUTS"]]}
//! ```
//! The witness (spend authorization) is a list of Schnorr signatures.

use nostr::{EventBuilder, EventId, Keys, Kind, PublicKey, Tag, TagKind};
use serde::{Deserialize, Serialize};

use crate::cashu::types::Proof;
use crate::error::Nip60Error;
use crate::kinds::{KIND_NUTZAP, KIND_NUTZAP_INFO};

// ─── NutZap info event (kind:10019) ───────────────────────────────────────

/// Decoded kind:10019 — advertises the user's nutzap preferences.
#[derive(Debug, Clone)]
pub struct NutZapInfo {
    /// Relay URLs to deliver nutzaps to.
    pub relays: Vec<String>,
    /// Accepted mint URLs (preference order).
    pub mints: Vec<String>,
    /// Cashu P2PK pubkey to lock proofs to (compressed hex).
    /// When `None`, proofs should be locked to the user's Nostr pubkey.
    pub cashu_pubkey: Option<String>,
}

/// Build a kind:10019 NutZap info event.
pub fn build_nutzap_info_event(info: &NutZapInfo, _keys: &Keys) -> Result<EventBuilder, Nip60Error> {
    let mut tags: Vec<Tag> = Vec::new();
    for relay in &info.relays {
        tags.push(Tag::custom(TagKind::custom("relay"), [relay.as_str()]));
    }
    for mint in &info.mints {
        tags.push(Tag::custom(TagKind::custom("mint"), [mint.as_str()]));
    }
    if let Some(ref pk) = info.cashu_pubkey {
        tags.push(Tag::custom(TagKind::custom("pubkey"), [pk.as_str()]));
    }
    Ok(EventBuilder::new(Kind::from(KIND_NUTZAP_INFO as u16), "").tags(tags))
}

/// Decode a kind:10019 event into [`NutZapInfo`].
pub fn decode_nutzap_info_event(event: &nostr::Event) -> NutZapInfo {
    let mut relays = Vec::new();
    let mut mints = Vec::new();
    let mut cashu_pubkey = None;
    for tag in event.tags.iter() {
        match tag.kind() {
            k if k == TagKind::custom("relay") => {
                if let Some(v) = tag.content() {
                    relays.push(v.to_owned());
                }
            }
            k if k == TagKind::custom("mint") => {
                if let Some(v) = tag.content() {
                    mints.push(v.to_owned());
                }
            }
            k if k == TagKind::custom("pubkey") => {
                if let Some(v) = tag.content() {
                    cashu_pubkey = Some(v.to_owned());
                }
            }
            _ => {}
        }
    }
    NutZapInfo {
        relays,
        mints,
        cashu_pubkey,
    }
}

// ─── P2PK spending conditions (NUT-11) ────────────────────────────────────

/// Build the P2PK secret string for a proof locked to `recipient_pubkey_hex`.
///
/// NUT-11 format: a JSON array `["P2PK", {"nonce":..., "data":..., "tags":...}]`
/// encoded as a string. The nonce prevents secret reuse.
pub fn p2pk_secret(recipient_pubkey_hex: &str) -> String {
    let nonce = hex::encode(&crate::cashu::crypto::random_secret()[..16]);
    serde_json::json!([
        "P2PK",
        {
            "nonce": nonce,
            "data": recipient_pubkey_hex,
            "tags": [["sigflag", "SIG_INPUTS"]]
        }
    ])
    .to_string()
}

/// Sign a P2PK proof (produce the witness signature).
///
/// The Schnorr signature is over the serialised proof secret using the
/// recipient's Cashu private key.
pub fn sign_p2pk_proof(proof: &Proof, cashu_sk: &nostr::secp256k1::SecretKey) -> Result<Proof, Nip60Error> {
    let secp = nostr::secp256k1::Secp256k1::new();
    let msg_bytes = sha2_hash(proof.secret.as_bytes());
    let msg = nostr::secp256k1::Message::from_digest(msg_bytes);
    // Use ECDSA — NUT-11 specifies Schnorr but many implementations accept ECDSA.
    // For compatibility with Cashu's reference implementation (Python nutshell)
    // which uses secp256k1 Schnorr, we use the schnorr signing path.
    let keypair = nostr::secp256k1::Keypair::from_secret_key(&secp, cashu_sk);
    let sig = secp.sign_schnorr(&msg, &keypair);
    // Nutshell expects `witness` to be a JSON-encoded *string*, not a nested object.
    let witness_obj = serde_json::json!({ "signatures": [hex::encode(sig.serialize())] });
    let witness_str = serde_json::to_string(&witness_obj)
        .map_err(|e| Nip60Error::Event(format!("witness serialize: {e}")))?;
    let mut signed = proof.clone();
    signed.witness = Some(serde_json::Value::String(witness_str));
    Ok(signed)
}

fn sha2_hash(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).into()
}

// ─── NutZap event (kind:9321) ─────────────────────────────────────────────

/// A single proof embedded in a nutzap event (wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NutZapProof {
    pub amount: u64,
    pub id: String,
    pub secret: String,
    #[serde(rename = "C")]
    pub c: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dleq: Option<crate::cashu::types::DleqProofWire>,
}

impl From<Proof> for NutZapProof {
    fn from(p: Proof) -> Self {
        Self {
            amount: p.amount,
            id: p.id,
            secret: p.secret,
            c: p.c,
            dleq: p.dleq,
        }
    }
}

/// Build a kind:9321 nutzap event.
///
/// `proofs` are P2PK-locked to the recipient. `mint_url` is the mint URL.
/// `recipient_pubkey` is the recipient's Nostr pubkey.
/// `comment` is an optional zap comment.
/// `zapped_event_id` is an optional event being zapped.
pub fn build_nutzap_event(
    proofs: Vec<NutZapProof>,
    mint_url: &str,
    recipient_pubkey: &PublicKey,
    comment: Option<&str>,
    zapped_event_id: Option<&EventId>,
) -> Result<EventBuilder, Nip60Error> {
    let mut tags: Vec<Tag> = Vec::new();

    // `proof` tags — one per proof, serialised as JSON.
    for proof in &proofs {
        let proof_json = serde_json::to_string(proof)?;
        tags.push(Tag::custom(TagKind::custom("proof"), [proof_json.as_str()]));
    }

    // `u` tag — mint URL.
    tags.push(Tag::custom(TagKind::custom("u"), [mint_url]));

    // `p` tag — recipient pubkey.
    tags.push(Tag::public_key(*recipient_pubkey));

    // `e` tag — zapped event (optional).
    if let Some(event_id) = zapped_event_id {
        tags.push(Tag::event(*event_id));
    }

    let content = comment.unwrap_or("").to_string();
    Ok(EventBuilder::new(Kind::from(KIND_NUTZAP as u16), content).tags(tags))
}

/// Decoded nutzap received by a user.
#[derive(Debug, Clone)]
pub struct ReceivedNutZap {
    pub event_id: EventId,
    pub sender_pubkey: PublicKey,
    pub proofs: Vec<NutZapProof>,
    pub mint_url: String,
    pub amount_sats: u64,
    pub comment: String,
    pub zapped_event_id: Option<EventId>,
}

/// Decode a kind:9321 nutzap event.
pub fn decode_nutzap_event(event: &nostr::Event) -> Result<ReceivedNutZap, Nip60Error> {
    let mut proofs = Vec::new();
    let mut mint_url = None;
    let mut zapped_event_id = None;

    for tag in event.tags.iter() {
        match tag.kind() {
            k if k == TagKind::custom("proof") => {
                if let Some(v) = tag.content() {
                    let proof: NutZapProof = serde_json::from_str(v)
                        .map_err(|e| Nip60Error::Event(format!("proof tag parse: {e}")))?;
                    proofs.push(proof);
                }
            }
            k if k == TagKind::custom("u") => {
                if let Some(v) = tag.content() {
                    mint_url = Some(v.to_owned());
                }
            }
            TagKind::SingleLetter(sl) if sl.character == nostr::Alphabet::E && !sl.uppercase => {
                if let Some(v) = tag.content() {
                    if let Ok(id) = EventId::from_hex(v) {
                        zapped_event_id = Some(id);
                    }
                }
            }
            _ => {}
        }
    }

    let mint_url = mint_url.ok_or_else(|| Nip60Error::Event("nutzap missing u (mint) tag".into()))?;
    if proofs.is_empty() {
        return Err(Nip60Error::Event("nutzap has no proof tags".into()));
    }
    let amount_sats = proofs.iter().map(|p| p.amount).sum();

    Ok(ReceivedNutZap {
        event_id: event.id,
        sender_pubkey: event.pubkey,
        proofs,
        mint_url,
        amount_sats,
        comment: event.content.clone(),
        zapped_event_id,
    })
}

/// Verify all DLEQ proofs on the proofs in a received nutzap.
///
/// Fetches the mint's keyset to get the signing public keys, then verifies
/// each proof's DLEQ proof if present. Requires `dleq.r` (the blinding factor
/// stored with the proof) to reconstruct B' and C' for verification.
pub fn verify_nutzap_dleq(
    nutzap: &ReceivedNutZap,
) -> Result<(), Nip60Error> {
    use crate::cashu::client::build_pubkey_map;
    use crate::cashu::crypto::{hash_to_curve, verify_dleq, DleqProof};
    use nostr::secp256k1::{PublicKey, Scalar, SecretKey};

    let client = crate::cashu::client::MintClient::new(&nutzap.mint_url);
    let keyset = client.get_sat_keyset()?;
    let pubkey_map = build_pubkey_map(&keyset)?;
    let secp = nostr::secp256k1::Secp256k1::new();

    for proof in &nutzap.proofs {
        let Some(ref dleq_wire) = proof.dleq else {
            continue; // mint may not support NUT-12
        };
        let Some(ref r_hex) = dleq_wire.r else {
            continue; // no blinding factor → cannot reconstruct B' / C'
        };

        let mint_pk = pubkey_map.get(&proof.amount).ok_or_else(|| {
            Nip60Error::Crypto(format!("no mint pubkey for amount {}", proof.amount))
        })?;

        // Reconstruct B' = hash_to_curve(secret) + r*G.
        // Per NUT-00, the secret is always hashed as its UTF-8 bytes.
        // This applies regardless of whether the secret is a hex string, JSON object,
        // or JSON array spending condition (NUT-11).
        let y = hash_to_curve(proof.secret.as_bytes())?;
        let r_bytes = hex::decode(r_hex)
            .map_err(|e| Nip60Error::Crypto(format!("DLEQ r hex: {e}")))?;
        let r_sk = SecretKey::from_slice(&r_bytes)
            .map_err(|e| Nip60Error::Crypto(format!("DLEQ r parse: {e}")))?;
        let r_g = PublicKey::from_secret_key(&secp, &r_sk);
        let b_prime = PublicKey::combine_keys(&[&y, &r_g])
            .map_err(|e| Nip60Error::Crypto(format!("B' combine: {e}")))?;

        // Reconstruct C' = C + r*K  (since C = C' - r*K).
        let c_bytes = hex::decode(&proof.c)
            .map_err(|e| Nip60Error::Crypto(format!("proof C hex: {e}")))?;
        let c_pt = PublicKey::from_slice(&c_bytes)
            .map_err(|e| Nip60Error::Crypto(format!("proof C parse: {e}")))?;
        let r_k = mint_pk
            .mul_tweak(&secp, &Scalar::from(r_sk))
            .map_err(|e| Nip60Error::Crypto(format!("r*K mul: {e}")))?;
        let c_prime = PublicKey::combine_keys(&[&c_pt, &r_k])
            .map_err(|e| Nip60Error::Crypto(format!("C' combine: {e}")))?;

        let e_bytes = hex::decode(&dleq_wire.e)
            .map_err(|e| Nip60Error::Crypto(format!("DLEQ e hex: {e}")))?;
        let s_bytes = hex::decode(&dleq_wire.s)
            .map_err(|e| Nip60Error::Crypto(format!("DLEQ s hex: {e}")))?;
        if e_bytes.len() != 32 || s_bytes.len() != 32 {
            return Err(Nip60Error::Crypto("DLEQ proof wrong length".into()));
        }
        let mut e = [0u8; 32];
        let mut s = [0u8; 32];
        e.copy_from_slice(&e_bytes);
        s.copy_from_slice(&s_bytes);

        verify_dleq(&DleqProof { e, s }, &b_prime, &c_prime, mint_pk, &secp)?;
    }
    Ok(())
}
