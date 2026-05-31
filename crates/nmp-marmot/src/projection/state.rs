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
//! via [`crate::projection::publish`] (the workspace-internal pure-Rust
//! `nmp_ffi::NmpApp::publish_signed_explicit` kernel API, called against
//! the retained `&NmpApp`). They no longer rely on a non-existent Swift
//! relay path. The publish module is `unsafe`-free for publish routing. The op result still
//! carries the signed event JSON (`event` / `events` / `evolution_event`
//! / `welcome_rumors`) but it is now INFORMATIONAL only — publish
//! already happened (fire-and-forget; success == "submitted to the
//! kernel publish pipeline").
//!
//! ## Inbound ingest seam — CLOSED (this is the receive direction)
//!
//! The kernel now also exposes a parallel raw signed-event tap
//! (`RawEventObserver`, sig included). The host shell's Marmot register
//! path registers [`crate::projection::tap`] against the retained
//! `*mut NmpApp` for kinds `[444, 445, 1059]`; the kernel delivers every
//! accepted inbound signed event of those kinds to it and it drives them
//! through the shared `ops::ingest_signed_event_core` (kind:1059 →
//! `unwrap_and_process_welcome`; kind:445 → `process_message`; seeds the
//! `group_id→relays` cache). Welcomes / messages received from relays
//! therefore surface in the next `snapshot` automatically — no Swift
//! path. The `{"op":"ingest_signed_event"}` dispatch op remains as a
//! back-compat alias over the same core.
//!
//! ## Threading
//!
//! MDK is synchronous (`&self`, interior mutability). `MarmotService` is
//! therefore sync and this projection does NOT invent threading — exactly
//! as `nmp-marmot`'s own rustdoc states ("callers in an async context
//! offload via the runtime's blocking bridge — not this crate's concern").
//!
//! This projection IS accessed from two threads concurrently: the kernel
//! actor thread (the `KernelEventObserver` fan-out + the raw signed-event
//! tap) and the host's FFI entry points (`snapshot` / dispatch). It does
//! not assume a single-threaded caller — the inner `Mutex` is what makes
//! that concurrent access sound. (An earlier revision of this comment
//! claimed "the Swift bridge serializes every FFI call onto a single
//! dispatch queue"; that is NOT true — Chirp's `KernelHandle` is a plain
//! `final class` with no queue. The FFI calls happen to originate from one
//! Swift isolation context (`@MainActor`), but the kernel callbacks do not,
//! so the `Mutex` is load-bearing, not a belt-and-braces extra.)
//!
//! ## Seams (documented, NOT blocking — see crate task)
//!
//! 1. **Signer seam.** `MarmotService::new` needs `nostr::Keys`. No
//!    kernel-level `Keys` provider exists for this crate yet, so
//!    the host shell's Marmot register path takes the secret key
//!    hex/nsec directly. Replace with a `KeyringCapability`-backed seam
//!    when one lands on `NmpApp`.
//! 2. **Lossy-observer seam — RESOLVED (inbound ingest CLOSED).** The
//!    kernel `KernelEventObserver` fan-out delivers a [`KernelEvent`]
//!    (id/author/kind/created_at/tags/content) — it carries NO signature,
//!    so a signed `nostr::Event` cannot be reconstructed from it, and
//!    `MarmotProjection::on_kernel_event` still only uses it for
//!    *metadata* (presence of the local identity's own kind:30443/443
//!    key-package). Actual MLS ingest of kind:445 group messages and
//!    kind:1059 gift-wraps is now driven automatically by the parallel
//!    raw signed-event tap ([`crate::projection::tap`], a
//!    `nmp-core` `RawEventObserver` that DOES carry `sig`), registered by
//!    the host shell. The
//!    `{"op":"ingest_signed_event","event_json":"…"}` dispatch op is kept
//!    as a back-compat alias over the SAME
//!    `ops::ingest_signed_event_core`; it no longer requires a Swift
//!    relay layer (none ever existed). This seam is no longer open.
//! 3. **KeyPackage cache seam.** `create_group` / `invite` need the
//!    invitees' *signed* kind:30443 key-package events. This crate has no
//!    kernel cache of signed events, so those ops accept an explicit
//!    `signed_key_package_events_json` array; absent it they return
//!    `{"ok":false,"error":"key_package_unavailable","needs":[…]}`.

use std::collections::HashMap;
use std::sync::Mutex;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_ffi::NmpApp;
use nostr::{Event, JsonUtil, PublicKey, RelayUrl};

use crate::service::MarmotService;

use crate::projection::display;
use crate::projection::payload::{
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
    /// A MISS → explicit publish fails closed (documented limitation).
    group_relays: HashMap<String, Vec<RelayUrl>>,
    /// The live `*mut NmpApp` the owning host Marmot handle retains. `null`
    /// for the in-memory test projection (publish degrades to a silent
    /// no-op there — the D6 fire-and-forget contract).
    app: *mut NmpApp,
    /// V-62: `true` when the projection was initialized with an in-memory
    /// mock credential store because the platform keyring was unavailable.
    /// Set once at construction; never flipped back to `false`. Surfaced in
    /// the snapshot so the host can warn the user and block group features.
    keyring_unavailable: bool,
}

/// Owned Marmot projection. `Mutex` because `on_kernel_event` takes `&self`
/// on the actor thread while the FFI snapshot / dispatch run on the Swift
/// bridge thread (low contention; the bridge serializes its calls).
pub struct MarmotProjection {
    inner: Mutex<Inner>,
}

// SAFETY: this `unsafe impl` is REQUIRED — `register_event_observer` casts
// `Arc<MarmotProjection>` to `Arc<dyn KernelEventObserver>`, and that trait
// is bounded `Send + Sync`. The auto-derived `!Send`/`!Sync` comes only from
// `Inner::app: *mut NmpApp`; every other field is already `Send + Sync`.
//
// The honest soundness argument (the prior comment's "Swift serializes
// every FFI call on one dispatch queue" is FALSE — `KernelHandle` is a
// plain `final class`, no queue):
//
//   * `MarmotProjection` is genuinely accessed from two threads — the
//     kernel actor thread (`on_kernel_event`, and the raw tap's
//     `on_raw_event` via `with_inner`) and the Swift main actor
//     (`snapshot` / `with_inner` from the FFI ops). All such access goes
//     through the inner `Mutex<Inner>`, which is what actually makes the
//     cross-thread sharing sound. The `unsafe impl` only asserts that the
//     `*mut NmpApp` field does not invalidate that.
//   * The `app` pointer is only ever READ (to forward fire-and-forget
//     commands to the kernel actor channel), never mutated. It cannot be
//     dangling at the point of use: `nmp_app_free` (`NmpApp::Drop`) sends
//     `Shutdown` and `join()`s the actor thread before freeing the
//     allocation, and every reader of `app` here runs INLINE on that actor
//     thread — the join fences any in-flight access.
//
// CALLER CONTRACT (upheld by the host FFI shell, `nmp-app-chirp`):
// `nmp_app_free` must not run while a kernel callback that reaches this
// projection is still executing. The in-process Rust-trait registration
// path provides that fence via the actor join.
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
    /// `keyring_unavailable` must be `true` when the service was initialized
    /// with the in-memory mock credential store (V-62). The flag is surfaced
    /// in every subsequent snapshot so the host can warn the user.
    #[must_use]
    pub fn new(service: MarmotService, keyring_unavailable: bool) -> Self {
        Self {
            inner: Mutex::new(Inner {
                service,
                pending_welcomes: HashMap::new(),
                key_package_published_at: None,
                key_package_d_tag: None,
                group_relays: HashMap::new(),
                app: std::ptr::null_mut(),
                keyring_unavailable,
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

    /// Build the JSON snapshot. D6 — poisoned mutex → empty snapshot.
    #[must_use]
    pub fn snapshot(&self, now_secs: u64) -> MarmotSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return MarmotSnapshot::empty();
        };

        let groups: Vec<MarmotGroupRow> = match inner.service.get_groups() {
            Ok(gs) => gs
                .into_iter()
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
                    let display_name = display::group_display_name(&g.name);
                    let initials = display::initials(&display_name);
                    let member_count = u32::try_from(members.len()).unwrap_or(u32::MAX);
                    MarmotGroupRow {
                        id_hex,
                        name: g.name.clone(),
                        display_name,
                        initials,
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
                display_name: display::welcome_display_name(&c.group_name),
                inviter_npub: c.inviter_npub.clone(),
            })
            .collect();
        let invites_chip_label = display::invites_chip_label(pending_welcomes.len());

        let mut key_package = match inner.key_package_published_at {
            Some(ts) => {
                let age = now_secs.saturating_sub(ts);
                KeyPackageStatus {
                    published: true,
                    d_tag: inner.key_package_d_tag.clone(),
                    age_secs: Some(age),
                    stale: age > KEY_PACKAGE_STALE_SECS,
                    age_display: Some(KeyPackageStatus::bucket_age(age)),
                    subtitle: String::new(),
                    action_label: String::new(),
                }
            }
            None => KeyPackageStatus::default(),
        };
        // Reaching this snapshot path means the iOS shell has a live
        // `MarmotHandle`, so the identity IS registered. The `false` branch
        // is only ever served by `MarmotSnapshot::empty()` on the Swift side.
        key_package.subtitle = key_package.render_subtitle(true);
        key_package.action_label = if key_package.published {
            KeyPackageStatus::ACTION_LABEL_ROTATE.to_string()
        } else {
            KeyPackageStatus::ACTION_LABEL_PUBLISH.to_string()
        };

        let cached_kp_pubkeys = inner.service.cached_kp_pubkeys();
        let orphaned_commit_count = inner.service.orphaned_commit_count();
        let keyring_unavailable = inner.keyring_unavailable;
        MarmotSnapshot {
            groups,
            pending_welcomes,
            key_package,
            cached_kp_pubkeys,
            invites_chip_label,
            is_registered: true,
            orphaned_commit_count,
            keyring_unavailable,
        }
    }
}

/// Lock-scoped accessor passed to FFI dispatch handlers. Keeps the `Mutex`
/// guard internal so handlers cannot leak it.
pub struct InnerHandle<'a> {
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
    /// `inner.app` is the live `*mut NmpApp` retained by the host Marmot
    /// handle for the handle's lifetime. The host shell guarantees the
    /// pointer is non-null only after `MarmotProjection::set_app` was
    /// called, and that `nmp_app_free` (`NmpApp::Drop`) sends `Shutdown`
    /// and `join()`s the actor thread before freeing the allocation —
    /// every reader here runs inline on that actor thread, so the join
    /// fences any in-flight access (see the `unsafe impl Send/Sync` block
    /// at the top of this file for the full soundness argument).
    fn app(&self) -> Option<&NmpApp> {
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
        if event.kind != MLS_KEY_PACKAGE_KIND && event.kind != MLS_KEY_PACKAGE_KIND_LEGACY {
            // kind:445 / kind:1059 require a signed event — driven by the
            // raw signed-event tap (`crate::projection::tap`), not here.
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

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
