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
use crate::kinds::{KIND_NIP61_NUTZAP, KIND_NIP61_NUTZAP_INFO};

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
pub fn build_nutzap_info_event(
    info: &NutZapInfo,
    _keys: &Keys,
) -> Result<EventBuilder, Nip60Error> {
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
    Ok(EventBuilder::new(Kind::from(KIND_NIP61_NUTZAP_INFO as u16), "").tags(tags))
}

/// Decode a kind:10019 event into [`NutZapInfo`].
pub fn decode_nutzap_info_event(event: &nostr::Event) -> NutZapInfo {
    decode_nutzap_info_fields(
        &event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect::<Vec<_>>(),
    )
}

/// Decode a kind:10019 event's raw tag rows into [`NutZapInfo`], without
/// requiring a verified `nostr::Event`.
///
/// A [`crate::kinds::KIND_NIP61_NUTZAP_INFO`] event a caller reads back from
/// `nmp_core::substrate::KernelEvent` (id/author/kind/tags/content, no
/// signature — the kernel already verified it before handing it out) cannot
/// be reconstructed into a real `nostr::Event` (`Event::new` requires a real
/// [`nostr::secp256k1::schnorr::Signature`], which `KernelEvent` never
/// carries). This is the tag-rows-only twin of [`decode_nutzap_info_event`]
/// for exactly that caller — both share this one decode body.
#[must_use]
pub fn decode_nutzap_info_fields(tags: &[Vec<String>]) -> NutZapInfo {
    let mut relays = Vec::new();
    let mut mints = Vec::new();
    let mut cashu_pubkey = None;
    for row in tags {
        match (row.first().map(String::as_str), row.get(1)) {
            (Some("relay"), Some(v)) => relays.push(v.clone()),
            (Some("mint"), Some(v)) => mints.push(v.clone()),
            (Some("pubkey"), Some(v)) => cashu_pubkey = Some(v.clone()),
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

/// The pubkey a P2PK proof secret locks to, or `None` if `secret` is not a
/// well-formed NUT-11 `["P2PK", {..}]` spending condition (e.g. an ordinary
/// random hex secret on a non-P2PK proof).
///
/// Used on receive to verify a nutzap proof is actually locked to THIS
/// wallet's Cashu pubkey before redeeming it (nip60-nip61-wallet-design.md,
/// "Receiving": "verify that each P2PK secret locks to the active Cashu P2PK
/// pubkey") — never trust the sender's claimed lock without checking.
#[must_use]
pub fn p2pk_secret_pubkey(secret: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(secret).ok()?;
    let arr = parsed.as_array()?;
    if arr.first()?.as_str()? != "P2PK" {
        return None;
    }
    arr.get(1)?.get("data")?.as_str().map(str::to_string)
}

/// Sign a P2PK proof (produce the witness signature).
///
/// The Schnorr signature is over the serialised proof secret using the
/// recipient's Cashu private key — routed through the audited `cashu`
/// crate's `SecretKey::sign`, which does exactly this sequence internally
/// (`SHA256(msg)` then `secp256k1::sign_schnorr`), rather than hand-rolling
/// it. NUT-11 specifies Schnorr, and that's what Cashu's reference
/// implementation (Python `nutshell`) actually verifies.
pub fn sign_p2pk_proof(
    proof: &Proof,
    cashu_sk: &nostr::secp256k1::SecretKey,
) -> Result<Proof, Nip60Error> {
    let cashu_sk = cashu::SecretKey::from_slice(&cashu_sk.secret_bytes())
        .map_err(|e| Nip60Error::Crypto(format!("P2PK secret key → cashu: {e}")))?;
    let sig = cashu_sk
        .sign(proof.secret.as_bytes())
        .map_err(|e| Nip60Error::Crypto(format!("P2PK schnorr sign: {e}")))?;
    // Nutshell expects `witness` to be a JSON-encoded *string*, not a nested object.
    let witness_obj = serde_json::json!({ "signatures": [hex::encode(sig.serialize())] });
    let witness_str = serde_json::to_string(&witness_obj)
        .map_err(|e| Nip60Error::Event(format!("witness serialize: {e}")))?;
    let mut signed = proof.clone();
    signed.witness = Some(serde_json::Value::String(witness_str));
    Ok(signed)
}

// ─── NutZap event (kind:9321) ─────────────────────────────────────────────

/// A single proof embedded in a nutzap event (wire format).
#[derive(Clone, Serialize, Deserialize)]
pub struct NutZapProof {
    pub amount: u64,
    pub id: String,
    /// The proof secret — spendable ecash; never printed in `Debug`.
    pub secret: String,
    #[serde(rename = "C")]
    pub c: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dleq: Option<crate::cashu::types::DleqProofWire>,
}

impl std::fmt::Debug for NutZapProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NutZapProof")
            .field("amount", &self.amount)
            .field("id", &self.id)
            .field("secret", &"<redacted>")
            .field("c", &self.c)
            .field("dleq", &self.dleq)
            .finish()
    }
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
    Ok(EventBuilder::new(Kind::from(KIND_NIP61_NUTZAP as u16), content).tags(tags))
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
    decode_nutzap_fields(
        &event.id.to_hex(),
        &event.pubkey.to_hex(),
        &event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect::<Vec<_>>(),
        &event.content,
    )
}

/// Decode a kind:9321 nutzap event's raw fields into [`ReceivedNutZap`],
/// without requiring a verified `nostr::Event`.
///
/// The receive-side twin of [`decode_nutzap_event`] for a caller holding an
/// `nmp_core::substrate::KernelEvent` (id/author/kind/tags/content, no
/// signature) rather than a `nostr::Event` — see
/// [`decode_nutzap_info_fields`]'s doc comment for why the two cannot be
/// unified through a reconstructed `nostr::Event`.
pub fn decode_nutzap_fields(
    event_id_hex: &str,
    sender_pubkey_hex: &str,
    tags: &[Vec<String>],
    content: &str,
) -> Result<ReceivedNutZap, Nip60Error> {
    let event_id = EventId::from_hex(event_id_hex)
        .map_err(|e| Nip60Error::Event(format!("nutzap event id: {e}")))?;
    let sender_pubkey = PublicKey::from_hex(sender_pubkey_hex)
        .map_err(|e| Nip60Error::Event(format!("nutzap sender pubkey: {e}")))?;

    let mut proofs = Vec::new();
    let mut mint_url = None;
    let mut zapped_event_id = None;

    for row in tags {
        match (row.first().map(String::as_str), row.get(1)) {
            (Some("proof"), Some(v)) => {
                let proof: NutZapProof = serde_json::from_str(v)
                    .map_err(|e| Nip60Error::Event(format!("proof tag parse: {e}")))?;
                proofs.push(proof);
            }
            (Some("u"), Some(v)) => mint_url = Some(v.clone()),
            (Some("e"), Some(v)) => {
                if let Ok(id) = EventId::from_hex(v) {
                    zapped_event_id = Some(id);
                }
            }
            _ => {}
        }
    }

    let mint_url =
        mint_url.ok_or_else(|| Nip60Error::Event("nutzap missing u (mint) tag".into()))?;
    if proofs.is_empty() {
        return Err(Nip60Error::Event("nutzap has no proof tags".into()));
    }
    let amount_sats = proofs.iter().map(|p| p.amount).sum();

    Ok(ReceivedNutZap {
        event_id,
        sender_pubkey,
        proofs,
        mint_url,
        amount_sats,
        comment: content.to_string(),
        zapped_event_id,
    })
}

/// Verify all DLEQ proofs on the proofs in a received nutzap.
///
/// Fetches the mint's keyset to get the signing public keys, then hands off
/// to [`verify_nutzap_dleq_against_keyset`] for the actual (pure, keyset-as-
/// parameter) verification.
///
/// Requires the `native` feature — it round-trips to the mint over HTTP
/// (`crate::cashu::client`) to fetch the keyset.
#[cfg(feature = "native")]
pub fn verify_nutzap_dleq(nutzap: &ReceivedNutZap) -> Result<(), Nip60Error> {
    let client = crate::cashu::client::MintClient::new(&nutzap.mint_url);
    let keyset = client.get_sat_keyset()?;
    verify_nutzap_dleq_against_keyset(nutzap, &keyset)
}

/// The pure DLEQ-verification half of [`verify_nutzap_dleq`], split out (a)
/// so it is unit-testable without a live mint HTTP round-trip and (b) to
/// mirror `blinded::finalize_blinded_outputs`'s own "keyset passed in, no
/// I/O in here" shape.
///
/// Verifies each proof's DLEQ proof, requiring `dleq.r` (the blinding factor
/// stored with the proof) to reconstruct B' and C'.
///
/// # Missing DLEQ fails closed (#2933)
///
/// A nutzap's proofs are a value claim from an untrusted THIRD PARTY (the
/// sender, or a relay serving a forged event) — unlike `MintClient`'s own
/// swap/mint calls, which use `DleqPolicy::VerifyIfPresent` because those
/// round-trips always land at a mint this wallet already accepts (a mint
/// that lies there is a trust decision this wallet already made, not
/// something DLEQ defends against). Skipping a nutzap proof with no DLEQ (or
/// no blinding factor `r`, without which B'/C' cannot be reconstructed) used
/// to let it pass unverified; the design doc's "verify DLEQ before counting"
/// invariant means missing evidence must count as failed evidence.
///
/// Gated to `native`-or-test builds: its only production caller is the
/// `#[cfg(feature = "native")]` [`verify_nutzap_dleq`] wrapper (nutzap DLEQ
/// verification needs the mint's keyset over HTTP), and its unit tests run
/// with the default `native` feature on. Without this gate it would be dead
/// code on the wasm32 / `--no-default-features` pure-codec build.
#[cfg(any(feature = "native", test))]
pub(crate) fn verify_nutzap_dleq_against_keyset(
    nutzap: &ReceivedNutZap,
    keyset: &crate::cashu::types::KeySet,
) -> Result<(), Nip60Error> {
    use crate::cashu::crypto::{hash_to_curve, verify_dleq, DleqProof};
    use crate::cashu::http::build_pubkey_map;
    use nostr::secp256k1::{PublicKey, Scalar, SecretKey};

    let pubkey_map = build_pubkey_map(keyset)?;
    let secp = nostr::secp256k1::Secp256k1::new();

    for proof in &nutzap.proofs {
        let Some(ref dleq_wire) = proof.dleq else {
            return Err(Nip60Error::Crypto(format!(
                "nutzap proof (C={}) has no DLEQ proof — missing DLEQ is rejected, not skipped",
                proof.c
            )));
        };
        let Some(ref r_hex) = dleq_wire.r else {
            return Err(Nip60Error::Crypto(format!(
                "nutzap proof (C={}) DLEQ is missing the blinding factor r",
                proof.c
            )));
        };

        let mint_pk = pubkey_map.get(&proof.amount).ok_or_else(|| {
            Nip60Error::Crypto(format!("no mint pubkey for amount {}", proof.amount))
        })?;

        // Reconstruct B' = hash_to_curve(secret) + r*G.
        // Per NUT-00, the secret is always hashed as its UTF-8 bytes.
        // This applies regardless of whether the secret is a hex string, JSON object,
        // or JSON array spending condition (NUT-11).
        let y = hash_to_curve(proof.secret.as_bytes())?;
        let r_bytes =
            hex::decode(r_hex).map_err(|e| Nip60Error::Crypto(format!("DLEQ r hex: {e}")))?;
        let r_sk = SecretKey::from_slice(&r_bytes)
            .map_err(|e| Nip60Error::Crypto(format!("DLEQ r parse: {e}")))?;
        let r_g = PublicKey::from_secret_key(&secp, &r_sk);
        let b_prime = PublicKey::combine_keys(&[&y, &r_g])
            .map_err(|e| Nip60Error::Crypto(format!("B' combine: {e}")))?;

        // Reconstruct C' = C + r*K  (since C = C' - r*K).
        let c_bytes =
            hex::decode(&proof.c).map_err(|e| Nip60Error::Crypto(format!("proof C hex: {e}")))?;
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

        verify_dleq(
            &DleqProof { e, s },
            &b_prime,
            &c_prime,
            mint_pk,
            proof.amount,
            &proof.id,
            &secp,
        )?;
    }
    Ok(())
}

/// Raw tag rows for a kind:10019 NutZap info event, without an `EventBuilder`
/// or a signing keypair — the twin [`build_nutzap_info_event`] needs
/// underneath, and what a caller building an [`nmp_signer_iface::UnsignedEvent`]
/// through the signer-transparent sign port (rather than `EventBuilder::
/// sign_with_keys`) actually needs (`nmp-wallet`'s `PublishNutzapInfo`, #2917).
#[must_use]
pub fn nutzap_info_tags(info: &NutZapInfo) -> Vec<Vec<String>> {
    let mut tags = Vec::new();
    for relay in &info.relays {
        tags.push(vec!["relay".to_string(), relay.clone()]);
    }
    for mint in &info.mints {
        tags.push(vec!["mint".to_string(), mint.clone()]);
    }
    if let Some(ref pk) = info.cashu_pubkey {
        tags.push(vec!["pubkey".to_string(), pk.clone()]);
    }
    tags
}

/// Raw tag rows for a kind:9321 nutzap event, without an `EventBuilder` or a
/// signing keypair — the twin [`build_nutzap_event`] needs underneath, for
/// the same signer-transparent-port reason as [`nutzap_info_tags`]. Content
/// (the optional public comment) is `comment.unwrap_or("")`, same as
/// `build_nutzap_event` — not part of this function's return since it is not
/// a tag.
pub fn nutzap_event_tags(
    proofs: &[NutZapProof],
    mint_url: &str,
    recipient_pubkey: &PublicKey,
    zapped_event_id: Option<&EventId>,
) -> Result<Vec<Vec<String>>, Nip60Error> {
    let mut tags: Vec<Vec<String>> = Vec::new();
    for proof in proofs {
        let proof_json = serde_json::to_string(proof)?;
        tags.push(vec!["proof".to_string(), proof_json]);
    }
    tags.push(vec!["u".to_string(), mint_url.to_string()]);
    tags.push(vec!["p".to_string(), recipient_pubkey.to_hex()]);
    if let Some(event_id) = zapped_event_id {
        tags.push(vec!["e".to_string(), event_id.to_hex()]);
    }
    Ok(tags)
}

#[cfg(test)]
#[path = "nutzap_tests.rs"]
mod tests;
