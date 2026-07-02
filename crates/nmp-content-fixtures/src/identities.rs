//! The five deterministic test identities.
//!
//! Each is built from a fixed 32-byte secret-key hex seed. Signing is fully
//! deterministic — BIP340 schnorr with **no auxiliary randomness**
//! (`sign_schnorr_no_aux_rand`) — so both the event id AND the signature are
//! byte-stable across runs. This makes the generated gallery bundles
//! reproducible (`cargo run …build-…-bundle && git diff --exit-code` is clean)
//! and keeps screenshots diffable. Keys NEVER touch a relay; this is offline
//! fixture material only.

use nmp_nostr_id::{format_nostr_uri, NostrUri};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
use nostr::event::builder::EventBuilder;
use nostr::key::Keys;
use nostr::secp256k1::{Keypair, Message, Secp256k1};
use nostr::types::Timestamp;
use nostr::{Kind, Tag};

/// A named fixture identity with its signing keys + derived bech32 forms.
pub struct Identity {
    /// Symbolic alias (`ALICE`, `BOB`, …).
    pub alias: &'static str,
    /// 32-byte pubkey hex.
    pub pubkey_hex: String,
    /// The deterministic signing keypair (from the fixed seed).
    keys: Keys,
}

impl Identity {
    fn new(alias: &'static str, secret_hex: &str) -> Self {
        let keys =
            Keys::parse(secret_hex).expect("deterministic 32-byte secret hex must construct keys");
        let pubkey_hex = keys.public_key().to_hex();
        Self {
            alias,
            pubkey_hex,
            keys,
        }
    }

    /// `nostr:npub…` for this identity (no relay hints).
    pub fn npub_uri(&self) -> String {
        format_nostr_uri(&NostrUri::Profile {
            pubkey: self.pubkey_hex.clone(),
            relays: vec![],
        })
        .expect("npub format from valid pubkey hex")
    }

    /// `nostr:nprofile…` for this identity (one relay hint so the entity
    /// encodes as `nprofile`, exercising the relay-hint path).
    pub fn nprofile_uri(&self) -> String {
        format_nostr_uri(&NostrUri::Profile {
            pubkey: self.pubkey_hex.clone(),
            relays: vec!["wss://relay.nmp.test".to_string()],
        })
        .expect("nprofile format from valid pubkey hex")
    }

    /// Sign an event with this identity, **deterministically**.
    ///
    /// Uses BIP340 schnorr with no auxiliary randomness
    /// (`sign_schnorr_no_aux_rand`), so the signature is byte-identical across
    /// runs for the same (kind, created_at, tags, content) inputs. The event id
    /// is the canonical NIP-01 hash (already deterministic). Both are valid:
    /// the bundle's `every_signed_event_verifies` test re-verifies the full
    /// schnorr signature + id hash.
    pub fn sign(
        &self,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
        content: impl Into<String>,
    ) -> SignedEvent {
        let content: String = content.into();
        let kind_u16 = u16::try_from(kind).expect("fixture kinds fit in u16");
        let parsed_tags: Vec<Tag> = tags
            .iter()
            .map(|t| Tag::parse(t.clone()))
            .collect::<Result<_, _>>()
            .expect("fixture tags are well-formed");

        // Build the unsigned event (id computed from the canonical NIP-01
        // serialization). `custom_created_at` pins the timestamp so the id is
        // deterministic — never `Timestamp::now()`.
        let mut unsigned = EventBuilder::new(Kind::from_u16(kind_u16), &content)
            .tags(parsed_tags)
            .custom_created_at(Timestamp::from(created_at))
            .build(self.keys.public_key());
        let id = unsigned.id();

        // Sign the id digest with NO auxiliary randomness → deterministic sig.
        let secp = Secp256k1::new();
        let message = Message::from_digest(id.to_bytes());
        let keypair = Keypair::from_secret_key(&secp, self.keys.secret_key());
        let sig = secp.sign_schnorr_no_aux_rand(&message, &keypair);

        let event = unsigned
            .add_signature(sig)
            .expect("manually-attached deterministic schnorr signature must verify");

        SignedEvent {
            id: event.id.to_hex(),
            sig: event.sig.to_string(),
            unsigned: UnsignedEvent {
                pubkey: event.pubkey.to_hex(),
                kind: u32::from(event.kind.as_u16()),
                tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: event.content.clone(),
                created_at: event.created_at.as_secs(),
            },
        }
    }
}

/// `nostr:note…` for a bare event id (no relay/author/kind hints).
pub fn note_uri(event_id_hex: &str) -> String {
    format_nostr_uri(&NostrUri::Event {
        event_id: event_id_hex.to_string(),
        relays: vec![],
        author: None,
        kind: None,
    })
    .expect("note format from valid event id hex")
}

/// `nostr:nevent…` carrying a relay hint + author + kind (forces the
/// `nevent` encoding, exercising the hint path).
pub fn nevent_uri(event_id_hex: &str, author_hex: &str, kind: u32) -> String {
    format_nostr_uri(&NostrUri::Event {
        event_id: event_id_hex.to_string(),
        relays: vec!["wss://relay.nmp.test".to_string()],
        author: Some(author_hex.to_string()),
        kind: Some(kind),
    })
    .expect("nevent format from valid event id hex")
}

/// `nostr:naddr…` for an addressable coordinate (`kind:pubkey:d`).
pub fn naddr_uri(kind: u32, pubkey_hex: &str, d_tag: &str) -> String {
    format_nostr_uri(&NostrUri::Address {
        identifier: d_tag.to_string(),
        pubkey: pubkey_hex.to_string(),
        kind,
        relays: vec![],
    })
    .expect("naddr format from valid coordinate")
}

/// The fixture identity set, constructed once.
pub struct Identities {
    /// Primary author.
    pub alice: Identity,
    /// Quoted / mentioned author.
    pub bob: Identity,
    /// Article author, list owner.
    pub carol: Identity,
    /// Profile-without-metadata author.
    pub dave: Identity,
    /// Cycle partner.
    pub eve: Identity,
}

fn seed(byte: u8) -> String {
    let mut s = "00".repeat(31);
    s.push_str(&format!("{byte:02x}"));
    s
}

impl Identities {
    /// Build the deterministic identity set.
    pub fn new() -> Self {
        Self {
            alice: Identity::new("ALICE", &seed(1)),
            bob: Identity::new("BOB", &seed(2)),
            carol: Identity::new("CAROL", &seed(3)),
            dave: Identity::new("DAVE", &seed(4)),
            eve: Identity::new("EVE", &seed(5)),
        }
    }
}

impl Default for Identities {
    fn default() -> Self {
        Self::new()
    }
}
