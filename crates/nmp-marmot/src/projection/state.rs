//! `MarmotProjection` — the per-app Marmot state.
//!
//! Owns one [`MarmotService`] (the typed MDK translation layer) plus the
//! FFI-local bookkeeping `MarmotService` does not itself surface:
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
//!   degrades the publish to author-outbox `Auto` (documented limitation).
//! * the deferred-op store + last-op-error banner — see
//!   [`crate::projection::deferred`].
//!
//! ## Relay seams — both CLOSED
//!
//! * **Outbound (publish).** Dispatch ops publish their signed events
//!   INTERNALLY via [`crate::projection::publish`]
//!   (`nmp_ffi::NmpApp::publish_signed_explicit` against the retained
//!   `&NmpApp`). The op result still carries the signed event JSON but it
//!   is INFORMATIONAL — publish already happened (fire-and-forget).
//! * **Inbound (receive).** [`crate::projection::tap::MarmotIngestParser`]
//!   drives accepted kind:445 / kind:1059 events through
//!   `ops::ingest_signed_event_core`; received Welcomes / messages surface
//!   in the next `snapshot` automatically (seam 2 below has the detail).
//!
//! ## Threading
//!
//! MDK is synchronous; `MarmotService` is sync and this projection invents
//! no threading. It IS accessed from two threads — the kernel actor thread
//! (`KernelEventObserver` fan-out + the ingest parser) and the host FFI
//! entry points (`snapshot` / dispatch) — so the inner `Mutex` is
//! load-bearing for that concurrent access, not a belt-and-braces extra.
//!
//! ## Seams (documented, NOT blocking — see crate task)
//!
//! 1. **Signer seam.** `MarmotService::new` needs `nostr::Keys`; no
//!    kernel-level `Keys` provider exists yet, so the host register path
//!    takes the secret key directly. Replace with a `KeyringCapability`
//!    seam when one lands on `NmpApp`.
//! 2. **Lossy-observer seam — RESOLVED (inbound ingest CLOSED).** The
//!    `KernelEventObserver` fan-out carries no signature, so
//!    `on_kernel_event` uses it for *metadata* only. Actual MLS ingest of
//!    kind:445 / kind:1059 is driven by
//!    [`crate::projection::tap::MarmotIngestParser`] (slot `"marmot"`,
//!    TAP_KINDS `[444, 445, 1059, 30443]`), which reconstructs the
//!    signed `nostr::Event` from [`nmp_store::VerifiedEvent::raw`]
//!    and drives `ops::ingest_signed_event_core`.
//! 3. **KeyPackage cache seam — deferred completion (see
//!    [`crate::projection::deferred`]).** `create_group` / `invite` need
//!    the invitees' signed kind:30443 key packages. When one is missing
//!    the op is PARKED (not terminally failed): the KP fetch fires and the
//!    op re-runs on KP arrival, recording its verdict under the original
//!    `correlation_id`. Parked ops expire on a wall-clock edge (60 s).
//!    Callers may still pass an explicit `signed_key_package_events_json`
//!    array to bypass the cache entirely.

use std::collections::HashMap;
use std::sync::Mutex;

use mdk_core::prelude::group_types::GroupState;
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_ffi::NmpApp;
use nostr::{Event, JsonUtil, PublicKey, RelayUrl};

use crate::projection::payload::{
    KeyPackageStatus, LastOpError, MarmotGroupRow, MarmotInitError, MarmotSnapshot,
    PendingWelcomeRow,
};
use crate::projection::pending::PendingOpsStore;
use crate::service::MarmotService;

// Marmot KeyPackage kinds — canonical `u32` integers from `nmp-kinds` (via
// `crate::interest`). Previously re-declared as a local literal here, diverging
// in type from the `service.rs` copy (u16) — #1493 fragmentation finding.
use crate::interest::KIND_MARMOT_KEY_PACKAGE;

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
    /// The live `*mut NmpApp` the owning host Marmot handle retains. `null`
    /// for the in-memory test projection (publish degrades to a silent
    /// no-op there — the D6 fire-and-forget contract).
    app: *mut NmpApp,
    /// #1651: the service-init failure surfaced in every snapshot, or `None`
    /// on a healthy registration. `Some(KeyringUnavailable)` when the projection
    /// was built over the in-memory mock credential store (formerly the V-62
    /// `keyring_unavailable` bool). Set once at construction; never cleared.
    /// (The `DbKeyLost` variant is carried by the degraded slot in `ffi.rs`,
    /// which has no `MarmotProjection` to hold it — see `MarmotSlotState`.)
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

/// Owned Marmot projection. `Mutex` because `on_kernel_event` takes `&self`
/// on the actor thread while the FFI snapshot / dispatch run on the Swift
/// bridge thread (low contention; the bridge serializes its calls).
pub struct MarmotProjection {
    // `pub(super)` so the sibling `resubscribe` module can lock it directly.
    pub(super) inner: Mutex<Inner>,
}

// SAFETY: REQUIRED — `register_event_observer` casts
// `Arc<MarmotProjection>` to `Arc<dyn KernelEventObserver>` (bounded
// `Send + Sync`). The auto-derived `!Send`/`!Sync` comes only from
// `Inner::app: *mut NmpApp`; every other field is already `Send + Sync`.
// Soundness:
//   * All cross-thread state access (kernel actor thread via
//     `on_kernel_event` / ingest parser; Swift `@MainActor` via FFI ops)
//     goes through the inner `Mutex<Inner>`. The `unsafe impl` only asserts
//     the `*mut NmpApp` field does not invalidate that.
//   * `app` is only ever READ (to forward fire-and-forget commands), never
//     mutated, and cannot dangle: `nmp_app_free` (`NmpApp::Drop`) sends
//     `Shutdown` + `join()`s the actor thread before freeing, and every
//     reader runs INLINE on that thread — the join fences in-flight access.
// CALLER CONTRACT (upheld by `nmp-app-chirp`): `nmp_app_free` must not run
// while a kernel callback reaching this projection is still executing; the
// in-process Rust-trait registration path provides that fence via the join.
unsafe impl Send for MarmotProjection {}
unsafe impl Sync for MarmotProjection {}

impl MarmotProjection {
    /// Build the projection around an already-constructed [`MarmotService`].
    /// The FFI layer owns service construction (it must parse the signer
    /// seam key + resolve the app-support DB path) so this stays infallible.
    /// `app` starts `null`; the FFI `register` path calls
    /// [`MarmotProjection::set_app`] with the retained pointer. Tests that
    /// build the projection directly leave it `null` → publish no-ops.
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
                app: std::ptr::null_mut(),
                init_error,
                pending_ops: PendingOpsStore::new(),
                last_op_error: None,
                #[cfg(test)]
                captured_commands: Vec::new(),
            }),
        }
    }

    /// Record the live `*mut NmpApp` so the dispatch ops can publish
    /// internally. Called once by the host shell's Marmot register path
    /// with the same pointer the handle retains for its lifetime. D6 —
    /// poisoned mutex silently no-ops (publish then degrades to no-op).
    pub fn set_app(&self, app: *mut NmpApp) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.app = app;
        }
    }

    /// Borrow the inner state under the lock for an FFI op. Returns `None`
    /// on a poisoned mutex (D6 — caller degrades to null / `{"ok":false}`).
    #[must_use]
    pub fn with_inner<R>(&self, f: impl FnOnce(&mut InnerHandle<'_>) -> R) -> Option<R> {
        let mut guard = self.inner.lock().ok()?;
        let mut h = InnerHandle { inner: &mut guard };
        Some(f(&mut h))
    }

    /// Build the all-groups messages map for the `"nmp.marmot.messages"` push
    /// projection (ADR-0039, V-107 Rust leg).
    ///
    /// Returns a `serde_json::Value::Object` keyed by `group_id_hex` →
    /// newest-N [`crate::projection::payload::MarmotMessageRow`] JSON array for
    /// every joined group. Bounded by `page` rows per group (typically
    /// `DEFAULT_MESSAGE_PAGE` = 200).
    ///
    /// Reads from the MDK SQLite message store directly — already-decrypted rows,
    /// no re-decrypt per tick. D8 compliant: cheap, non-blocking.
    /// D6: poisoned mutex → empty JSON object.
    #[must_use]
    pub fn messages_all_groups_json(&self, page: usize) -> serde_json::Value {
        self.with_inner(|h| {
            let group_ids: Vec<String> = h
                .service()
                .get_groups()
                .map(|gs| {
                    gs.into_iter()
                        .filter(|g| g.state == GroupState::Active)
                        .map(|g| hex_encode(g.mls_group_id.as_slice()))
                        .collect()
                })
                .unwrap_or_default();
            let mut map = serde_json::Map::with_capacity(group_ids.len());
            for gid_hex in group_ids {
                let rows = crate::projection::ops::group_messages(h, &gid_hex, page);
                map.insert(
                    gid_hex,
                    serde_json::to_value(rows).unwrap_or(serde_json::Value::Array(vec![])),
                );
            }
            serde_json::Value::Object(map)
        })
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
    }

    /// Structured sibling of [`Self::messages_all_groups_json`] for the typed
    /// FlatBuffers sidecar (ADR-0037, Wave A). Returns the SAME per-group data
    /// the JSON projection emits — `(group_id_hex, newest-N rows)` for every
    /// joined group — as native Rust structs instead of a `serde_json::Value`
    /// map, so [`crate::wire::messages_fb`] can encode them without re-parsing
    /// JSON.
    ///
    /// This is an additive read path: the authoritative JSON projection above is
    /// untouched and stays the source of truth. The two methods each issue an
    /// independent MDK read per tick (the typed sidecar is emitted alongside the
    /// JSON one); they are NOT merged so the JSON projection's wire behaviour is
    /// unchanged. The returned vector is in `get_groups()` order;
    /// [`crate::wire::messages_fb::encode_marmot_messages`] sorts by
    /// `group_id_hex` for a deterministic wire. D8 compliant: cheap,
    /// non-blocking. D6: poisoned mutex → empty vector.
    #[must_use]
    pub fn messages_all_groups(
        &self,
        page: usize,
    ) -> Vec<(String, Vec<crate::projection::payload::MarmotMessageRow>)> {
        self.with_inner(|h| {
            let group_ids: Vec<String> = h
                .service()
                .get_groups()
                .map(|gs| {
                    gs.into_iter()
                        .filter(|g| g.state == GroupState::Active)
                        .map(|g| hex_encode(g.mls_group_id.as_slice()))
                        .collect()
                })
                .unwrap_or_default();
            group_ids
                .into_iter()
                .map(|gid_hex| {
                    let rows = crate::projection::ops::group_messages(h, &gid_hex, page);
                    (gid_hex, rows)
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// Build the JSON snapshot. D6 — poisoned mutex → empty snapshot.
    #[must_use]
    pub fn snapshot(&self, now_secs: u64) -> MarmotSnapshot {
        let Ok(mut guard) = self.inner.lock() else {
            return MarmotSnapshot::empty();
        };

        // Expiry is wall-clock gated on snapshot edges as well as KP-ingest
        // edges (D8 — no timers, no polling). Snapshots fire on every
        // frame-producing actor tick, so a parked op whose KP never arrives
        // still expires within a tick of its 60 s deadline. Run the same
        // `evict_expired_pending` path here via a transient `InnerHandle`.
        InnerHandle { inner: &mut guard }.evict_expired_pending(now_secs);
        let inner = guard;

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

        // Reaching this snapshot path means the iOS shell has a live
        // `MarmotHandle`, so the identity IS registered. The `false` branch
        // is only ever served by `MarmotSnapshot::empty()` on the Swift side.
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

/// Lock-scoped accessor passed to FFI dispatch handlers. Keeps the `Mutex`
/// guard internal so handlers cannot leak it.
pub struct InnerHandle<'a> {
    pub(super) inner: &'a mut Inner,
}

impl<'a> InnerHandle<'a> {
    pub(crate) fn service(&self) -> &MarmotService {
        &self.inner.service
    }

    pub(crate) fn record_key_package(&mut self, d_tag: String, now_secs: u64) {
        self.inner.key_package_published_at = Some(now_secs);
        self.inner.key_package_d_tag = Some(d_tag);
    }

    /// Seed / overwrite the relay-pinned relay list for a group. Called
    /// from `create_group` (envelope `relays`) and `accept_welcome` /
    /// gift-wrap ingest (`Welcome::group_relays`). Empty list is ignored
    /// (keep any prior, more-specific entry).
    pub(crate) fn cache_group_relays(&mut self, group_id_hex: String, relays: Vec<RelayUrl>) {
        if relays.is_empty() {
            return;
        }
        let relay_urls = relays
            .iter()
            .map(|relay| relay.to_string())
            .collect::<Vec<_>>();
        self.inner.group_relays.insert(group_id_hex.clone(), relays);
        self.subscribe_group_messages(&group_id_hex, relay_urls);
    }

    /// Borrow the retained host `NmpApp` as `&NmpApp`, or `None` if no host
    /// app is bound (the in-memory test projection sets `app` to null).
    ///
    /// This is the SOLE `unsafe` deref of the retained `*mut NmpApp` in
    /// this crate. Every other call site (publish routing, write-relay
    /// lookup, interest push, key-package fetch) routes through here, so
    /// the soundness argument lives in ONE place and the publish-routing
    /// modules (`projection::publish`, `publish_group_pinned`,
    /// `publish_explicit`) are themselves `unsafe`-free.
    ///
    /// # SAFETY
    ///
    /// `inner.app` is the live `*mut NmpApp` retained by the host handle for
    /// its lifetime (non-null only after `set_app`). See the `unsafe impl
    /// Send/Sync` block at the top of this file for the full soundness
    /// argument (the `Drop` → `Shutdown` + `join` fence).
    pub(super) fn app(&self) -> Option<&NmpApp> {
        if self.inner.app.is_null() {
            return None;
        }
        // SAFETY: see this function's rustdoc.
        Some(unsafe { &*self.inner.app })
    }

    fn subscribe_group_messages(&self, group_id_hex: &str, relay_urls: Vec<String>) {
        let Some(app) = self.app() else {
            return;
        };
        for interest in crate::interest::group_message_interests(group_id_hex, relay_urls) {
            app.push_interest(interest);
        }
    }

    /// The cached relay-pinned relays for a group, or `&[]` on a miss
    /// (caller fails closed on the explicit publish boundary).
    #[must_use]
    pub(crate) fn group_relays(&self, group_id_hex: &str) -> Vec<RelayUrl> {
        self.inner
            .group_relays
            .get(group_id_hex)
            .cloned()
            .unwrap_or_default()
    }

    /// Publish a signed event to the group's relay-pinned relays
    /// (`Explicit`); a cache miss now fails closed instead of falling
    /// through to the author outbox.
    /// Used for kind:445 (group message / commit) and the kind:1059
    /// gift-wrap inbox-routing approximation.
    ///
    /// This method contains no `unsafe` block. The pointer deref happens once
    /// inside [`Self::app`]; the publish-routing call site is plain safe Rust.
    pub(crate) fn publish_group_pinned(&self, group_id_hex: &str, event: &nostr::Event) {
        let Some(app) = self.app() else {
            return;
        };
        let relays = self.group_relays(group_id_hex);
        crate::projection::publish::publish_to(app, event, &relays);
    }

    /// Publish a signed event to an EXPLICIT relay set (`Explicit`; empty
    /// → fail closed). Used by `create_group` / `invite` while a borrowed
    /// `PendingGroupChange` is still live (the relay-pinned cache is keyed
    /// by group and the relays are already known from the envelope, so we
    /// route directly without a `&mut self` cache read/write).
    ///
    /// `unsafe`-free for publish routing (see `publish_group_pinned`).
    pub(crate) fn publish_explicit(&self, event: &nostr::Event, relays: &[RelayUrl]) {
        let Some(app) = self.app() else {
            return;
        };
        crate::projection::publish::publish_to(app, event, relays);
    }

    /// Read the user's current write-relay URLs from the shared kernel
    /// relay-edit projection. Empty when no write relays are configured.
    #[must_use]
    pub(crate) fn write_relay_urls(&self) -> Vec<String> {
        let Some(app) = self.app() else {
            return Vec::new();
        };
        app.write_relay_urls()
    }

    /// Ask the kernel to fetch peer KeyPackage events for the given pubkeys.
    ///
    /// This is Rust-owned retry/recovery policy: `create_group` / `invite`
    /// discover the missing key packages, enqueue the lookup interests, then
    /// return a pending result for the UI to render. Native does not decide
    /// when to fetch or retry.
    pub(crate) fn request_key_package_fetch(&self, pubkeys: &[PublicKey]) -> usize {
        let Some(app) = self.app() else {
            return 0;
        };
        // `push_interest` is infallible once `app()` is `Some` (guarded above),
        // so the count always equals the pubkey count. Callers use it only as a
        // UI hint (`fetch_requested` in the `key_package_unavailable` response).
        let mut sent = 0;
        for pk in pubkeys {
            app.push_interest(crate::interest::key_package_lookup_interest(&pk.to_hex()));
            sent += 1;
        }
        sent
    }

    /// Cache an incoming gift-wrap as a pending Welcome (no MLS type held).
    pub(crate) fn cache_welcome(
        &mut self,
        id_hex: String,
        gift_wrap: Event,
        group_name: String,
        inviter_npub: String,
    ) {
        self.inner.pending_welcomes.insert(
            id_hex,
            CachedWelcome {
                gift_wrap,
                group_name,
                inviter_npub,
            },
        );
    }

    /// Look up + remove a cached pending Welcome, returning the gift-wrap
    /// `Event` so the caller can re-run the idempotent
    /// `unwrap_and_process_welcome` to obtain the `&Welcome`.
    #[must_use]
    pub(crate) fn take_welcome_gift_wrap(&mut self, id_hex: &str) -> Option<Event> {
        self.inner
            .pending_welcomes
            .remove(id_hex)
            .map(|c| c.gift_wrap)
    }

    /// Restore a previously-taken Welcome (used when accept/decline fails so
    /// the row reappears in the next snapshot for a retry).
    pub(crate) fn restore_welcome(
        &mut self,
        id_hex: String,
        gift_wrap: Event,
        group_name: String,
        inviter_npub: String,
    ) {
        self.cache_welcome(id_hex, gift_wrap, group_name, inviter_npub);
    }
}

impl KernelEventObserver for MarmotProjection {
    /// Metadata-only `KernelEvent` observer (see module rustdoc): a
    /// [`KernelEvent`] has no signature so we cannot feed kind:445 /
    /// kind:1059 into MDK from here — that is now done automatically by
    /// the parallel raw signed-event tap ([`crate::projection::tap`]). This
    /// observer only tracks metadata: if the local identity has published
    /// a key-package and the kernel re-ingests it (e.g. relay echo), keep
    /// the `published` flag warm so the snapshot reflects reality even
    /// before a `publish_key_package` dispatch this session.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_MARMOT_KEY_PACKAGE {
            // kind:445 / kind:1059 require a signed event — driven by the
            // raw signed-event tap (`crate::projection::tap`), not here.
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
