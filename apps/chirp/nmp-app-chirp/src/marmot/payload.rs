//! Wire-shape carried across the Marmot FFI to Swift.
//!
//! Every struct here is a flat, decoder-free DTO: Swift mirrors the serde
//! shape 1:1 and renders directly. No MLS / MDK type ever appears — the
//! `nmp-marmot` `MarmotService` is the typed translation layer (opaque
//! `group_id` as hex string, errors as strings) and this crate flattens
//! its outputs one more step for the C-ABI JSON boundary.
//!
//! The iOS shell depends on these field names VERBATIM. Treat any rename
//! as a breaking ABI change.

use serde::{Deserialize, Serialize};

/// One group row in the snapshot. `id_hex` is the MLS group id hex-encoded
/// (the opaque handle Swift passes back to `group_messages` / `dispatch`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MarmotGroupRow {
    pub id_hex: String,
    pub name: String,
    /// Member Nostr pubkeys (hex), sorted (BTreeSet order from MDK).
    pub members: Vec<String>,
    /// **Read-cursor seam**: there is no per-device read watermark in
    /// `MarmotService` / MDK, so this is the TOTAL decrypted
    /// application-message count for the group, NOT a true unread delta.
    /// The iOS shell owns the read watermark and computes
    /// `unread = this - last_seen_count` itself. The field keeps the name
    /// `unread` because the iOS agent's schema is pinned to it; treat it
    /// as "message_count" until a read-cursor lands.
    pub unread: u64,
    /// Sender `created_at` of the most recent message, or `null` if none.
    pub last_msg_at: Option<u64>,
}

/// One pending (un-accepted) Welcome the local user has received.
///
/// `MarmotService` exposes no `get_pending_welcomes`, so these rows are
/// served from the in-handle cache populated when a kind:1059 gift-wrap is
/// fed in via the `ingest_signed_event` dispatch op (see `state.rs`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PendingWelcomeRow {
    /// The kind:444 Welcome event id, hex. Pass back as `welcome_id_hex`
    /// to the `accept_welcome` / `decline_welcome` dispatch ops.
    pub id_hex: String,
    pub group_name: String,
    /// The inviter's Nostr pubkey, hex (the gift-wrap seal sender).
    pub inviter_npub: String,
}

/// KeyPackage publication health for the local identity.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct KeyPackageStatus {
    /// `true` once `publish_key_package` has been dispatched this session
    /// (the signing seam is in-crate, so this is authoritative for the
    /// current process — see the `published`/`stale` seam in `state.rs`).
    pub published: bool,
    /// The kind:30443 `d` tag of the most recent publication, or `null`.
    pub d_tag: Option<String>,
    /// Seconds since the most recent publication, or `null` if never
    /// published this session.
    pub age_secs: Option<u64>,
    /// `true` when `age_secs` exceeds the 7-day rotation threshold.
    pub stale: bool,
}

impl Default for KeyPackageStatus {
    fn default() -> Self {
        Self {
            published: false,
            d_tag: None,
            age_secs: None,
            stale: false,
        }
    }
}

/// Complete snapshot Swift consumes via `nmp_app_chirp_marmot_snapshot`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MarmotSnapshot {
    pub groups: Vec<MarmotGroupRow>,
    pub pending_welcomes: Vec<PendingWelcomeRow>,
    pub key_package: KeyPackageStatus,
    /// Pubkeys (hex) of peers whose signed KeyPackage events are cached in
    /// `MarmotService::kp_cache`. Populated by the tap when the kernel
    /// delivers a peer's kind:30443/443 event. Swift checks this to know
    /// when `fetch_key_packages` has delivered results and a retry of
    /// `create_group`/`invite` will succeed.
    pub cached_kp_pubkeys: Vec<String>,
}

impl MarmotSnapshot {
    /// D6 — degraded/empty snapshot (poisoned mutex, service init failure).
    pub fn empty() -> Self {
        Self {
            groups: Vec::new(),
            pending_welcomes: Vec::new(),
            key_package: KeyPackageStatus::default(),
            cached_kp_pubkeys: Vec::new(),
        }
    }
}

/// One decrypted message row from `nmp_app_chirp_marmot_group_messages`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MarmotMessageRow {
    /// The message (rumor) event id, hex.
    pub id: String,
    /// Author Nostr pubkey, hex.
    pub sender_npub: String,
    pub content: String,
    /// Rumor `created_at` (sender clock).
    pub created_at: u64,
    /// MLS epoch the message was decrypted at, or `null` (pre-epoch msgs).
    pub epoch: Option<u64>,
}
