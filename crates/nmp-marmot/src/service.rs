//! `MarmotService` — the real MDK-driving API layer.
//!
//! This is the only module in `nmp-marmot` that touches MDK types. It is
//! consumed in-crate (round-trip tests) and by a future actor/FFI bridge; no
//! other NMP crate depends on it, so the kernel-boundary exit gate
//! ("`nmp-marmot` is the sole importer of `mdk-core`/`openmls`") holds.
//!
//! MDK is synchronous (`&self`, interior mutability). `MarmotService` is
//! therefore sync; an async caller (the future actor) offloads via the
//! runtime's existing blocking bridge — this crate does NOT invent threading.
//!
//! ## Correctness invariants enforced here (mdk-api.md §7)
//!
//! - `merge_pending_commit` is MANDATORY after `create_group`, `add_members`,
//!   `remove_members`, `self_update`. NOT after `leave_group` (SelfRemove —
//!   a peer commits it).
//! - On relay-publish FAILURE of an `evolution_event`, the caller MUST call
//!   `clear_pending_commit` to unblock future group ops. This service returns
//!   a [`PendingGroupChange`] handle whose [`PendingGroupChange::commit`] /
//!   [`PendingGroupChange::clear`] make the success/failure branch
//!   uncircumventable.
//! - Dual-publish KeyPackages: kind:30443 AND legacy kind:443
//!   ([`KeyPackagePublication`] exposes both signed events) through 2026-05-31.
//! - Post-join self-update is mandatory per MIP-02 — call
//!   [`MarmotService::self_update`] after accepting a Welcome.
//!
//! ## Welcome (kind:444) delivery — NIP-59
//!
//! [`wrap_welcome`](MarmotService::wrap_welcome) /
//! [`unwrap_and_process_welcome`](MarmotService::unwrap_and_process_welcome)
//! drive the gift-wrap via `nmp_nip59::{gift_wrap, unwrap_gift_wrap}` (the
//! M11.5 key-boundary seam). The kind:444 rumor → kind:1059 gift-wrap → unwrap
//! → `process_welcome` → `accept_welcome` flow is fully exercised in-crate.
//!
//! `openmls` is NEVER imported directly — only `mdk_core::prelude` re-exports.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use mdk_core::key_packages::KeyPackageEventData;
use mdk_core::prelude::{
    group_types, message_types, welcome_types, GroupId, MessageProcessingResult,
    NostrGroupConfigData, UpdateGroupResult, MDK,
};
use mdk_core::MdkConfig;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Event, EventBuilder, Keys, Kind, PublicKey, RelayUrl, UnsignedEvent};

/// Marmot KeyPackage event kinds (kept local; mirrors `crate::interest`).
const MLS_KEY_PACKAGE_KIND: u16 = 30443;
const MLS_KEY_PACKAGE_KIND_LEGACY: u16 = 443;

/// Errors surfaced by the service. Wraps `mdk_core::Error` (kept opaque as a
/// string so the error type does not leak MLS types across a future FFI
/// boundary) plus service-level validation.
#[derive(Debug)]
pub enum MarmotError {
    /// An underlying MDK / MLS error (stringified to keep MLS types in-crate).
    Mdk(String),
    /// A Nostr event construction / signing error.
    Nostr(String),
    /// A NIP-59 gift-wrap / unwrap error.
    GiftWrap(String),
    /// Service-level invariant violation.
    Invariant(String),
}

impl std::fmt::Display for MarmotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarmotError::Mdk(s) => write!(f, "mdk error: {s}"),
            MarmotError::Nostr(s) => write!(f, "nostr error: {s}"),
            MarmotError::GiftWrap(s) => write!(f, "nip59 error: {s}"),
            MarmotError::Invariant(s) => write!(f, "invariant violation: {s}"),
        }
    }
}
impl std::error::Error for MarmotError {}

impl From<mdk_core::Error> for MarmotError {
    fn from(e: mdk_core::Error) -> Self {
        MarmotError::Mdk(e.to_string())
    }
}
impl From<nmp_nip59::Nip59Error> for MarmotError {
    fn from(e: nmp_nip59::Nip59Error) -> Self {
        MarmotError::GiftWrap(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MarmotError>;

/// The signed Nostr events to publish for one KeyPackage publication.
/// Dual-published (kind:30443 + legacy kind:443) through 2026-05-31. `d_tag`
/// and `hash_ref` are surfaced for the rotation lifecycle (plan §Step 3).
#[derive(Debug)]
pub struct KeyPackagePublication {
    /// Signed kind:30443 event (current spec, NIP-33 addressable).
    pub event_30443: Event,
    /// Signed legacy kind:443 event (dual-publish through 2026-05-31).
    pub event_443: Event,
    /// The `d` tag value — store and reuse on rotation for relay replacement.
    pub d_tag: String,
    /// postcard-serialized `KeyPackageRef` bytes for consumption tracking.
    pub hash_ref: Vec<u8>,
}

/// A group state change that produced an MLS pending commit which MUST be
/// resolved exactly once: [`commit`](Self::commit) on relay-publish success,
/// or [`clear`](Self::clear) on relay-publish failure (mdk-api.md §7.7).
///
/// `evolution_event` is the signed kind:445 event the caller publishes to the
/// group relay. `welcome_rumors` (if any) are kind:444 rumors the caller
/// gift-wraps (NIP-59) and delivers to invitees — use
/// [`MarmotService::wrap_welcome`].
#[must_use = "a PendingGroupChange must be commit()'d on publish-success or clear()'d on failure"]
pub struct PendingGroupChange<'a> {
    service: &'a MarmotService,
    group_id: GroupId,
    /// `true` for SelfRemove (`leave_group`): a peer commits it, so this
    /// handle's `commit()` is a no-op (NO `merge_pending_commit`).
    self_remove: bool,
    resolved: bool,
    pub evolution_event: Event,
    pub welcome_rumors: Vec<UnsignedEvent>,
}

impl<'a> PendingGroupChange<'a> {
    /// Call after the `evolution_event` was successfully published to the
    /// group relay. Performs `merge_pending_commit` (except SelfRemove).
    pub fn commit(mut self) -> Result<()> {
        self.resolved = true;
        if self.self_remove {
            // SelfRemove (leave_group): a peer auto-commits; we do NOT merge.
            return Ok(());
        }
        self.service
            .mdk
            .merge_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// Call if the `evolution_event` failed to publish. Clears the MLS
    /// pending commit so future group ops are not blocked (mdk-api.md §7.7).
    pub fn clear(mut self) -> Result<()> {
        self.resolved = true;
        if self.self_remove {
            // No pending commit was created for SelfRemove.
            return Ok(());
        }
        self.service
            .mdk
            .clear_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// The MLS group id this change applies to (hex).
    pub fn group_id_hex(&self) -> String {
        hex_encode(self.group_id.as_slice())
    }
}

impl<'a> Drop for PendingGroupChange<'a> {
    fn drop(&mut self) {
        // Defensive: if a caller drops the handle without resolving it (e.g.
        // a panic / early return), clear the pending commit so the group is
        // not wedged. A correct caller always commit()'s or clear()'s.
        if !self.resolved && !self.self_remove {
            let _ = self.service.mdk.clear_pending_commit(&self.group_id);
        }
    }
}

/// The Marmot service. Owns an `MDK<MdkSqliteStorage>` (its dedicated SQLite
/// MLS-state file is an implementation detail no other crate sees) plus the
/// local `nostr::Keys` used to sign KeyPackage events, gift-wrap Welcomes,
/// and bind the MLS credential to the Nostr identity (plan §Architecture).
pub struct MarmotService {
    mdk: MDK<MdkSqliteStorage>,
    keys: Keys,
    /// `author_pubkey_hex` → most-recent full signed kind:30443/443 event for
    /// that peer. Populated by the app's raw-event tap when the kernel
    /// delivers a peer's KeyPackage. Any app using Marmot can populate this
    /// cache (the tap is a thin per-app kernel bridge); the protocol logic
    /// (cache lookup in `create_group`/`add_members`) lives here so all
    /// NMP apps get it for free.
    kp_cache: Mutex<HashMap<String, Event>>,
}

impl MarmotService {
    /// Production constructor: encrypted SQLite via the platform keyring.
    /// `db_path` is `<app_support>/marmot-mls-state.sqlite` (owned by this
    /// crate). `service_id` / `db_key_id` are the keyring coordinates.
    pub fn new(
        db_path: impl AsRef<Path>,
        service_id: &str,
        db_key_id: &str,
        keys: Keys,
    ) -> Result<Self> {
        let path = db_path
            .as_ref()
            .to_str()
            .ok_or_else(|| MarmotError::Invariant("non-utf8 db path".into()))?;
        let storage = MdkSqliteStorage::new(path, service_id, db_key_id)
            .map_err(|e| MarmotError::Mdk(e.to_string()))?;
        Ok(Self {
            mdk: MDK::new(storage),
            keys,
            kp_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Construct from an already-built storage backend + a custom MDK config
    /// (e.g. `max_past_epochs`). Used by tests (`new_in_memory`) and advanced
    /// callers.
    pub fn from_storage(storage: MdkSqliteStorage, keys: Keys, config: MdkConfig) -> Self {
        Self {
            mdk: MDK::builder(storage).with_config(config).build(),
            keys,
            kp_cache: Mutex::new(HashMap::new()),
        }
    }

    /// The local identity public key (binds the MLS credential).
    pub fn public_key(&self) -> PublicKey {
        self.keys.public_key()
    }

    // ── KeyPackage cache (populated by the app's raw-event tap) ─────────────

    /// Cache a peer's full signed kind:30443/443 event by author pubkey.
    /// Called by the app's raw-event tap when the kernel delivers a peer's
    /// KeyPackage. Overwrites silently — always keep the newest one received.
    pub fn cache_key_package(&self, event: Event) {
        if let Ok(mut cache) = self.kp_cache.lock() {
            cache.insert(event.pubkey.to_hex(), event);
        }
    }

    /// Retrieve cached full signed events for the given pubkeys. Returns only
    /// the pubkeys whose events are cached. Used by `create_group`/`add_members`
    /// as a fallback when the caller does not supply explicit key-package events.
    pub fn cached_key_packages(&self, pubkeys: &[PublicKey]) -> Vec<Event> {
        let Ok(cache) = self.kp_cache.lock() else {
            return Vec::new();
        };
        pubkeys
            .iter()
            .filter_map(|pk| cache.get(&pk.to_hex()).cloned())
            .collect()
    }

    /// Pubkeys (hex) that have a cached KeyPackage. Surfaced in the snapshot so
    /// native can render pending state while Rust-owned lookup requests settle.
    pub fn cached_kp_pubkeys(&self) -> Vec<String> {
        self.kp_cache
            .lock()
            .map(|cache| cache.keys().cloned().collect())
            .unwrap_or_default()
    }

    // ── KeyPackage (kind:30443 + legacy 443, author-write outbox) ────────────

    /// Generate a fresh MLS KeyPackage and produce the dual-published signed
    /// Nostr events (kind:30443 + legacy kind:443). Caller publishes both via
    /// standard author-write outbox routing (NOT relay-pinned).
    ///
    /// `relays` are advertised in the KeyPackage (the owner's write relays).
    /// On rotation, the returned `d_tag` SHOULD be reused so relays replace
    /// the prior kind:30443 event (mdk-api.md §7.4).
    pub fn publish_key_package(
        &self,
        relays: impl IntoIterator<Item = RelayUrl>,
    ) -> Result<KeyPackagePublication> {
        let KeyPackageEventData {
            content,
            tags_30443,
            tags_443,
            hash_ref,
            d_tag,
        } = self
            .mdk
            .create_key_package_for_event(&self.keys.public_key(), relays)?;

        let event_30443 = EventBuilder::new(Kind::Custom(MLS_KEY_PACKAGE_KIND), content.clone())
            .tags(tags_30443)
            .sign_with_keys(&self.keys)
            .map_err(|e| MarmotError::Nostr(e.to_string()))?;
        let event_443 = EventBuilder::new(Kind::Custom(MLS_KEY_PACKAGE_KIND_LEGACY), content)
            .tags(tags_443)
            .sign_with_keys(&self.keys)
            .map_err(|e| MarmotError::Nostr(e.to_string()))?;

        Ok(KeyPackagePublication {
            event_30443,
            event_443,
            d_tag,
            hash_ref,
        })
    }

    /// Validate a peer's KeyPackage Nostr event (kind:30443 or legacy 443)
    /// parses. MDK parses the embedded KeyPackage internally on
    /// `create_group`/`add_members`; this is a pre-flight sanity check.
    pub fn validate_peer_key_package(&self, event: &Event) -> Result<()> {
        self.mdk
            .parse_key_package(event)
            .map(|_| ())
            .map_err(MarmotError::from)
    }

    // ── Group lifecycle ──────────────────────────────────────────────────────

    /// Create an MLS group inviting the members whose signed KeyPackage events
    /// are supplied. Returns the stored group + a [`CreateGroupPending`]
    /// carrying the kind:444 welcome rumors. The caller gift-wraps + delivers
    /// the welcomes and then `commit()`s; on welcome-publish failure `clear()`s
    /// (mdk-api.md §7.3 / §7.7).
    pub fn create_group(
        &self,
        member_key_package_events: Vec<Event>,
        config: NostrGroupConfigData,
    ) -> Result<(group_types::Group, CreateGroupPending<'_>)> {
        let result =
            self.mdk
                .create_group(&self.keys.public_key(), member_key_package_events, config)?;
        let group_id = result.group.mls_group_id.clone();
        Ok((
            result.group,
            CreateGroupPending {
                service: self,
                group_id,
                resolved: false,
                welcome_rumors: result.welcome_rumors,
            },
        ))
    }

    /// Admin-only. Add members from their signed KeyPackage events. Returns a
    /// [`PendingGroupChange`] with the kind:445 `evolution_event` + kind:444
    /// welcome rumors. Publish the evolution_event to the group relay, deliver
    /// welcomes, then `commit()`; on failure `clear()`.
    pub fn add_members(
        &self,
        group_id: &GroupId,
        key_package_events: &[Event],
    ) -> Result<PendingGroupChange<'_>> {
        let r = self.mdk.add_members(group_id, key_package_events)?;
        Ok(self.pending_from_update(group_id.clone(), r, false))
    }

    /// Admin-only. Remove members by Nostr pubkey. Returns a
    /// [`PendingGroupChange`] (kind:445 commit). Publish then `commit()`;
    /// on failure `clear()`.
    pub fn remove_members(
        &self,
        group_id: &GroupId,
        pubkeys: &[PublicKey],
    ) -> Result<PendingGroupChange<'_>> {
        let r = self.mdk.remove_members(group_id, pubkeys)?;
        Ok(self.pending_from_update(group_id.clone(), r, false))
    }

    /// Rotate this member's MLS leaf keypair (forward secrecy / PCS).
    /// Any member may call this; mandatory post-join per MIP-02. Returns a
    /// [`PendingGroupChange`] (kind:445 commit). Publish then `commit()`.
    pub fn self_update(&self, group_id: &GroupId) -> Result<PendingGroupChange<'_>> {
        let r = self.mdk.self_update(group_id)?;
        Ok(self.pending_from_update(group_id.clone(), r, false))
    }

    /// Leave the group (SelfRemove proposal). Returns a [`PendingGroupChange`]
    /// flagged `self_remove`: a peer auto-commits it, so `commit()` does NOT
    /// call `merge_pending_commit` (mdk-api.md §7.3 / §5.3).
    pub fn leave_group(&self, group_id: &GroupId) -> Result<PendingGroupChange<'_>> {
        let r = self.mdk.leave_group(group_id)?;
        Ok(self.pending_from_update(group_id.clone(), r, true))
    }

    fn pending_from_update(
        &self,
        group_id: GroupId,
        r: UpdateGroupResult,
        self_remove: bool,
    ) -> PendingGroupChange<'_> {
        PendingGroupChange {
            service: self,
            group_id,
            self_remove,
            resolved: false,
            evolution_event: r.evolution_event,
            welcome_rumors: r.welcome_rumors.unwrap_or_default(),
        }
    }

    // ── Welcome (NIP-59 gift-wrap / unwrap + MDK processing) ─────────────────

    /// Gift-wrap a kind:444 Welcome rumor for one invitee (NIP-59 kind:1059).
    /// `receiver` is the invitee's Nostr pubkey. The returned signed kind:1059
    /// event is published to the invitee's NIP-65 inbox relays.
    pub fn wrap_welcome(
        &self,
        receiver: &PublicKey,
        welcome_rumor: UnsignedEvent,
        expiration: Option<nostr::Timestamp>,
    ) -> Result<Event> {
        nmp_nip59::gift_wrap(&self.keys, receiver, welcome_rumor, expiration)
            .map_err(MarmotError::from)
    }

    /// Receiver side: unwrap an incoming kind:1059 gift-wrap, then
    /// `process_welcome` the inner kind:444 rumor. Returns the stored Welcome
    /// (state `Pending`) and the sender pubkey. Call
    /// [`accept_welcome`](Self::accept_welcome) to finalize the join.
    pub fn unwrap_and_process_welcome(
        &self,
        gift_wrap: &Event,
    ) -> Result<(welcome_types::Welcome, PublicKey)> {
        let unwrapped = nmp_nip59::unwrap_gift_wrap(&self.keys, gift_wrap)?;
        let welcome = self
            .mdk
            .process_welcome(&gift_wrap.id, &unwrapped.rumor)
            .map_err(MarmotError::from)?;
        Ok((welcome, unwrapped.sender))
    }

    /// Process an already-unwrapped kind:444 Welcome rumor directly (the
    /// caller performed the NIP-59 unwrap; `wrapper_event_id` is the outer
    /// kind:1059 id). Headless test / actor-bridge entry point.
    pub fn process_welcome(
        &self,
        wrapper_event_id: &nostr::EventId,
        rumor: &UnsignedEvent,
    ) -> Result<welcome_types::Welcome> {
        self.mdk
            .process_welcome(wrapper_event_id, rumor)
            .map_err(MarmotError::from)
    }

    /// Accept a processed Welcome — finalizes the MLS group join. After this
    /// the caller MUST trigger [`self_update`](Self::self_update) (post-join
    /// self-update is mandatory per MIP-02; MDK sets
    /// `SelfUpdateState::Required`).
    pub fn accept_welcome(&self, welcome: &welcome_types::Welcome) -> Result<()> {
        self.mdk.accept_welcome(welcome).map_err(MarmotError::from)
    }

    /// Decline a processed Welcome.
    pub fn decline_welcome(&self, welcome: &welcome_types::Welcome) -> Result<()> {
        self.mdk.decline_welcome(welcome).map_err(MarmotError::from)
    }

    // ── Messages ─────────────────────────────────────────────────────────────

    /// Encrypt a plaintext rumor as an MLS ApplicationMessage. Returns a
    /// signed kind:445 `Event` ready to publish to the group relay (MDK signs
    /// it with the MLS credential key — no extra signing needed).
    pub fn create_message(&self, group_id: &GroupId, rumor: UnsignedEvent) -> Result<Event> {
        self.mdk
            .create_message(group_id, rumor, None)
            .map_err(MarmotError::from)
    }

    /// Process an incoming kind:445 event (application message / commit /
    /// proposal). Returns the MDK processing result enum.
    pub fn process_message(&self, event: &Event) -> Result<MessageProcessingResult> {
        self.mdk.process_message(event).map_err(MarmotError::from)
    }

    // ── Read projections (back the Domain/View modules) ──────────────────────

    /// All groups (any state). Backs `GroupList`.
    pub fn get_groups(&self) -> Result<Vec<group_types::Group>> {
        self.mdk.get_groups().map_err(MarmotError::from)
    }

    /// A single group's display metadata. Backs `MarmotGroup`.
    pub fn get_group(&self, group_id: &GroupId) -> Result<Option<group_types::Group>> {
        self.mdk.get_group(group_id).map_err(MarmotError::from)
    }

    /// The current member set (Nostr pubkeys). Backs `MemberList`.
    pub fn get_members(&self, group_id: &GroupId) -> Result<std::collections::BTreeSet<PublicKey>> {
        self.mdk.get_members(group_id).map_err(MarmotError::from)
    }

    /// MLS leaf-index → pubkey map. Backs `MemberList` leaf indices.
    pub fn group_leaf_map(
        &self,
        group_id: &GroupId,
    ) -> Result<std::collections::BTreeMap<u32, PublicKey>> {
        self.mdk.group_leaf_map(group_id).map_err(MarmotError::from)
    }

    /// Decrypted message history (unpaginated). Backs `GroupMessages`.
    pub fn get_messages(&self, group_id: &GroupId) -> Result<Vec<message_types::Message>> {
        self.mdk
            .get_messages(group_id, None)
            .map_err(MarmotError::from)
    }

    /// Groups whose self-update (key rotation) is overdue past `threshold_secs`.
    /// Drives the TTL re-publish path (plan §Step 3).
    pub fn groups_needing_self_update(&self, threshold_secs: u64) -> Result<Vec<GroupId>> {
        self.mdk
            .groups_needing_self_update(threshold_secs)
            .map_err(MarmotError::from)
    }
}

/// The pending-commit handle returned by [`MarmotService::create_group`].
/// `create_group` produces no evolution_event, so this is a distinct type
/// from [`PendingGroupChange`] (which carries one) but enforces the same
/// commit/clear discipline.
#[must_use = "a CreateGroupPending must be commit()'d on welcome-publish success or clear()'d on failure"]
pub struct CreateGroupPending<'a> {
    service: &'a MarmotService,
    group_id: GroupId,
    resolved: bool,
    pub welcome_rumors: Vec<UnsignedEvent>,
}

impl<'a> CreateGroupPending<'a> {
    /// Call after the kind:444 welcome rumors were delivered. Performs the
    /// mandatory `merge_pending_commit` (mdk-api.md §7.3).
    pub fn commit(mut self) -> Result<()> {
        self.resolved = true;
        self.service
            .mdk
            .merge_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// Call if welcome delivery failed; clears the pending commit.
    pub fn clear(mut self) -> Result<()> {
        self.resolved = true;
        self.service
            .mdk
            .clear_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// The created group's MLS id.
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    /// The created group's MLS id, hex-encoded.
    pub fn group_id_hex(&self) -> String {
        hex_encode(self.group_id.as_slice())
    }
}

impl<'a> Drop for CreateGroupPending<'a> {
    fn drop(&mut self) {
        if !self.resolved {
            let _ = self.service.mdk.clear_pending_commit(&self.group_id);
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
