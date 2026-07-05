//! `MarmotProjection` — the per-app Marmot state.
//!
//! Owns one [`MarmotService`] (the typed MDK translation layer) plus the
//! projection-local bookkeeping `MarmotService` does not itself surface:
//!
//! * a cache of pending Welcomes keyed by kind:1059 gift-wrap event-id hex.
//!   We store the **gift-wrap `nostr::Event`** (NOT any MLS type, so the
//!   "nmp-marmot is the sole importer of mdk-core/openmls" boundary holds);
//!   `process_welcome` is idempotent, so accept/decline lazily re-runs
//!   `unwrap_and_process_welcome` to recover the `&Welcome` without naming an
//!   MLS type.
//! * the local key-package publication timestamp + `d` tag (snapshot
//!   `age_secs` / `stale`).
//! * a `group_id_hex → Vec<RelayUrl>` cache of each group's relay-pinned
//!   relay list (kind:445 commits/messages MUST go to the group relay, not
//!   the author outbox). `mdk-core` does not surface the list, so we cache it
//!   where it IS observable: the `create_group` envelope (`relays`) and the
//!   `Welcome::group_relays` set recovered on accept/gift-wrap ingest. A MISS
//!   suppresses relay dispatch for relay-pinned event kinds rather than falling
//!   back to an author outbox.
//! * the deferred-op store + last-op-error banner — see
//!   [`crate::projection::deferred`].
//! ## Relay seams — both CLOSED
//! * **Outbound (publish).** Dispatch ops publish signed events internally via
//!   [`crate::projection::publish`] through the actor/protocol
//!   runtime port. The op result still carries the signed event JSON but it is
//!   INFORMATIONAL — publish already happened (fire-and-forget).
//! * **Inbound (receive).** [`crate::projection::tap::MarmotIngestParser`] drives
//!   accepted kind:445 / kind:1059 events through
//!   `ops::ingest_signed_event_core`; received Welcomes / messages surface
//!   in the next `snapshot` automatically (seam 2 below has the detail).
//!
//! ## Threading
//!
//! MDK is synchronous; `MarmotService` is sync and this projection invents
//! no threading. It IS accessed from two threads — the kernel actor thread
//! (`ObservedProjectionSink` fan-out + the ingest parser) and host
//! entry points (`snapshot` / dispatch) — so the inner `Mutex` is
//! load-bearing for that concurrent access, not a belt-and-braces extra.
//!
//! ## Seams
//!
//! 1. **Credential seam.** `MarmotService::new` needs `nostr::Keys`.
//!    `nmp_marmot::install` receives a [`crate::MarmotLocalCredentialSlot`]
//!    wrapper and `nmp-marmot` is the only crate that reads/parses the MLS
//!    nsec slot. The raw key does not cross `AppHost` or native binding APIs.
//! 2. **Lossy-observer seam resolved.** The
//!    `ObservedProjectionSink` fan-out carries no signature, so
//!    `on_kernel_event` uses it for *metadata* only. Actual MLS ingest of
//!    kind:445 / kind:1059 is driven by
//!    [`crate::projection::tap::MarmotIngestParser`] (slot `"nmp.marmot"`,
//!    TAP_KINDS `[444, 445, 1059, 30443]`), which reconstructs the
//!    signed `nostr::Event` from [`nmp_store::VerifiedEvent::raw`]
//!    and drives `ops::ingest_signed_event_core`.
//! 3. **KeyPackage cache seam — deferred completion (see
//!    [`crate::projection::deferred`]).** `create_group` / `invite` need
//!    the invitees' signed kind:30443 key packages. When one is missing
//!    the op is PARKED (not terminally failed): the KP fetch fires and the
//!    op re-runs on KP arrival under the original `correlation_id`. Parked ops
//!    expire against actor/kernel-authored time (60 s).
//!    Callers may still pass an explicit `signed_key_package_events_json`
//!    array to bypass the cache entirely.

use std::collections::HashMap;
use std::sync::Mutex;

use mdk_core::prelude::group_types::GroupState;
use nmp_core::actor::{ActorCommand, CommandSender};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nostr::{Event, JsonUtil, RelayUrl};

use crate::interest::KIND_MARMOT_KEY_PACKAGE;
use crate::projection::payload::{
    KeyPackageStatus, LastOpError, MarmotGroupRow, MarmotInitError, MarmotSnapshot,
    PendingWelcomeRow,
};
use crate::projection::pending::PendingOpsStore;
use crate::service::MarmotService;

#[path = "state/handle.rs"]
mod handle;
#[path = "state/messages.rs"]
mod messages;
pub use handle::InnerHandle;

/// Narrow actor/protocol port Marmot needs while executing stateful writes.
///
/// The production implementation is backed by `ProtocolCommandContext`; the
/// stored actor sender is used for later ingest/deferred completions when no
/// command context is on the stack. No raw `NmpApp` handle crosses into the
/// projection.
pub(crate) trait MarmotRuntimePort {
    /// Publish an already-signed event to an explicit relay set under the
    /// caller-supplied `route_class` — an HONEST provenance claim (see
    /// `nmp_core::publish::PublishRouteClass`). D10 rejects any private
    /// kind (kind:1059 gift-wrap) whose target is not classified
    /// `VerifiedPrivateInbox`; callers must earn that classification (e.g.
    /// via [`Self::dm_inbox_relays`]) before claiming it — never pass it as
    /// a bypass.
    fn publish_signed_explicit(
        &self,
        event: &nostr::Event,
        relays: &[RelayUrl],
        route_class: nmp_core::publish::PublishRouteClass,
    );
    fn ensure_interest(
        &self,
        identity: nmp_core::subs::SubIdentity,
        interest: nmp_planner::LogicalInterest,
    );
    fn write_relay_urls(&self, author_hex: &str, kind: u32) -> Vec<String>;
    fn send_actor_command(&self, cmd: ActorCommand);
    /// Resolve a peer's kind:10050 DM-inbox relay list (the same lookup
    /// `nmp-nip17`'s DM-send path uses to earn `VerifiedPrivateInbox`
    /// provenance). `None`/empty means "not yet resolved" — callers must
    /// fail closed rather than approximate with an unrelated relay set.
    fn dm_inbox_relays(&self, pubkey_hex: &str) -> Option<Vec<String>>;
}

/// 7-day key-package rotation threshold (snapshot `stale`).
const KEY_PACKAGE_STALE_SECS: u64 = 7 * 24 * 60 * 60;

/// A cached pending Welcome. We keep the **gift-wrap `nostr::Event`** (not
/// any MLS type) so `accept`/`decline` can lazily re-derive the `&Welcome`
/// via the idempotent `unwrap_and_process_welcome`, plus the display
/// strings the snapshot renders.
struct CachedWelcome {
    gift_wrap: Event,
    group_name: String,
    inviter_npub: String,
}

pub(super) struct Inner {
    service: MarmotService,
    /// kind:1059 gift-wrap-event-id hex → cached pending Welcome.
    pending_welcomes: HashMap<String, CachedWelcome>,
    /// Wall-clock secs of the most recent `publish_key_package` dispatch.
    key_package_published_at: Option<u64>,
    /// `d` tag of the most recent key-package publication.
    key_package_d_tag: Option<String>,
    /// `group_id_hex` → the group's configured (relay-pinned) relay list,
    /// seeded from the `create_group` envelope + `Welcome::group_relays`.
    /// A MISS → explicit publish fails closed (documented limitation).
    group_relays: HashMap<String, Vec<RelayUrl>>,
    /// Actor command sender for ingest/deferred follow-up work that happens
    /// after the original `ProtocolCommandContext` has returned.
    actor_sender: Option<CommandSender>,
    /// #3057 round-6: live view of the kernel's kind:10050 DM-inbox lookup,
    /// used to resolve invitee DM relays when running WITHOUT a port (the
    /// ingest-thread Welcome-publish retry). `None` in bare test projections.
    dm_inbox_lookup: Option<std::sync::Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>,
    /// #1651: the service-init failure surfaced in every snapshot, or `None`
    /// on a healthy registration. `Some(KeyringUnavailable)` when the projection
    /// was built over the in-memory mock credential store (formerly the V-62
    /// `keyring_unavailable` bool). Set once at construction; never cleared.
    /// `DbKeyLost` is retained for host registration layers that need to
    /// surface a lost MLS database key.
    init_error: Option<MarmotInitError>,
    /// Pending ops deferred because invitee KPs were not yet in the cache.
    /// Re-tried on every KP ingest; expired via wall-clock gate (D8).
    pub(super) pending_ops: PendingOpsStore,
    /// The most recent terminal op FAILURE (deferred-op expiry or a failed
    /// retry), or `None` when no op has failed or the last failure was
    /// superseded by a later success. Surfaced verbatim in the snapshot
    /// (`MarmotSnapshot::last_op_error`) so a host can show a one-line error
    /// banner without subscribing to the action-status stream. Set by
    /// `record_last_op_failure`, cleared by `clear_last_op_error` on the next
    /// successful op.
    pub(super) last_op_error: Option<LastOpError>,
    /// Test-only capture of every terminal verdict routed through
    /// [`InnerHandle::push_actor_command`], as `(verdict, correlation_id)`
    /// where `verdict` is `"success"` or `"failure"`. In production the
    /// deferred verdict goes to the live actor channel (which needs a full
    /// `NmpApp`); `ActorCommand` is not `Clone`, so this buffer records a
    /// lightweight projection that still lets unit tests assert the EXACT
    /// command stream — one terminal per correlation_id — without standing
    /// up the actor.
    #[cfg(test)]
    pub(super) captured_commands: Vec<(&'static str, String)>,
}

/// Owned Marmot projection. `Mutex` because ingest and host snapshot/action
/// reads can reach the same projection from different runtime call paths.
pub struct MarmotProjection {
    // `pub(super)` so the sibling `resubscribe` module can lock it directly.
    pub(super) inner: Mutex<Inner>,
}

impl MarmotProjection {
    /// Build the projection around an already-constructed [`MarmotService`].
    /// The host registration layer owns service construction (it must parse
    /// the signer seam key + resolve the app-support DB path) so this stays
    /// infallible. `actor_sender` starts absent; hosts install it after
    /// constructing the projection. Tests that build the projection directly
    /// leave it absent, so publish/interest side effects are no-ops while
    /// state changes remain testable.
    ///
    /// `init_error` is `Some(MarmotInitError::KeyringUnavailable)` when the
    /// service was initialized with the in-memory mock credential store
    /// (formerly V-62 `keyring_unavailable = true`), else `None`. It is
    /// surfaced in every subsequent snapshot so the host can warn the user.
    #[must_use]
    pub fn new(service: MarmotService, init_error: Option<MarmotInitError>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                service,
                pending_welcomes: HashMap::new(),
                key_package_published_at: None,
                key_package_d_tag: None,
                group_relays: HashMap::new(),
                actor_sender: None,
                dm_inbox_lookup: None,
                init_error,
                pending_ops: PendingOpsStore::new(),
                last_op_error: None,
                #[cfg(test)]
                captured_commands: Vec::new(),
            }),
        }
    }

    /// Record the actor sender used for deferred completions and ingest-time
    /// interest/publish effects. D6 — poisoned mutex silently no-ops.
    pub fn set_actor_sender(&self, actor_sender: CommandSender) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.actor_sender = Some(actor_sender);
        }
    }

    /// Record the live DM-inbox lookup so port-less (ingest-thread) op retries
    /// can resolve invitee DM relays (#3057 round-6). D6 — poisoned mutex
    /// silently no-ops.
    pub fn set_dm_inbox_lookup(
        &self,
        lookup: std::sync::Arc<dyn nmp_core::substrate::DmInboxRelayLookup>,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.dm_inbox_lookup = Some(lookup);
        }
    }

    /// Borrow the inner state under the lock. Returns `None` on a poisoned
    /// mutex (D6 — caller degrades to an empty/error result).
    #[must_use]
    pub fn with_inner<R>(&self, f: impl FnOnce(&mut InnerHandle<'_>) -> R) -> Option<R> {
        let mut guard = self.inner.lock().ok()?;
        let mut h = InnerHandle {
            inner: &mut guard,
            port: None,
        };
        Some(f(&mut h))
    }

    /// Borrow the inner state with a live runtime port. Used by
    /// `MarmotProtocolCommand` so writes publish/subscribe through the actor
    /// protocol context instead of a raw host pointer.
    #[must_use]
    pub(crate) fn with_inner_port<R>(
        &self,
        port: &dyn MarmotRuntimePort,
        f: impl FnOnce(&mut InnerHandle<'_>) -> R,
    ) -> Option<R> {
        let mut guard = self.inner.lock().ok()?;
        let mut h = InnerHandle {
            inner: &mut guard,
            port: Some(port),
        };
        Some(f(&mut h))
    }

    /// Registry identities for all currently cached group-message interests.
    ///
    /// Used by the active-identity runtime when a Marmot account is deactivated
    /// or replaced. The projection owns the group-relay cache, so it also owns
    /// reconstructing the exact interest identities to withdraw.
    #[must_use]
    pub(crate) fn group_message_identities(&self) -> Vec<nmp_core::subs::SubIdentity> {
        let Ok(guard) = self.inner.lock() else {
            return Vec::new();
        };
        guard
            .group_relays
            .iter()
            .flat_map(|(group_id_hex, relays)| {
                relays.iter().map(move |relay| {
                    crate::interest::group_message_identity(group_id_hex, &relay.to_string())
                })
            })
            .collect()
    }

    /// Build the JSON snapshot. D6 — poisoned mutex → empty snapshot.
    #[must_use]
    pub fn snapshot(&self, now_secs: u64) -> MarmotSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return MarmotSnapshot::empty();
        };

        let groups: Vec<MarmotGroupRow> = match inner.service.get_groups() {
            Ok(gs) => gs
                .into_iter()
                .filter(|g| g.state == GroupState::Active)
                .map(|g| {
                    let id_hex = hex_encode(g.mls_group_id.as_slice());
                    let members = inner
                        .service
                        .get_members(&g.mls_group_id)
                        .map(|set| set.into_iter().map(|pk| pk.to_hex()).collect::<Vec<_>>())
                        .unwrap_or_default();
                    // Unread seam: no read-cursor — total app-message count.
                    let unread = inner
                        .service
                        .get_messages(&g.mls_group_id)
                        .map(|m| m.len() as u32)
                        .unwrap_or(0);
                    let unread_count = if unread == 0 { None } else { Some(unread) };
                    let member_count = u32::try_from(members.len()).unwrap_or(u32::MAX);
                    MarmotGroupRow {
                        id_hex,
                        name: g.name.clone(),
                        members,
                        member_count,
                        unread_count,
                        last_msg_at: g.last_message_at.map(|t| t.as_secs()),
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        let pending_welcomes: Vec<PendingWelcomeRow> = inner
            .pending_welcomes
            .iter()
            .map(|(id_hex, c)| PendingWelcomeRow {
                id_hex: id_hex.clone(),
                group_name: c.group_name.clone(),
                inviter_npub: c.inviter_npub.clone(),
            })
            .collect();

        // Reaching this snapshot path means a host has built a live Marmot
        // projection, so the identity is registered. Hosts without Marmot state
        // use `MarmotSnapshot::empty()`.
        let key_package = match inner.key_package_published_at {
            Some(ts) => {
                let age = now_secs.saturating_sub(ts);
                KeyPackageStatus {
                    published: true,
                    d_tag: inner.key_package_d_tag.clone(),
                    age_secs: Some(age),
                    stale: age > KEY_PACKAGE_STALE_SECS,
                    is_registered: true,
                }
            }
            None => KeyPackageStatus {
                is_registered: true,
                ..Default::default()
            },
        };

        let cached_kp_pubkeys = inner.service.cached_kp_pubkeys();
        let orphaned_commit_count = inner.service.orphaned_commit_count();
        let init_error = inner.init_error.clone();
        // Deferred-op snapshot rows + the last-op-error banner are built by the
        // `deferred` sub-module (the owner of all pending-op shape decisions).
        let pending_ops = super::deferred::pending_op_rows(&inner.pending_ops, now_secs);
        let last_op_error = inner.last_op_error.clone();
        MarmotSnapshot {
            groups,
            pending_welcomes,
            key_package,
            cached_kp_pubkeys,
            is_registered: true,
            orphaned_commit_count,
            init_error,
            pending_ops,
            last_op_error,
        }
    }
}

impl ObservedProjectionSink for MarmotProjection {
    /// Metadata-only `KernelEvent` observer (see module rustdoc): a
    /// [`KernelEvent`] has no signature so we cannot feed kind:445 /
    /// kind:1059 into MDK from here — that is now done automatically by
    /// the Marmot ingest parser ([`crate::projection::tap`]). This
    /// observer only tracks metadata: if the local identity has published
    /// a key-package and the kernel re-ingests it (e.g. relay echo), keep
    /// the `published` flag warm so the snapshot reflects reality even
    /// before a `publish_key_package` dispatch this session.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_MARMOT_KEY_PACKAGE {
            // kind:445 / kind:1059 require a signed event — driven by the
            // Marmot ingest parser (`crate::projection::tap`), not here.
            // Legacy kind:443 was retired 2026-05-31 and is no longer tracked.
            return;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return; // D6 — poisoned mutex silently no-ops.
        };
        let is_local = inner.service.public_key().to_hex() == event.author;
        if !is_local {
            return;
        }
        if inner.key_package_published_at.is_none() {
            inner.key_package_published_at = Some(event.created_at);
            if inner.key_package_d_tag.is_none() {
                if let Some(d) = event
                    .tags
                    .iter()
                    .find(|t| t.first().map(String::as_str) == Some("d"))
                    .and_then(|t| t.get(1))
                {
                    inner.key_package_d_tag = Some(d.clone());
                }
            }
        }
    }
}

/// Parse a signed `nostr::Event` from its JSON wire form (D6: `Err` →
/// caller returns `{"ok":false}`).
#[must_use]
pub(crate) fn parse_signed_event(json: &str) -> Result<Event, String> {
    Event::from_json(json).map_err(|e| format!("invalid signed event json: {e}"))
}

/// Extract the `"op"` tag from a stored action-JSON envelope, or
/// `"unknown"` if it cannot be parsed. Used to label a `LastOpError`.
pub(super) fn op_tag_of(action_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(action_json)
        .ok()
        .and_then(|v| v.get("op").and_then(|s| s.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
