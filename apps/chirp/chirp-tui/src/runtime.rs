use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::ptr;
use std::sync::mpsc::Receiver;

use nmp_app_chirp::ffi::{nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list};
use nmp_app_chirp::{
    follow_spec, nmp_app_chirp_close_group_discovery, nmp_app_chirp_declare_consumed_projections,
    nmp_app_chirp_identity_restore, nmp_app_chirp_register, nmp_app_chirp_unregister,
    nmp_marmot_unregister, nmp_signer_broker_init, publish_note_action, react_spec, repost_spec,
    unfollow_spec, ChirpHandle, MarmotHandle, NmpRegisterStatus,
};
use nmp_core::tags::Nip10Refs;
use nmp_nip01::NoteRecord;

use crate::app::ReplyTarget;
use nmp_ffi::{
    nmp_app_free, nmp_app_load_older_feed, nmp_app_release_profile_ref,
    nmp_app_resolve_profile_card_live, nmp_app_resolve_profile_ref, nmp_app_start, GroupFeedHandle,
    NmpApp, NmpConfigStatus,
};
use serde_json::{json, Value};

use crate::bridge::{self, NmpEvent, NmpUpdateBridge};
use crate::Result;

const VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX: &str = "chirp-tui.visible-author";
const VISIBLE_NOTE_RELATIONS_CONSUMER_PREFIX: &str = "chirp-tui.visible-note";
/// Consumer prefix for the open profile pane's `profile.card` / `Live` ref
/// (ADR-0063 #1671 Lane F). Distinct from the feed-row visible-author consumer so
/// the slot is upgraded to Live/card while the pane is open and downgraded back
/// to whatever feed rows still demand when the pane closes.
const OPEN_PROFILE_CONSUMER_PREFIX: &str = "chirp-tui.open-profile";

#[path = "runtime/feed.rs"]
mod feed;

pub struct AppRuntime {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    pub(crate) marmot: Cell<*mut MarmotHandle>,
    /// Open group-discovery handle; closed (and replaced) on each `discover_groups`
    /// call, then finally freed in `Drop`. `null_mut()` when inactive.
    pub(crate) discovery: Cell<*mut GroupFeedHandle>,
    feed_handles: RefCell<feed::FeedHandles>,
    update_bridge: Option<Box<NmpUpdateBridge>>,
}

impl AppRuntime {
    #[must_use]
    pub fn new() -> Result<(Self, Receiver<NmpEvent>)> {
        let app = nmp_ffi::nmp_app_new();
        if app.is_null() {
            return Err("nmp_app_new returned null".to_string());
        }
        let broker_rc = nmp_signer_broker_init(app);
        if broker_rc != NmpConfigStatus::Ok as u32 {
            nmp_app_free(app);
            return Err(format!(
                "nmp_signer_broker_init failed with NmpConfigStatus={broker_rc}"
            ));
        }

        nmp_ffi::nmp_app_set_capability_callback(
            app,
            ptr::null_mut(),
            Some(crate::keyring::keyring_handler),
        );

        // V-73: nmp_app_chirp_register now returns a status code; the handle is
        // written through the out-parameter.  Passing null viewer_pubkey (no
        // viewer set at startup) always succeeds.
        let mut chirp: *mut ChirpHandle = ptr::null_mut();
        let register_status = nmp_app_chirp_register(app, ptr::null(), &mut chirp);
        if register_status != NmpRegisterStatus::Ok as u32 || chirp.is_null() {
            nmp_app_free(app);
            return Err(format!(
                "nmp_app_chirp_register failed (status={register_status})"
            ));
        }

        let (mut bridge, rx) = NmpUpdateBridge::channel();
        NmpUpdateBridge::register(app, &mut bridge);
        nmp_app_chirp_register_dm_inbox(app);
        nmp_app_chirp_register_follow_list(app, ptr::null());

        let db_dir = crate::keyring::chirp_data_dir()
            .map(|p| p.join("marmot"))
            .and_then(|p| std::fs::create_dir_all(&p).ok().map(|_| p));
        let marmot = db_dir.and_then(|dir| {
            let dir_c = CString::new(dir.to_string_lossy().as_ref()).ok()?;
            let h = nmp_app_chirp_identity_restore(app, dir_c.as_ptr(), ptr::null());
            if h.is_null() {
                None
            } else {
                Some(h)
            }
        });
        let initial_marmot = marmot.unwrap_or(ptr::null_mut());

        // ADR-0053 / Workstream-E4 — declare projection-consumption intent
        // BEFORE start. chirp-tui is a full client (reads every kernel built-in),
        // so it consumes all explicitly; an undeclared start is a loud
        // forgotten-wiring bug, not a silent firehose.
        nmp_app_chirp_declare_consumed_projections(app);

        nmp_app_start(app, 200, 10);
        let home_feed_handle = feed::open_home_feed(app).ok();

        Ok((
            Self {
                app,
                chirp,
                marmot: Cell::new(initial_marmot),
                discovery: Cell::new(ptr::null_mut()),
                feed_handles: RefCell::new(feed::FeedHandles {
                    home: home_feed_handle,
                    ..feed::FeedHandles::default()
                }),
                update_bridge: Some(bridge),
            },
            rx,
        ))
    }

    pub fn add_relay(&self, url: &str, role: &str) -> Result<()> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }

    pub fn open_thread(&self, event_id: &str) -> Result<()> {
        self.open_thread_feed(event_id)
    }

    pub fn close_thread(&self, event_id: &str) -> Result<()> {
        self.close_thread_feed(event_id)
    }

    pub fn open_author(&self, pubkey: &str) -> Result<()> {
        self.open_author_feed(pubkey)
    }

    pub fn close_author(&self, pubkey: &str) -> Result<()> {
        self.close_author_feed(pubkey)
    }

    pub fn claim_visible_author_profile(&self, pubkey: &str) -> Result<()> {
        // ADR-0063 (#1671 Lane F): a visible feed/list row author resolves at the
        // feed-avatar shape `profile.ref` and `CacheOk` liveness (no per-row
        // tailing sub). Origin-blind: the same path serves home/profile/thread/
        // mention authors. The slot dedupes per (namespace, key); Live/card from
        // the open profile pane upgrades it in place.
        self.with_visible_author_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_resolve_profile_ref(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    pub fn release_visible_author_profile(&self, pubkey: &str) -> Result<()> {
        self.with_visible_author_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_release_profile_ref(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    /// ADR-0063 (#1671 Lane F): resolve the OPEN profile pane's author at the
    /// full `profile.card` shape and `Live` liveness — the profile-screen demands
    /// every card field and wants reactive replacement on a fresh kind:0. Paired
    /// with [`Self::release_open_profile`] on pane close (D5: bounded by the open
    /// view). Uses a consumer id distinct from the feed-row visible-author claim.
    pub fn resolve_open_profile(&self, pubkey: &str) -> Result<()> {
        self.with_open_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_resolve_profile_card_live(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    pub fn release_open_profile(&self, pubkey: &str) -> Result<()> {
        self.with_open_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_release_profile_ref(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    pub fn claim_visible_note_relation_counts(&self, event_id: &str) -> Result<()> {
        self.dispatch_visible_note_relations("claim", event_id)
    }

    pub fn release_visible_note_relation_counts(&self, event_id: &str) -> Result<()> {
        self.dispatch_visible_note_relations("release", event_id)
    }

    pub fn publish_note(&self, content: &str, reply_to: Option<&ReplyTarget>) -> Result<String> {
        // Reconstruct the minimal NoteRecord the NIP-10 reply builder needs.
        // The home-feed projection carries the parent's author/content but not
        // its own Nip10Refs, so `refs` defaults to empty: the builder then
        // treats this parent as the thread root (correct for top-level replies,
        // best-effort for deep threads). The shared `publish_note_action` is
        // the single source of truth for the PublishRaw{kind:1} envelope and
        // the marked-form reply / `p` re-notification tags.
        let record = reply_to.map(|t| NoteRecord {
            event_id: t.id.clone(),
            author: t.author_pubkey.clone(),
            created_at: t.created_at,
            content: t.content.clone(),
            refs: Nip10Refs::default(),
        });
        let (namespace, action) = publish_note_action(content, record.as_ref())?;
        self.dispatch_action(&namespace, &action)
    }

    pub fn react(&self, event_id: &str, reaction: &str) -> Result<String> {
        let spec = react_spec(event_id, reaction);
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn repost(&self, event_id: &str, author_pubkey: &str) -> Result<String> {
        let spec = repost_spec(event_id, author_pubkey);
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn follow(&self, pubkey: &str, add: bool) -> Result<String> {
        let spec = if add {
            follow_spec(pubkey)
        } else {
            unfollow_spec(pubkey)
        };
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn ack_action_stage(&self, correlation_id: &str) -> Result<()> {
        self.with_cstr(correlation_id, |c| {
            nmp_ffi::nmp_app_ack_action_stage(self.app, c.as_ptr())
        })
    }

    pub fn chirp_load_older_timeline(&self) {
        let key = CString::new("nmp.feed.home").expect("static feed key has no NUL byte");
        nmp_app_load_older_feed(self.app, key.as_ptr());
    }

    pub fn dispatch_action_value(&self, namespace: &str, action: &Value) -> Result<String> {
        self.dispatch_action(namespace, &action.to_string())
    }

    pub(crate) fn app_ptr(&self) -> *mut NmpApp {
        self.app
    }

    /// Dispatch a Chirp action through the typed byte doorway.
    ///
    /// `action_json` is the canonical action body produced by a Chirp builder
    /// (`crate::action_specs` / a runtime `json!` body). The shared
    /// `nmp_app_chirp::dispatch_bytes` seam encodes it into the namespace's typed
    /// [`ActionPayload`] bytes, wraps it in a host-minted dispatch envelope, and
    /// calls `nmp_app_dispatch_action_bytes` — the JSON never crosses the FFI
    /// (ADR-0064 / Cut-B, #1756). The Marmot path is unaffected: it uses the
    /// native `MarmotHandle::dispatch` accessor, not this seam.
    pub(crate) fn dispatch_action(&self, namespace: &str, action_json: &str) -> Result<String> {
        nmp_app_chirp::dispatch_action_bytes_for(self.app, namespace, action_json)
    }

    pub(crate) fn with_cstr<T>(&self, value: &str, f: impl FnOnce(&CString) -> T) -> Result<T> {
        let c = CString::new(value).map_err(|_| "string contains NUL byte".to_string())?;
        Ok(f(&c))
    }

    fn with_visible_author_profile_args(
        &self,
        pubkey: &str,
        f: impl FnOnce(&CString, &CString),
    ) -> Result<()> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let consumer_id = visible_author_profile_consumer_id(pubkey)?;
        let pubkey = CString::new(pubkey).map_err(|_| "pubkey contains NUL byte".to_string())?;
        let consumer_id = CString::new(consumer_id)
            .map_err(|_| "profile consumer id contains NUL byte".to_string())?;
        f(&pubkey, &consumer_id);
        Ok(())
    }

    fn with_open_profile_args(
        &self,
        pubkey: &str,
        f: impl FnOnce(&CString, &CString),
    ) -> Result<()> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let consumer_id = open_profile_consumer_id(pubkey)?;
        let pubkey = CString::new(pubkey).map_err(|_| "pubkey contains NUL byte".to_string())?;
        let consumer_id = CString::new(consumer_id)
            .map_err(|_| "profile consumer id contains NUL byte".to_string())?;
        f(&pubkey, &consumer_id);
        Ok(())
    }

    fn dispatch_visible_note_relations(&self, op: &str, event_id: &str) -> Result<()> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let consumer_id = visible_note_relations_consumer_id(event_id)?;
        let action = json!({
            "op": op,
            "event_id": event_id,
            "consumer_id": consumer_id,
        });
        self.dispatch_action_value("nmp.nip01.visible_note_relations", &action)
            .map(|_| ())
    }
}

// The dispatch-result envelope parser moved into `nmp_app_chirp::dispatch_bytes`
// alongside the byte-doorway call that produces the envelope (ADR-0064 / Cut-B,
// #1756); it is unit-tested there. The TUI no longer owns a copy.

impl Drop for AppRuntime {
    fn drop(&mut self) {
        if !self.app.is_null() {
            self.close_all_feeds();
            bridge::unregister(self.app);
        }
        self.update_bridge.take();
        if !self.chirp.is_null() {
            nmp_app_chirp_unregister(self.chirp);
            self.chirp = ptr::null_mut();
        }
        if !self.discovery.get().is_null() {
            nmp_app_chirp_close_group_discovery(self.discovery.get());
            self.discovery.set(ptr::null_mut());
        }
        if !self.marmot.get().is_null() {
            nmp_marmot_unregister(self.marmot.get());
            self.marmot.set(ptr::null_mut());
        }
        if !self.app.is_null() {
            nmp_app_free(self.app);
            self.app = ptr::null_mut();
        }
    }
}

fn visible_author_profile_consumer_id(pubkey: &str) -> Result<String> {
    validate_hex64("pubkey", pubkey)?;
    Ok(format!("{VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX}:{pubkey}"))
}

fn open_profile_consumer_id(pubkey: &str) -> Result<String> {
    validate_hex64("pubkey", pubkey)?;
    Ok(format!("{OPEN_PROFILE_CONSUMER_PREFIX}:{pubkey}"))
}

fn visible_note_relations_consumer_id(event_id: &str) -> Result<String> {
    validate_hex64("event id", event_id)?;
    Ok(format!(
        "{VISIBLE_NOTE_RELATIONS_CONSUMER_PREFIX}:{event_id}"
    ))
}

fn validate_hex64(label: &str, value: &str) -> Result<()> {
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{label} must be 64 hex characters"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const EVENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn visible_author_profile_consumer_id_is_stable() {
        assert_eq!(
            visible_author_profile_consumer_id(ALICE).unwrap(),
            format!("{VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX}:{ALICE}")
        );
    }

    #[test]
    fn visible_author_profile_claims_reject_invalid_pubkeys() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(
            runtime.claim_visible_author_profile("not-a-pubkey"),
            Err("pubkey must be 64 hex characters".to_string())
        );
        assert_eq!(
            runtime.release_visible_author_profile("not-a-pubkey"),
            Err("pubkey must be 64 hex characters".to_string())
        );
    }

    #[test]
    fn visible_author_profile_claim_release_are_idempotent() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(runtime.claim_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.claim_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.release_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.release_visible_author_profile(ALICE), Ok(()));
    }

    #[test]
    fn note_relation_count_claim_release_are_idempotent() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(runtime.claim_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(runtime.claim_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(runtime.release_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(runtime.release_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(
            runtime.claim_visible_note_relation_counts("bad"),
            Err("event id must be 64 hex characters".to_string())
        );
    }

    #[test]
    fn dispatch_envelope_requires_correlation_id_or_error() {
        // The parser now lives in the shared `nmp_app_chirp::dispatch_bytes` seam
        // (ADR-0064 / Cut-B, #1756); assert through its public re-export so the
        // TUI keeps a smoke check on the contract it depends on.
        use nmp_app_chirp::parse_dispatch_envelope;
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"correlation_id": "abc"})),
            Ok("abc".to_string())
        );
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"error": "bad action"})),
            Err("bad action".to_string())
        );
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"ok": true})),
            Err("action dispatch envelope missing correlation_id".to_string())
        );
    }
}
