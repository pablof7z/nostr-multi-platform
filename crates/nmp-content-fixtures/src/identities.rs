//! The five deterministic test identities.
//!
//! Each is a `nmp_signers::LocalKeySigner` built from a fixed 32-byte
//! secret-key hex seed via `LocalKeySigner::from_secret_hex` — so every
//! signed event id is stable across runs and screenshots stay diffable.
//! Keys NEVER touch a relay; this is offline fixture material only.

use nmp_core::nip21::{format_nostr_uri, NostrUri};
use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nmp_signers::signers::SignerOp;
use nmp_signers::{LocalKeySigner, Signer};
use std::time::Duration;

/// A named fixture identity with its signer + derived bech32 forms.
pub struct Identity {
    /// Symbolic alias (`ALICE`, `BOB`, …).
    pub alias: &'static str,
    /// 32-byte pubkey hex.
    pub pubkey_hex: String,
    signer: LocalKeySigner,
}

impl Identity {
    fn new(alias: &'static str, secret_hex: &str) -> Self {
        let signer = LocalKeySigner::from_secret_hex(secret_hex)
            .expect("deterministic 32-byte secret hex must construct a signer");
        let pubkey_hex = signer.pubkey().to_hex();
        Self {
            alias,
            pubkey_hex,
            signer,
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

    /// Sign an event with this identity. Synchronous: `LocalKeySigner`
    /// returns `SignerOp::Ready`, so `wait` resolves immediately.
    pub fn sign(
        &self,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
        content: impl Into<String>,
    ) -> SignedEvent {
        let unsigned = UnsignedEvent {
            pubkey: self.pubkey_hex.clone(),
            kind,
            tags,
            content: content.into(),
            created_at,
        };
        match self.signer.sign(unsigned) {
            SignerOp::Ready(r) => r.expect("LocalKeySigner sign is infallible"),
            op => op
                .wait(Duration::from_secs(1))
                .expect("LocalKeySigner sign resolves immediately"),
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
