//! NMP-native domain record shapes for Marmot.
//!
//! These are the LMDB-persisted projections (per plan §Step 1). They carry
//! NO MLS / MDK types — the cryptographic ratchet state lives entirely in
//! `nmp-marmot`'s dedicated SQLite file (owned by [`crate::service`]). These
//! records exist so the rest of NMP can join Marmot facts via the kernel's
//! composite-key reverse index without any MLS awareness (kernel-boundary
//! exit gate).
//!
//! Group identity here is the hex-encoded MLS group id (`group_id_hex`) — a
//! stable opaque string. The group relay URL is carried alongside so routing
//! never has to derive it from a wire shape.

use serde::{Deserialize, Serialize};

/// Display metadata for a joined / pending Marmot group. Projected from
/// `mdk_core::prelude::group_types::Group`; carries no MLS state.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MarmotGroupRecord {
    /// Hex-encoded MLS group id (stable primary key).
    pub group_id_hex: String,
    /// The group relay all kind:445 events are pinned to.
    pub group_relay_url: String,
    pub name: String,
    pub description: String,
    /// Hex-encoded admin pubkeys.
    pub admin_pubkeys: Vec<String>,
    /// MLS epoch (advances on every commit).
    pub epoch: u64,
    /// `"active" | "inactive" | "pending"` — flattened `GroupState`.
    pub state: String,
    /// Unix-seconds timestamp of the last message, if any.
    pub last_message_at: Option<u64>,
}

/// A decrypted application message, keyed by group + epoch + sender.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MarmotMessageRecord {
    pub group_id_hex: String,
    /// Rumor event id (the inner plaintext message id).
    pub message_id: String,
    pub sender_pubkey: String,
    /// MLS epoch the message was decrypted in.
    pub epoch: Option<u64>,
    pub created_at: u64,
    pub kind: u32,
    pub content: String,
}

/// Tracks an own / peer published KeyPackage (as a Nostr event) and its
/// rotation lifecycle (`d_tag` reuse + `hash_ref` consumption tracking).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MarmotKeyPackageRecord {
    /// Owner pubkey (hex). For own key packages this is the local identity.
    pub owner_pubkey: String,
    /// The Nostr event id of the kind:30443 publication.
    pub event_id: String,
    /// The `d` tag value — reused on rotation for relay-side replacement.
    pub d_tag: String,
    /// Hex-encoded postcard-serialized `KeyPackageRef` for lifecycle tracking.
    pub hash_ref_hex: String,
    /// When this key package was published (unix seconds). Drives TTL re-publish.
    pub published_at: u64,
    /// `true` once consumed by an inbound Welcome (triggers immediate rotation).
    pub consumed: bool,
}

/// Tracks a pending inbound Welcome (kind:444 rumor unwrapped from a NIP-59
/// gift-wrap) awaiting accept / decline.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MarmotWelcomeRecord {
    /// The kind:1059 gift-wrap event id that carried this Welcome.
    pub wrapper_event_id: String,
    /// Hex MLS group id this Welcome would join.
    pub group_id_hex: String,
    /// Pubkey of the inviter.
    pub inviter_pubkey: String,
    /// `"pending" | "accepted" | "declined" | "failed"`.
    pub state: String,
}
