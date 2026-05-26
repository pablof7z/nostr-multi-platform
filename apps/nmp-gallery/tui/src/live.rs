//! Persistent live-mode kernel host for the gallery TUI.
//!
//! The gallery is **live-only** (ADR-0034 / M16): there is no fixture mode,
//! no pre-warm bootstrap, no synthesized embed envelopes. The kernel boots
//! once at program start and stays alive for the lifetime of the process.
//!
//! Initial cold-start does an opportunistic fetch of the demo profile +
//! the mention/media/quote events so the user-* component pages render
//! real kind:0 data immediately. After that, all further data — including
//! every embedded event in the kind-dispatch showcase — flows through the
//! standard snapshot push:
//!
//! 1. Renderer encounters an `EventRef(uri)` token.
//! 2. `NostrContentView` calls `sink.claim(uri, consumer_id)` via the
//!    `EventClaimSink` host bridge.
//! 3. `LiveKernelSink::claim` forwards to `nmp_app_claim_event` — the
//!    kernel registers a `OneshotApi` interest (D4 single writer), short-
//!    circuits on cache hit, or compiles a wire REQ on cache miss.
//! 4. The event arrives (cache or relay), gets surfaced in
//!    `snapshot.projections.claimed_events[primary_id]`, the gallery's
//!    snapshot thread sends a `GalleryEvent::Snapshot` to the main loop,
//!    `EmbedHostState::update_from_snapshot` decodes it, and the next
//!    redraw shows the resolved article (or short-note / highlight / ...).
//!
//! `LiveKernel` is `pub` so `main.rs` can keep it alive for the program
//! lifetime; `LiveKernelSink` wraps the `*mut NmpApp` pointer as the
//! `EventClaimSink` plugged into the renderer via the W4/W5 wiring.

use std::{
    ffi::{c_void, CString},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

use nmp_content::EventClaimSink;
use nmp_core::nip21::{parse_nostr_uri, NostrUri};
use serde_json::Value;

const PRIMARY_PUBKEY: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
const MENTION_EVENT_ID: &str = "caef905a1e1520fd6621b56364cca823c262327a32ac063b4ff0435f41aa7660";
const MEDIA_EVENT_ID: &str = "c2ee64b0371f290edf66fc797598b2d307aa79192f6d6e0bf5344cf81104029b";
const QUOTE_SOURCE_EVENT_ID: &str =
    "2df88accbf264b10f47809abcf9d32b4146b035a5a197c9ff30e45ac010d5368";
const CONSUMER_ID: &str = "nmp-gallery-tui.preview";

const RELAYS: &[(&str, &str)] = &[
    ("wss://purplepag.es", "indexer"),
    ("wss://nos.lol", "both"),
    ("wss://relay.damus.io", "both"),
    ("wss://relay.nostr.band", "both"),
];

pub struct LiveGallerySource {
    timeout: Duration,
}

/// Initial cold-start data. Populated by `LiveGallerySource::bootstrap`
/// before the main loop takes over the snapshot stream. Embedded events
/// are NOT pre-warmed here — the renderer-triggered claim path drives
/// those (ADR-0034).
pub struct LiveFacts {
    pub primary_profile: LiveProfile,
    pub mention_profile: LiveProfile,
    pub quote_target_profile: LiveProfile,
    pub mention_item: LiveItem,
    pub media_item: LiveItem,
    pub quote_source_item: LiveItem,
    pub quote_target_item: LiveItem,
    pub mention_profile_uri: String,
    pub quote_event_uri: String,
}

#[derive(Clone)]
pub struct LiveProfile {
    pub pubkey: String,
    pub display_name: Option<String>,
    pub picture_url: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
}

#[derive(Clone)]
pub struct LiveItem {
    pub id: String,
    pub author_pubkey: String,
    pub kind: u32,
    pub content: String,
    pub content_preview: String,
    pub created_at: u64,
}

struct LiveAuthorView {
    profile: LiveProfile,
}

/// Persistent kernel handle. Owned by the gallery's main loop for the
/// entire process lifetime. The actor thread keeps running; snapshot pushes
/// arrive on `rx` until `Drop` tears the app down (program exit).
pub struct LiveKernel {
    /// Raw `*mut NmpApp` pointer. The actor (running on its own threads)
    /// is the single owner of the pointer's mutable state — every FFI
    /// symbol routes through its command channel. The pointer is opaque to
    /// callers and is only used to identify the app instance.
    pub app: *mut nmp_ffi::NmpApp,
    /// Keepalive for the update-callback context. Lives as long as
    /// `LiveKernel` does so the callback never sees a dangling pointer.
    bridge: Option<Box<UpdateBridge>>,
    /// Snapshot stream — taken once by `take_receiver` so the main loop
    /// can hand it to its snapshot-thread aggregator.
    rx: Option<Receiver<String>>,
}

struct UpdateBridge {
    tx: Sender<String>,
}

/// `EventClaimSink` impl wrapping a live kernel's app pointer. The
/// renderer-triggered claim path (`NostrContentView::claim_sink`) calls
/// this on each render frame; `claim` forwards to `nmp_app_claim_event`,
/// `release` to `nmp_app_release_event`. `Send + Sync` because every FFI
/// symbol forwards to the actor's command channel — the pointer is just
/// an opaque key.
pub struct LiveKernelSink {
    pub app: *mut nmp_ffi::NmpApp,
}

unsafe impl Send for LiveKernelSink {}
unsafe impl Sync for LiveKernelSink {}

impl EventClaimSink for LiveKernelSink {
    fn claim(&self, uri: &str, consumer_id: &str) {
        let Ok(uri_c) = CString::new(uri) else { return };
        let Ok(cid) = CString::new(consumer_id) else { return };
        nmp_ffi::nmp_app_claim_event(self.app, uri_c.as_ptr(), cid.as_ptr());
    }

    fn release(&self, uri: &str, consumer_id: &str) {
        let Ok(uri_c) = CString::new(uri) else { return };
        let Ok(cid) = CString::new(consumer_id) else { return };
        nmp_ffi::nmp_app_release_event(self.app, uri_c.as_ptr(), cid.as_ptr());
    }
}

impl LiveGallerySource {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Boot the kernel and seed the relay pool without waiting on any
    /// specific events. Used by the `--smoke` mode to validate the embed
    /// architecture in isolation from cold-start latency / relay flakes.
    /// Returns the kernel; the caller is responsible for any further
    /// data fetches via the standard claim_* / open_* FFI surface.
    pub fn boot_kernel_only() -> Result<LiveKernel, String> {
        LiveKernel::new()
    }

    /// Boot the kernel, fetch the initial profile + thread/author/media
    /// items the user-* component pages need, and return a `(LiveFacts,
    /// LiveKernel)` pair. The kernel STAYS ALIVE — caller owns it for the
    /// lifetime of the program. Embeds are NOT pre-warmed.
    pub fn bootstrap(&self) -> Result<(LiveFacts, LiveKernel), String> {
        let kernel = LiveKernel::new()?;
        let primary = kernel.wait_for_author(PRIMARY_PUBKEY, &[], self.timeout)?;
        let mention_item = kernel.wait_for_event(MENTION_EVENT_ID, None, self.timeout)?;
        let media_item = kernel.wait_for_event(MEDIA_EVENT_ID, None, self.timeout)?;
        let quote_source_item = kernel.wait_for_event(QUOTE_SOURCE_EVENT_ID, None, self.timeout)?;

        let (mention_profile_uri, mention_pubkey) = first_profile_uri(&mention_item.content)
            .ok_or_else(|| "live mention event did not contain a nostr profile URI".to_string())?;
        let mention_profile = kernel
            .wait_for_author(&mention_pubkey, &[], self.timeout)?
            .profile;

        let (quote_event_uri, quote_event_id) = first_event_uri(&quote_source_item.content)
            .ok_or_else(|| {
                "live quote source event did not contain a nostr event URI".to_string()
            })?;
        let quote_target_item =
            kernel.wait_for_event(&quote_event_id, Some(&quote_event_uri), self.timeout)?;
        let quote_target_profile = kernel
            .wait_for_author(&quote_target_item.author_pubkey, &[], self.timeout)?
            .profile;

        let facts = LiveFacts {
            primary_profile: primary.profile,
            mention_profile,
            quote_target_profile,
            mention_item,
            media_item,
            quote_source_item,
            quote_target_item,
            mention_profile_uri,
            quote_event_uri,
        };
        Ok((facts, kernel))
    }
}

impl LiveProfile {
    pub fn display_label(&self) -> String {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.pubkey.clone())
    }
}

impl LiveItem {
    pub fn preview(&self) -> String {
        if self.content_preview.trim().is_empty() {
            self.content.replace('\n', " ").chars().take(180).collect()
        } else {
            self.content_preview.clone()
        }
    }
}

impl LiveKernel {
    fn new() -> Result<Self, String> {
        let app = nmp_ffi::nmp_app_new();
        if app.is_null() {
            return Err("nmp_app_new returned null".to_string());
        }
        nmp_app_gallery::nmp_app_gallery_register(app as *mut c_void);

        let (tx, rx) = mpsc::channel();
        let mut bridge = Box::new(UpdateBridge { tx });
        let context = bridge.as_mut() as *mut UpdateBridge as *mut c_void;
        nmp_ffi::nmp_app_set_update_callback(app, context, Some(on_update));
        nmp_ffi::nmp_app_start(app, 0, 200, 8);

        let kernel = Self {
            app,
            bridge: Some(bridge),
            rx: Some(rx),
        };
        for (url, role) in RELAYS {
            kernel.add_relay(url, role)?;
        }
        Ok(kernel)
    }

    /// Take ownership of the snapshot receiver. After this call, the kernel
    /// can no longer poll its own snapshots (`wait_for_*` would return
    /// `Disconnected`) — the main loop owns the stream. Idempotent: a
    /// second call returns `None`.
    pub fn take_receiver(&mut self) -> Option<Receiver<String>> {
        self.rx.take()
    }

    fn wait_for_author(
        &self,
        pubkey: &str,
        required_item_ids: &[&str],
        timeout: Duration,
    ) -> Result<LiveAuthorView, String> {
        self.claim_profile(pubkey)?;
        self.open_author(pubkey)?;
        let started = Instant::now();
        let mut last = String::new();
        loop {
            let payload = self.next_payload(started, timeout, &last, "author view")?;
            last = payload;
            let Some(snapshot) = parse_snapshot(&last) else {
                continue;
            };
            if let Some(view) = author_view_for(&snapshot, pubkey, required_item_ids)? {
                return Ok(view);
            }
        }
    }

    fn wait_for_event(
        &self,
        event_id: &str,
        uri: Option<&str>,
        timeout: Duration,
    ) -> Result<LiveItem, String> {
        if let Some(uri) = uri {
            self.open_uri(uri)?;
        }
        self.open_thread(event_id)?;
        let started = Instant::now();
        let mut last = String::new();
        loop {
            let payload = self.next_payload(started, timeout, &last, "thread view")?;
            last = payload;
            let Some(snapshot) = parse_snapshot(&last) else {
                continue;
            };
            if let Some(item) = thread_item_for(&snapshot, event_id)? {
                return Ok(item);
            }
        }
    }

    fn next_payload(
        &self,
        started: Instant,
        timeout: Duration,
        last: &str,
        label: &str,
    ) -> Result<String, String> {
        let remaining = timeout
            .checked_sub(started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                format!(
                    "timed out waiting for live {label}: {}",
                    snapshot_summary(last)
                )
            })?;
        let rx = self
            .rx
            .as_ref()
            .ok_or_else(|| "live kernel receiver already taken by main loop".to_string())?;
        match rx.recv_timeout(remaining) {
            Ok(payload) => Ok(payload),
            Err(RecvTimeoutError::Timeout) => Err(format!(
                "timed out waiting for live {label}: {}",
                snapshot_summary(last)
            )),
            Err(RecvTimeoutError::Disconnected) => {
                Err(format!("live {label} update channel disconnected"))
            }
        }
    }

    fn add_relay(&self, url: &str, role: &str) -> Result<(), String> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }

    fn claim_profile(&self, pubkey: &str) -> Result<(), String> {
        let pubkey = CString::new(pubkey).map_err(|_| "pubkey contains NUL byte".to_string())?;
        let consumer =
            CString::new(CONSUMER_ID).map_err(|_| "consumer id contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_claim_profile(self.app, pubkey.as_ptr(), consumer.as_ptr());
        Ok(())
    }

    fn open_author(&self, pubkey: &str) -> Result<(), String> {
        let pubkey = CString::new(pubkey).map_err(|_| "pubkey contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_open_author(self.app, pubkey.as_ptr());
        Ok(())
    }

    fn open_thread(&self, event_id: &str) -> Result<(), String> {
        let event_id =
            CString::new(event_id).map_err(|_| "event id contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_open_thread(self.app, event_id.as_ptr());
        Ok(())
    }

    fn open_uri(&self, uri: &str) -> Result<(), String> {
        let uri = CString::new(uri).map_err(|_| "nostr URI contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_open_uri(self.app, uri.as_ptr());
        Ok(())
    }
}

impl Drop for LiveKernel {
    fn drop(&mut self) {
        if !self.app.is_null() {
            nmp_ffi::nmp_app_set_update_callback(self.app, std::ptr::null_mut(), None);
            nmp_ffi::nmp_app_free(self.app);
            self.app = std::ptr::null_mut();
        }
        self.bridge.take();
    }
}

extern "C" fn on_update(context: *mut c_void, payload: *const u8, len: usize) {
    if context.is_null() || payload.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) };
    let Ok(snapshot) = nmp_core::decode_snapshot_payload(bytes) else {
        return;
    };
    let bridge = unsafe { &*(context as *const UpdateBridge) };
    let _ = bridge.tx.send(snapshot.to_string());
}

/// Parse a kernel update payload (envelope or bare snapshot). Public so
/// the main loop's snapshot aggregator can decode pushed frames into the
/// `serde_json::Value` shape `EmbedHostState::update_from_snapshot` reads.
pub fn parse_snapshot(payload: &str) -> Option<Value> {
    let envelope: Value = serde_json::from_str(payload).ok()?;
    if envelope.get("t").and_then(Value::as_str) == Some("snapshot") {
        envelope.get("v").cloned()
    } else {
        Some(envelope)
    }
}

fn author_view_for(
    snapshot: &Value,
    pubkey: &str,
    required_item_ids: &[&str],
) -> Result<Option<LiveAuthorView>, String> {
    let Some(view) = snapshot
        .get("projections")
        .and_then(|value| value.get("author_view"))
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    if view.get("pubkey").and_then(Value::as_str) != Some(pubkey) {
        return Ok(None);
    }
    let Some(profile) = profile_from_value(view.get("profile").unwrap_or(&Value::Null)) else {
        return Ok(None);
    };
    if !has_profile(view.get("profile").unwrap_or(&Value::Null)) {
        return Ok(None);
    }
    let items = view
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(item_from_value).collect::<Vec<_>>())
        .unwrap_or_default();
    let has_required = required_item_ids
        .iter()
        .all(|required| items.iter().any(|item| item.id == *required));
    if !has_required {
        return Ok(None);
    }
    Ok(Some(LiveAuthorView { profile }))
}

fn thread_item_for(snapshot: &Value, event_id: &str) -> Result<Option<LiveItem>, String> {
    let Some(items) = snapshot
        .get("projections")
        .and_then(|value| value.get("thread_view"))
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    for value in items {
        if value.get("id").and_then(Value::as_str) == Some(event_id) {
            return Ok(item_from_value(value));
        }
    }
    Ok(None)
}

fn profile_from_value(value: &Value) -> Option<LiveProfile> {
    let pubkey = string(value, "pubkey")?;
    Some(LiveProfile {
        pubkey,
        display_name: string(value, "display_name"),
        picture_url: string(value, "picture_url"),
        nip05: string(value, "nip05"),
        about: string(value, "about"),
    })
}

fn item_from_value(value: &Value) -> Option<LiveItem> {
    Some(LiveItem {
        id: string(value, "id")?,
        author_pubkey: string(value, "author_pubkey")?,
        kind: value.get("kind").and_then(Value::as_u64)? as u32,
        content: string(value, "content")?,
        content_preview: string(value, "content_preview").unwrap_or_default(),
        created_at: value.get("created_at").and_then(Value::as_u64).unwrap_or(0),
    })
}

fn has_profile(value: &Value) -> bool {
    value
        .get("has_profile")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn first_profile_uri(content: &str) -> Option<(String, String)> {
    nostr_targets(content).find_map(|(uri, target)| match target {
        NostrUri::Profile { pubkey, .. } => Some((uri, pubkey)),
        _ => None,
    })
}

fn first_event_uri(content: &str) -> Option<(String, String)> {
    nostr_targets(content).find_map(|(uri, target)| match target {
        NostrUri::Event { event_id, .. } => Some((uri, event_id)),
        _ => None,
    })
}

fn nostr_targets(content: &str) -> impl Iterator<Item = (String, NostrUri)> + '_ {
    content.split_whitespace().filter_map(|word| {
        let start = word.find("nostr:")?;
        let uri = word[start..].trim_matches(uri_boundary_char).to_string();
        parse_nostr_uri(&uri).ok().map(|target| (uri, target))
    })
}

fn uri_boundary_char(ch: char) -> bool {
    matches!(
        ch,
        ',' | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '"'
            | '\''
    )
}

fn snapshot_summary(payload: &str) -> String {
    if payload.is_empty() {
        return "no snapshots received".to_string();
    }
    let Some(snapshot) = parse_snapshot(payload) else {
        return format!("last payload was not JSON ({} bytes)", payload.len());
    };
    let metrics = snapshot
        .get("metrics")
        .map(Value::to_string)
        .unwrap_or_else(|| "no metrics".to_string());
    let relays = snapshot
        .get("relay_statuses")
        .map(Value::to_string)
        .unwrap_or_else(|| "no relay_statuses".to_string());
    format!("metrics={metrics}; relay_statuses={relays}")
}
