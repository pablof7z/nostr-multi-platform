//! `MarmotService` — the real MDK-driving API layer.
//!
//! This is the only module in `nmp-marmot` that touches MDK types. It is
//! consumed in-crate by round-trip tests and protocol projection code; no
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
//! - KeyPackages are published as kind:30443 only (legacy kind:443 was retired
//!   2026-05-31; [`KeyPackagePublication`] carries only `event_30443`).
//! - Post-join self-update is mandatory per MIP-02 — call
//!   [`MarmotService::self_update`] after accepting a Welcome.
//!
//! ## Welcome (kind:444) delivery — NIP-59
//!
//! [`wrap_welcome`](MarmotService::wrap_welcome) /
//! [`unwrap_and_process_welcome`](MarmotService::unwrap_and_process_welcome)
//! drive the gift-wrap via `nmp_nip59::{gift_wrap_local, unwrap_gift_wrap}` (the
//! M11.5 key-boundary seam — Marmot is local-key-only by construction). The
//! kind:444 rumor → kind:1059 gift-wrap → unwrap → `process_welcome` →
//! `accept_welcome` flow is fully exercised in-crate.
//!
//! `openmls` is NEVER imported directly — only `mdk_core::prelude` re-exports.
//!
//! ## Internal module split (#962)
//!
//! This orchestration file owns the `MarmotService` struct and its `impl`
//! (constructors, KeyPackage cache, group lifecycle, Welcome, messages). The
//! protocol/domain helpers it calls into live in sibling submodules and are
//! re-exported here to keep `crate::service::*` paths stable:
//! - [`error`] — [`MarmotError`] / [`Result`].
//! - [`key_package`] — [`KeyPackagePublication`].
//! - [`pending_commit`] — [`PendingGroupChange`] / [`CreateGroupPending`].
//!
//! Read-projection methods (`get_groups` / `get_group` / `get_members` /
//! `group_relays` / `group_leaf_map` / `get_messages` /
//! `groups_needing_self_update`) live in the sibling `service_reads` module —
//! same `impl MarmotService`, same public API — to keep this file under the
//! size cap.

mod error;
mod key_package;
mod pending_commit;

pub use error::{MarmotError, Result};
pub use key_package::KeyPackagePublication;
pub use pending_commit::{CreateGroupPending, PendingGroupChange};

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use mdk_core::key_packages::KeyPackageEventData;
use mdk_core::prelude::{
    group_types, welcome_types, GroupId, MessageProcessingResult, NostrGroupConfigData,
    UpdateGroupResult, MDK,
};
use mdk_core::MdkConfig;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Event, EventBuilder, Keys, Kind, PublicKey, RelayUrl, UnsignedEvent};
use zeroize::Zeroizing;

// Marmot KeyPackage event kind — canonical `u32` integer from `nmp-kinds`
// (via `crate::interest`). `Kind::Custom` wants a `u16`, so the build site
// casts at the call; the single source of truth is the registry, not a literal.
use crate::interest::KIND_MARMOT_KEY_PACKAGE;

/// The Marmot service. Owns an `MDK<MdkSqliteStorage>` (its dedicated SQLite
/// MLS-state file is an implementation detail no other crate sees) plus the
/// local `nostr::Keys` used to sign KeyPackage events, gift-wrap Welcomes,
/// and bind the MLS credential to the Nostr identity (plan §Architecture).
pub struct MarmotService {
    // `pub(crate)` so the `service_reads` module can drive read projections.
    pub(crate) mdk: MDK<MdkSqliteStorage>,
    keys: Keys,
    /// Redundant `Zeroizing` copy of the raw secret-key bytes, held purely so
    /// that *a* copy of the secret is reliably wiped from the heap on drop.
    ///
    /// PARTIAL MITIGATION — same constraint as `nmp-signers` `LocalKeySigner`:
    /// `nostr::Keys` keeps the secret in its private `secp256k1::SecretKey`
    /// field (no `&mut` accessor) and a cached `Keypair`. `secp256k1` 0.29 has
    /// no `zeroize` feature and `nostr` 0.44 implements neither `Zeroize` nor
    /// `Drop`, so those copies are NOT wiped on drop. This field wipes the one
    /// Rust-owned copy we can reach, reducing (not eliminating) recoverable
    /// secret material in freed memory. Tracked as V-55 in GitHub issue #971.
    _secret_bytes: Zeroizing<[u8; 32]>,
    /// `author_pubkey_hex` → most-recent full signed kind:30443 event for that
    /// peer. Populated by Marmot's ingest parser when the kernel delivers a
    /// peer's KeyPackage. The protocol logic (cache lookup in
    /// `create_group`/`add_members`) lives here so all NMP apps get it for
    /// free.
    kp_cache: Mutex<HashMap<String, Event>>,
    /// Cumulative count of `PendingGroupChange` / `CreateGroupPending` handles
    /// that were dropped without being committed or cleared (V-61). Each
    /// increment means local MLS state may have diverged from the
    /// relay-published epoch for the affected group. The projection reads this
    /// counter and surfaces it in the snapshot so the host can observe the
    /// divergence and decide whether to block further group sends.
    ///
    /// Shared via `Arc` so the `PendingGroupChange` handle (which borrows
    /// `&MarmotService` with a lifetime that cannot outlive the service) can
    /// write to it from `Drop` without needing a mutable borrow.
    pub(crate) orphaned_commit_count: Arc<AtomicU32>,
}

impl MarmotService {
    /// Production constructor: encrypted SQLite via the platform keyring.
    /// `db_path` is `<app_support>/marmot-mls-state.sqlite` (owned by this
    /// crate). `service_id` / `db_key_id` are the keyring coordinates.
    #[must_use]
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
            _secret_bytes: Zeroizing::new(keys.secret_key().to_secret_bytes()),
            keys,
            kp_cache: Mutex::new(HashMap::new()),
            orphaned_commit_count: Arc::new(AtomicU32::new(0)),
        })
    }

    /// Construct from an already-built storage backend + a custom MDK config
    /// (e.g. `max_past_epochs`). Used by tests (`new_in_memory`) and advanced
    /// callers.
    #[must_use]
    pub fn from_storage(storage: MdkSqliteStorage, keys: Keys, config: MdkConfig) -> Self {
        Self {
            mdk: MDK::builder(storage).with_config(config).build(),
            _secret_bytes: Zeroizing::new(keys.secret_key().to_secret_bytes()),
            keys,
            kp_cache: Mutex::new(HashMap::new()),
            orphaned_commit_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// The local identity public key (binds the MLS credential).
    pub fn public_key(&self) -> PublicKey {
        self.keys.public_key()
    }

    /// Cumulative count of `PendingGroupChange` / `CreateGroupPending` handles
    /// that were dropped without commit/clear this session (V-61 diagnostic).
    ///
    /// A non-zero value means local MLS state may have diverged from the
    /// relay-published epoch for one or more groups. The host should block
    /// further group sends and surface a recovery prompt to the user.
    #[must_use]
    pub fn orphaned_commit_count(&self) -> u32 {
        self.orphaned_commit_count.load(Ordering::Relaxed)
    }

    // ── KeyPackage cache (populated by Marmot's ingest parser) ─────────────

    /// Cache a peer's full signed kind:30443 event by author pubkey. Called by
    /// Marmot's ingest parser when the kernel delivers a peer's KeyPackage.
    /// Overwrites silently — always keep the newest one received.
    pub fn cache_key_package(&self, event: Event) {
        if let Ok(mut cache) = self.kp_cache.lock() {
            cache.insert(event.pubkey.to_hex(), event);
        }
    }

    /// Retrieve cached full signed events for the given pubkeys. Returns only
    /// the pubkeys whose events are cached. Used by `create_group`/`add_members`
    /// as a fallback when the caller does not supply explicit key-package events.
    #[must_use]
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
    #[must_use]
    pub fn cached_kp_pubkeys(&self) -> Vec<String> {
        self.kp_cache
            .lock()
            .map(|cache| cache.keys().cloned().collect())
            .unwrap_or_default()
    }

    // ── KeyPackage (kind:30443, author-write outbox) ────────────────────────

    /// Generate a fresh MLS KeyPackage and produce a signed kind:30443 Nostr
    /// event. Caller publishes via standard author-write outbox routing (NOT
    /// relay-pinned).
    ///
    /// `relays` are advertised in the KeyPackage (the owner's write relays).
    /// On rotation, the returned `d_tag` SHOULD be reused so relays replace
    /// the prior kind:30443 event (mdk-api.md §7.4).
    ///
    /// Only kind:30443 is published (legacy kind:443 was retired 2026-05-31).
    pub fn publish_key_package(
        &self,
        relays: impl IntoIterator<Item = RelayUrl>,
    ) -> Result<KeyPackagePublication> {
        let KeyPackageEventData {
            content,
            tags_30443,
            hash_ref,
            d_tag,
            // tags_443 is provided by mdk_core but no longer used — legacy
            // kind:443 dual-publish was retired 2026-05-31.
            ..
        } = self
            .mdk
            .create_key_package_for_event(&self.keys.public_key(), relays)?;

        let event_30443 = EventBuilder::new(Kind::Custom(KIND_MARMOT_KEY_PACKAGE as u16), content)
            .tags(tags_30443)
            .sign_with_keys(&self.keys)
            .map_err(|e| MarmotError::Nostr(e.to_string()))?;

        Ok(KeyPackagePublication {
            event_30443,
            d_tag,
            hash_ref,
        })
    }

    /// Validate a peer's kind:30443 KeyPackage Nostr event parses. MDK parses
    /// the embedded KeyPackage internally on `create_group`/`add_members`; this
    /// is a pre-flight sanity check.
    #[must_use]
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
            CreateGroupPending::new(
                self,
                group_id,
                Arc::clone(&self.orphaned_commit_count),
                result.welcome_rumors,
            ),
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
    #[must_use]
    pub fn self_update(&self, group_id: &GroupId) -> Result<PendingGroupChange<'_>> {
        let r = self.mdk.self_update(group_id)?;
        Ok(self.pending_from_update(group_id.clone(), r, false))
    }

    /// Leave the group (SelfRemove proposal). Returns a [`PendingGroupChange`]
    /// flagged `self_remove`: a peer auto-commits it, so `commit()` does NOT
    /// call `merge_pending_commit` (mdk-api.md §7.3 / §5.3).
    #[must_use]
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
        PendingGroupChange::new(
            self,
            group_id,
            self_remove,
            Arc::clone(&self.orphaned_commit_count),
            r.evolution_event,
            r.welcome_rumors.unwrap_or_default(),
        )
    }

    // ── Welcome (NIP-59 gift-wrap / unwrap + MDK processing) ─────────────────

    /// Gift-wrap a kind:444 Welcome rumor for one invitee (NIP-59 kind:1059).
    /// `receiver` is the invitee's Nostr pubkey. The returned signed kind:1059
    /// event is published to the invitee's NIP-65 inbox relays.
    ///
    /// Marmot is local-key-only by construction (it holds its own MLS identity
    /// `Keys`), so this seals + wraps synchronously through the pure
    /// `nmp_nip59::gift_wrap_local` composition (ADR-0072 §D5 — the
    /// `SignerForSeal` trait + driver-thread execution model is gone). No port,
    /// no thread, no `SignerOp`.
    pub fn wrap_welcome(
        &self,
        receiver: &PublicKey,
        welcome_rumor: UnsignedEvent,
    ) -> Result<Event> {
        let tweaked = nostr::Timestamp::tweaked(nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK);
        nmp_nip59::gift_wrap_local(&self.keys, receiver, &welcome_rumor, tweaked)
            .map_err(|e| MarmotError::GiftWrap(e.to_string()))
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

    /// Receiver side, unwrap ONLY: NIP-59-unwrap an incoming kind:1059
    /// gift-wrap and return the inner rumor + sender WITHOUT running MDK's
    /// `process_welcome`.
    ///
    /// The ingest chokepoint uses this to triage a kind:1059 before committing
    /// to Welcome processing: a gift-wrap not addressed to us (or that fails to
    /// decrypt) is not ours, and a gift-wrap whose inner rumor is NOT a
    /// [`Kind::MlsWelcome`] (444) is another protocol's envelope (e.g. a NIP-17
    /// DM sharing kind:1059) — neither is a Marmot ingest ERROR. Only once the
    /// rumor is confirmed to be a kind:444 Welcome is a subsequent
    /// `process_welcome` failure a genuine, surface-worthy Marmot failure
    /// (#3057). Returns the [`nmp_nip59::UnwrappedGift`] `(sender, rumor)`.
    pub fn unwrap_giftwrap(&self, gift_wrap: &Event) -> Result<nmp_nip59::UnwrappedGift> {
        nmp_nip59::unwrap_gift_wrap(&self.keys, gift_wrap).map_err(MarmotError::from)
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
    #[must_use]
    pub fn accept_welcome(&self, welcome: &welcome_types::Welcome) -> Result<()> {
        self.mdk.accept_welcome(welcome).map_err(MarmotError::from)
    }

    /// Decline a processed Welcome.
    #[must_use]
    pub fn decline_welcome(&self, welcome: &welcome_types::Welcome) -> Result<()> {
        self.mdk.decline_welcome(welcome).map_err(MarmotError::from)
    }

    // ── Messages ─────────────────────────────────────────────────────────────

    /// Encrypt a plaintext rumor as an MLS ApplicationMessage. Returns a
    /// signed kind:445 `Event` ready to publish to the group relay (MDK signs
    /// it with the MLS credential key — no extra signing needed).
    #[must_use]
    pub fn create_message(&self, group_id: &GroupId, rumor: UnsignedEvent) -> Result<Event> {
        self.mdk
            .create_message(group_id, rumor, None)
            .map_err(MarmotError::from)
    }

    /// Process an incoming kind:445 event (application message / commit /
    /// proposal). Returns the MDK processing result enum.
    #[must_use]
    pub fn process_message(&self, event: &Event) -> Result<MessageProcessingResult> {
        self.mdk.process_message(event).map_err(MarmotError::from)
    }

    // ── Read projections (back the Domain/View modules) ──────────────────────
    // The read-projection methods (get_groups / get_group / get_members /
    // group_relays / group_leaf_map / get_messages / groups_needing_self_update)
    // live in the sibling `service_reads` module — same `impl MarmotService`,
    // same public API — to keep this file under the size cap.
}
