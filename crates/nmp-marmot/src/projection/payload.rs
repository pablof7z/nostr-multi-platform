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
//!
//! ## Raw data only (aim.md §2)
//!
//! Per aim.md §2, NMP is a data framework: projection and snapshot code
//! sends raw protocol data (hex pubkeys, Unix timestamps, raw counts).
//! Presentation layers (Swift, Kotlin, TUI, web) own all formatting:
//! bech32 encoding, abbreviation, avatar initials/tints for pubkeys,
//! relative-time labels, plural-count strings. This module ships counts
//! as `u32`/`u64`, timestamps as `u64`, pubkeys as 64-char hex.
//!
//! Free-form metadata fallbacks (empty group name → "Untitled group",
//! empty welcome name → "Group invite", pluralised invite chip label,
//! 2-char initials over the group name) are still computed here — those
//! are protocol-level decisions about how to surface a name field with
//! no kind-defined empty-string semantics, not banned helper
//! forwarders.

use serde::{Deserialize, Serialize};

/// One group row in the snapshot. `id_hex` is the MLS group id hex-encoded
/// (the opaque handle Swift passes back to `group_messages` / `dispatch`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MarmotGroupRow {
    pub id_hex: String,
    /// Group name verbatim from MLS metadata. May be empty — presentation
    /// layers own the rendering decision (typically a fallback string).
    pub name: String,
    /// Group name with an empty-name fallback ("Untitled group") so the UI
    /// can render `display_name` unconditionally. Free-form metadata
    /// fallback for the name field, NOT a banned pubkey/timestamp
    /// formatter — see aim.md §2.
    pub display_name: String,
    /// 2-char ASCII initials for the avatar tile derived from `name`
    /// (rendered on a flat background by the UI). Free-form metadata
    /// derivation, not a banned pubkey/timestamp formatter.
    pub initials: String,
    /// Member Nostr pubkeys (hex, 64 chars), sorted (BTreeSet order from
    /// MDK). Presentation layer formats each entry for display.
    pub members: Vec<String>,
    /// Member count (length of `members`, surfaced separately so a host
    /// shell can render the count without iterating the array).
    pub member_count: u32,
    /// Total decrypted application-message count for the group, or `None`
    /// when zero. **Read-cursor seam**: there is no per-device read
    /// watermark in `MarmotService` / MDK, so this is NOT a true unread
    /// delta — the host shell owns the read watermark and computes
    /// `unread = unread_count - last_seen_count` itself. Carries a count
    /// rather than a display string (aim.md §2).
    pub unread_count: Option<u32>,
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
    /// Group name verbatim from the Welcome envelope. May be empty —
    /// presentation layers own the empty-name fallback.
    pub group_name: String,
    /// Empty-name fallback ("Group invite") so the UI renders this string
    /// unconditionally. Free-form metadata fallback, not a banned
    /// pubkey/timestamp formatter — see aim.md §2.
    pub display_name: String,
    /// The inviter's Nostr pubkey (hex, 64 chars — the field name is
    /// historical; the value is hex, not bech32). The gift-wrap seal
    /// sender. Presentation layer formats for display.
    pub inviter_npub: String,
}

/// KeyPackage publication health for the local identity.
///
/// Raw status struct — shells (Swift, Kotlin) own all presentation formatting
/// (subtitles, button labels, bucketed age strings). Per aim.md §2: Rust sends
/// raw data only. Presentation layers derive copy from `published`, `age_secs`,
/// `stale`, and `is_registered`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
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
    /// `true` when this status was built against a registered Marmot signing
    /// identity. `false` only when no handle exists (empty snapshot path).
    /// Shells gate the publish button and derive subtitle copy from this +
    /// `published` / `age_secs` / `stale`.
    pub is_registered: bool,
}

/// Summary of one pending (deferred) op waiting for a peer's KP to arrive.
/// Surfaced in the snapshot so native can render "Waiting for key packages…"
/// state without polling. Rust owns the `display_label` string.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PendingOpRow {
    /// Original action correlation_id minted when the op was dispatched.
    pub correlation_id: String,
    /// Op tag ("create_group" | "invite") for display branching.
    pub op_tag: String,
    /// Number of peer KPs still missing.
    pub missing_count: u32,
    /// Rust-owned display string (e.g. "Waiting for key packages (1)…"). The
    /// projection owns formatting; native renders verbatim.
    pub display_label: String,
    /// Wall-clock seconds since the op was parked (`now_secs - created_at`),
    /// computed fresh at snapshot time. Gives the host temporal context to
    /// show elapsed wait time / fade a spinner as the 60 s deadline nears.
    #[serde(default)]
    pub age_secs: u64,
}

/// The most recent terminal op FAILURE surfaced in the snapshot — a
/// deferred-op expiry or a failed retry. Raw data only (aim.md §2): `reason`
/// is the machine code (e.g. `"key_package_unavailable"`); native maps it to
/// a banner string. `op` + `correlation_id` let the host correlate the
/// failure with the action it dispatched; `at_secs` is when it was recorded.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct LastOpError {
    /// Failing op tag ("create_group" | "invite").
    pub op: String,
    /// Machine code, e.g. "key_package_unavailable".
    pub reason: String,
    /// Wall-clock second the failure was recorded.
    pub at_secs: u64,
    /// correlation_id of the failed action.
    pub correlation_id: String,
}

/// Complete snapshot hosts consume via the `nmp.marmot.snapshot` push projection.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MarmotSnapshot {
    pub groups: Vec<MarmotGroupRow>,
    pub pending_welcomes: Vec<PendingWelcomeRow>,
    pub key_package: KeyPackageStatus,
    /// Pubkeys (hex) of peers whose signed KeyPackage events are cached in
    /// `MarmotService::kp_cache`. Populated by the tap when the kernel
    /// delivers a peer's kind:30443/443 event. Native renders this as
    /// pending/completed state; Rust owns when lookup interests are opened.
    pub cached_kp_pubkeys: Vec<String>,
    /// Pluralised label for the top-of-list pending-invites chip
    /// (`"1 invite"` / `"3 invites"`), or `None` when there are no
    /// pending welcomes. The UI renders this string verbatim — no
    /// `.count == 1 ? "" : "s"` decision in Swift.
    pub invites_chip_label: Option<String>,
    /// `true` when this snapshot was built against a registered Marmot
    /// signing identity. `false` only for `empty()` (no handle on the iOS
    /// side, so the snapshot path was never taken in Rust). Lets the host
    /// render `KeyPackageStatus.subtitle` verbatim without re-branching on a
    /// separately-plumbed `is_registered` flag.
    #[serde(default)]
    pub is_registered: bool,
    /// Cumulative count of `PendingGroupChange` / `CreateGroupPending` handles
    /// dropped without commit/clear this session (V-61 diagnostic).
    ///
    /// A non-zero value means a kind:445/commit event was produced locally but
    /// may not have reached the relay — local MLS state and the
    /// relay-published epoch may have diverged for one or more groups. The
    /// host should block further group sends and surface a recovery prompt.
    /// Resets to zero only when the `MarmotService` is re-initialized (e.g.
    /// on the next app launch). This is an additive field — older hosts that
    /// do not read it degrade gracefully.
    #[serde(default)]
    pub orphaned_commit_count: u32,
    /// `true` when Marmot was initialized with an in-memory credential store
    /// because the platform keyring was unavailable at registration time
    /// (V-62 diagnostic).
    ///
    /// When `true`, MLS group secrets live only in process memory — they are
    /// lost on the next launch and every group becomes unjoinable. The host
    /// should surface a prominent warning and block group features until the
    /// user resolves the keyring issue (e.g. via Keychain access prompt,
    /// device unlock, or app re-install). This field is `false` when the real
    /// platform keyring is in use and is never set to `true` silently. This
    /// is an additive field — older hosts that do not read it degrade
    /// gracefully.
    #[serde(default)]
    pub keyring_unavailable: bool,
    /// Ops parked in the deferred-completion store, waiting for peer KPs.
    /// Empty when no ops are pending. Native renders each row's `display_label`
    /// verbatim alongside the relevant group action's spinner. Additive —
    /// older hosts that do not read this field degrade gracefully.
    #[serde(default)]
    pub pending_ops: Vec<PendingOpRow>,
    /// Most recent terminal op failure, or `None` when no op has failed this
    /// session. Set by the projection on deferred-op expiry / failed retry and
    /// CLEARED when the next marmot op succeeds, so the banner never lingers
    /// stale. Additive — older hosts degrade gracefully.
    #[serde(default)]
    pub last_op_error: Option<LastOpError>,
}

impl MarmotSnapshot {
    /// D6 — degraded/empty snapshot (poisoned mutex, service init failure).
    /// Returned by the iOS shell whenever no `MarmotHandle` exists; the
    /// kernel-side snapshot path always sets `is_registered = true`.
    #[must_use]
    pub fn empty() -> Self {
        let kp = KeyPackageStatus::default();
        Self {
            groups: Vec::new(),
            pending_welcomes: Vec::new(),
            key_package: kp,
            cached_kp_pubkeys: Vec::new(),
            invites_chip_label: None,
            is_registered: false,
            orphaned_commit_count: 0,
            keyring_unavailable: false,
            pending_ops: Vec::new(),
            last_op_error: None,
        }
    }
}

/// One decrypted message row, delivered via the `nmp.marmot.messages` projection.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MarmotMessageRow {
    /// The message (rumor) event id, hex.
    pub id: String,
    /// Author Nostr pubkey (hex, 64 chars). The presentation layer formats
    /// for display.
    pub sender_pubkey_hex: String,
    pub content: String,
    /// Rumor `created_at` (sender clock, Unix seconds). The presentation
    /// layer formats for display (aim.md §2).
    pub created_at: u64,
    /// MLS epoch the message was decrypted at, or `null` (pre-epoch msgs).
    pub epoch: Option<u64>,
}
