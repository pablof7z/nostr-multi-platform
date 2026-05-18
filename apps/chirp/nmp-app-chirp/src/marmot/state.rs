//! `MarmotProjection` — the per-app Marmot state.
//!
//! Owns one [`MarmotService`] (the typed MDK translation layer from
//! `nmp-marmot`) plus the small amount of FFI-local bookkeeping that
//! `MarmotService` does not itself surface:
//!
//! * a cache of pending Welcomes keyed by kind:1059 gift-wrap event-id hex
//!   — we store the **gift-wrap `nostr::Event`** (NOT any MLS type, so the
//!   kernel-boundary "nmp-marmot is the sole importer of mdk-core/openmls"
//!   gate holds) plus the display strings the snapshot needs.
//!   `MarmotService::process_welcome` is idempotent (it returns the stored
//!   Welcome when the wrapper id was already processed and only errors on a
//!   *previously-failed* attempt — verified against mdk-core 0.8.0
//!   `welcomes.rs`), so accept/decline lazily re-runs
//!   `unwrap_and_process_welcome` to recover the `&Welcome` value those
//!   ops require without this crate ever naming an MLS type, and
//! * the local key-package publication timestamp + `d` tag (snapshot
//!   `age_secs` / `stale`), and
//! * a `group_id_hex → Vec<RelayUrl>` cache of each group's configured
//!   relay list. Marmot groups are relay-pinned (kind:445 commits /
//!   messages MUST go to the group relay, not the author outbox), but
//!   neither `MarmotService` nor `mdk-core`'s `group_types::Group`
//!   surfaces the relay list to a non-`nmp-marmot` consumer
//!   (`MDK::get_relays` exists but is not re-exported, and adding an
//!   accessor would touch `nmp-marmot`). We therefore cache the relays at
//!   the points where they ARE observable to this crate: the
//!   `create_group` dispatch envelope (`relays` array) and the
//!   `welcome_types::Welcome::group_relays` set recovered on
//!   `accept_welcome` / gift-wrap ingest. A cache MISS (e.g. a group
//!   joined in a session before this code landed) degrades the publish to
//!   author-outbox `Auto` — documented limitation, NOT a regression
//!   (those events previously did not reach relays at all).
//!
//! ## Outbound relay seam — CLOSED (this is the publish direction)
//!
//! The dispatch ops now publish their signed events to relays INTERNALLY
//! via [`crate::marmot::publish`] (the `nmp-core`
//! `nmp_app_publish_signed_event*` kernel capabilities, called against the
//! retained `*mut NmpApp`). They no longer rely on a non-existent Swift
//! relay path. The op result still carries the signed event JSON
//! (`event` / `events` / `evolution_event` / `welcome_rumors`) but it is
//! now INFORMATIONAL only — publish already happened (fire-and-forget;
//! success == "submitted to the kernel publish pipeline"). The INBOUND
//! ingest seam (`{"op":"ingest_signed_event"}`) is a SEPARATE seam and is
//! still open (see seam #2 below).
//!
//! ## Threading
//!
//! MDK is synchronous (`&self`, interior mutability). `MarmotService` is
//! therefore sync and this projection does NOT invent threading — exactly
//! as `nmp-marmot`'s own rustdoc states ("callers in an async context
//! offload via the runtime's blocking bridge — not this crate's concern").
//! The Swift bridge already serializes every FFI call onto a single
//! dispatch queue (the same invariant `nmp_app_chirp_snapshot` relies on),
//! so the kernel-observer fan-out (actor thread) and the dispatch/snapshot
//! entry points (Swift bridge thread) never tear the inner `Mutex`.
//!
//! ## Seams (documented, NOT blocking — see crate task)
//!
//! 1. **Signer seam.** `MarmotService::new` needs `nostr::Keys`. No
//!    kernel-level `Keys` provider exists for this crate yet, so
//!    `nmp_app_chirp_marmot_register` takes the secret key hex/nsec
//!    directly. Replace with a `KeyringCapability`-backed seam when one
//!    lands on `NmpApp`.
//! 2. **Lossy-observer seam.** The kernel `KernelEventObserver` fan-out
//!    delivers a [`KernelEvent`] (id/author/kind/created_at/tags/content)
//!    — it carries NO signature, so a signed `nostr::Event` cannot be
//!    reconstructed. `MDK::process_message` /
//!    `unwrap_and_process_welcome` REQUIRE a signed event. The observer
//!    therefore only tracks *metadata* it can derive from the lossy
//!    projection (presence of the local identity's own kind:30443/443
//!    key-package). Actual MLS ingest of kind:445 group messages and
//!    kind:1059 gift-wraps is driven through the
//!    `{"op":"ingest_signed_event","event_json":"…"}` dispatch op, which
//!    takes the full signed event JSON from the Swift relay layer. Remove
//!    this op once the kernel exposes signed `nostr::Event`s to observers.
//! 3. **KeyPackage cache seam.** `create_group` / `invite` need the
//!    invitees' *signed* kind:30443 key-package events. This crate has no
//!    kernel cache of signed events, so those ops accept an explicit
//!    `signed_key_package_events_json` array; absent it they return
//!    `{"ok":false,"error":"key_package_unavailable","needs":[…]}`.

use std::collections::HashMap;
use std::sync::Mutex;

use nmp_core::substrate::KernelEvent;
use nmp_core::{KernelEventObserver, NmpApp};
use nostr::{Event, JsonUtil, RelayUrl};

use nmp_marmot::service::MarmotService;

use crate::marmot::payload::{
    KeyPackageStatus, MarmotGroupRow, MarmotSnapshot, PendingWelcomeRow,
};

/// Marmot KeyPackage kinds (mirrors `nmp_marmot::interest`; kept local so
/// this crate does not reach into `nmp-marmot` internals).
const MLS_KEY_PACKAGE_KIND: u32 = 30443;
const MLS_KEY_PACKAGE_KIND_LEGACY: u32 = 443;

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

struct Inner {
    service: MarmotService,
    /// kind:1059 gift-wrap-event-id hex → cached pending Welcome.
    pending_welcomes: HashMap<String, CachedWelcome>,
    /// Wall-clock secs of the most recent `publish_key_package` dispatch.
    key_package_published_at: Option<u64>,
    /// `d` tag of the most recent key-package publication.
    key_package_d_tag: Option<String>,
    /// `group_id_hex` → the group's configured (relay-pinned) relay list,
    /// seeded from the `create_group` envelope + `Welcome::group_relays`.
    /// A MISS → publish falls back to `Auto` (documented limitation).
    group_relays: HashMap<String, Vec<RelayUrl>>,
    /// The live `*mut NmpApp` the owning `MarmotHandle` retains. `null`
    /// for the in-memory test projection (publish degrades to a silent
    /// no-op there — the D6 fire-and-forget contract).
    app: *mut NmpApp,
}

/// Owned Marmot projection. `Mutex` because `on_kernel_event` takes `&self`
/// on the actor thread while the FFI snapshot / dispatch run on the Swift
/// bridge thread (low contention; the bridge serializes its calls).
pub struct MarmotProjection {
    inner: Mutex<Inner>,
}

// SAFETY: identical rationale to `MarmotHandle` — Swift drives every FFI
// call from one serialized bridge dispatch queue; the only `!Send`/`!Sync`
// material is the `app` raw pointer, which is never mutated cross-thread
// and is only ever read to forward a fire-and-forget publish to the kernel
// actor channel (same validity rule as the observer register/unregister).
unsafe impl Send for MarmotProjection {}
unsafe impl Sync for MarmotProjection {}

impl MarmotProjection {
    /// Build the projection around an already-constructed [`MarmotService`].
    /// The FFI layer owns service construction (it must parse the signer
    /// seam key + resolve the app-support DB path) so this stays infallible.
    /// `app` starts `null`; the FFI `register` path calls
    /// [`MarmotProjection::set_app`] with the retained pointer. Tests that
    /// build the projection directly leave it `null` → publish no-ops.
    pub fn new(service: MarmotService) -> Self {
        Self {
            inner: Mutex::new(Inner {
                service,
                pending_welcomes: HashMap::new(),
                key_package_published_at: None,
                key_package_d_tag: None,
                group_relays: HashMap::new(),
                app: std::ptr::null_mut(),
            }),
        }
    }

    /// Record the live `*mut NmpApp` so the dispatch ops can publish
    /// internally. Called once by `nmp_app_chirp_marmot_register` with the
    /// same pointer the `MarmotHandle` retains for its lifetime. D6 —
    /// poisoned mutex silently no-ops (publish then degrades to no-op).
    pub fn set_app(&self, app: *mut NmpApp) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.app = app;
        }
    }

    /// Borrow the inner state under the lock for an FFI op. Returns `None`
    /// on a poisoned mutex (D6 — caller degrades to null / `{"ok":false}`).
    pub(crate) fn with_inner<R>(
        &self,
        f: impl FnOnce(&mut InnerHandle<'_>) -> R,
    ) -> Option<R> {
        let mut guard = self.inner.lock().ok()?;
        let mut h = InnerHandle { inner: &mut guard };
        Some(f(&mut h))
    }

    /// Build the JSON snapshot. D6 — poisoned mutex → empty snapshot.
    pub fn snapshot(&self, now_secs: u64) -> MarmotSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return MarmotSnapshot::empty();
        };

        let groups = match inner.service.get_groups() {
            Ok(gs) => gs
                .into_iter()
                .map(|g| {
                    let id_hex = hex_encode(g.mls_group_id.as_slice());
                    let members = inner
                        .service
                        .get_members(&g.mls_group_id)
                        .map(|set| {
                            set.into_iter().map(|pk| pk.to_hex()).collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    // Unread seam: no read-cursor — total app-message count.
                    let unread = inner
                        .service
                        .get_messages(&g.mls_group_id)
                        .map(|m| m.len() as u64)
                        .unwrap_or(0);
                    MarmotGroupRow {
                        id_hex,
                        name: g.name.clone(),
                        members,
                        unread,
                        last_msg_at: g.last_message_at.map(|t| t.as_secs()),
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        let pending_welcomes = inner
            .pending_welcomes
            .iter()
            .map(|(id_hex, c)| PendingWelcomeRow {
                id_hex: id_hex.clone(),
                group_name: c.group_name.clone(),
                inviter_npub: c.inviter_npub.clone(),
            })
            .collect();

        let key_package = match inner.key_package_published_at {
            Some(ts) => {
                let age = now_secs.saturating_sub(ts);
                KeyPackageStatus {
                    published: true,
                    d_tag: inner.key_package_d_tag.clone(),
                    age_secs: Some(age),
                    stale: age > KEY_PACKAGE_STALE_SECS,
                }
            }
            None => KeyPackageStatus::default(),
        };

        MarmotSnapshot {
            groups,
            pending_welcomes,
            key_package,
        }
    }
}

/// Lock-scoped accessor passed to FFI dispatch handlers. Keeps the `Mutex`
/// guard internal so handlers cannot leak it.
pub(crate) struct InnerHandle<'a> {
    inner: &'a mut Inner,
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
    pub(crate) fn cache_group_relays(
        &mut self,
        group_id_hex: String,
        relays: Vec<RelayUrl>,
    ) {
        if relays.is_empty() {
            return;
        }
        self.inner.group_relays.insert(group_id_hex, relays);
    }

    /// The cached relay-pinned relays for a group, or `&[]` on a miss
    /// (caller falls back to `Auto` — documented limitation).
    pub(crate) fn group_relays(&self, group_id_hex: &str) -> Vec<RelayUrl> {
        self.inner
            .group_relays
            .get(group_id_hex)
            .cloned()
            .unwrap_or_default()
    }

    /// Publish a signed event to the group's relay-pinned relays
    /// (`Explicit`); a cache miss → `Auto` (empty relay set falls through
    /// to the author outbox — the kernel symbol's documented behaviour).
    /// Used for kind:445 (group message / commit) and the kind:1059
    /// gift-wrap inbox-routing approximation.
    pub(crate) fn publish_group_pinned(
        &self,
        group_id_hex: &str,
        event: &nostr::Event,
    ) {
        let relays = self.group_relays(group_id_hex);
        crate::marmot::publish::publish_to(self.inner.app, event, &relays);
    }

    /// Publish a signed event to an EXPLICIT relay set (`Explicit`; empty
    /// → `Auto`). Used by `create_group` / `invite` while a borrowed
    /// `PendingGroupChange` is still live (the relay-pinned cache is keyed
    /// by group and the relays are already known from the envelope, so we
    /// route directly without a `&mut self` cache read/write).
    pub(crate) fn publish_explicit(
        &self,
        event: &nostr::Event,
        relays: &[RelayUrl],
    ) {
        crate::marmot::publish::publish_to(self.inner.app, event, relays);
    }

    /// Publish a signed event to the author NIP-65 outbox (`Auto`).
    /// Correct for kind:30443 / kind:443 key-packages.
    pub(crate) fn publish_author_outbox(&self, event: &nostr::Event) {
        crate::marmot::publish::publish_auto(self.inner.app, event);
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
    /// Lossy-observer seam (see module rustdoc): a [`KernelEvent`] has no
    /// signature so we cannot feed kind:445 / kind:1059 into MDK from here.
    /// We only track metadata: if the local identity has published a
    /// key-package and the kernel re-ingests it (e.g. relay echo), keep the
    /// `published` flag warm so the snapshot reflects reality even before a
    /// `publish_key_package` dispatch this session.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != MLS_KEY_PACKAGE_KIND && event.kind != MLS_KEY_PACKAGE_KIND_LEGACY
        {
            // kind:445 / kind:1059 require a signed event — handled via the
            // `ingest_signed_event` dispatch op, not here.
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
pub(crate) fn parse_signed_event(json: &str) -> Result<Event, String> {
    Event::from_json(json).map_err(|e| format!("invalid signed event json: {e}"))
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
