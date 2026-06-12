//! Test-support facade for the NIP golden-tag conformance suite.
//!
//! V-38: the wallet runtime moved to `nmp-nip47`. The harness no longer
//! drives `wallet_connect` — that test moved to `crates/nmp-nip47/tests/`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::identity::{add_signer, create_account, IdentityRuntime};
use super::publish::{follow, publish_unsigned_event, react};
use crate::actor::SignerSource;
use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::{OutboundMessage, DEFAULT_VISIBLE_LIMIT};
use crate::substrate::UnsignedEvent;
use crate::tags::{e_tag, p_tag};

/// A real `Kernel` + `IdentityRuntime` driven by the actual command
/// handlers — no mocks. Each `emit_*` method runs a command and returns the
/// emitted `["EVENT", {...}]` JSON object so a conformance test can assert
/// on the `tags` array.
pub struct ConformanceHarness {
    identity: IdentityRuntime,
    kernel: Kernel,
    /// The same `InMemoryPublishStore` the kernel was constructed with. Kept
    /// here so [`Self::published_event_of_kind`] can read back the full signed
    /// event (tags included) for kinds the kernel routes through
    /// `publish_signed` without surfacing the outbound frame to the caller
    /// (kind:0 / kind:10002 emitted by `create_account`).
    publish_store: Arc<InMemoryPublishStore>,
}

impl Default for ConformanceHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl ConformanceHarness {
    /// A fresh, signed-out harness.
    #[must_use]
    pub fn new() -> Self {
        let publish_store = Arc::new(InMemoryPublishStore::new());
        Self {
            // D0: NIP-46 remote signing is an app noun — the conformance
            // harness wires a private throwaway bunker-handshake slot (no host
            // reads it). V-14 step b: likewise a throwaway connection-state slot.
            identity: IdentityRuntime::new(
                super::new_bunker_handshake_slot(),
                super::new_signer_state_slot(),
            ),
            kernel: Kernel::with_publish_store(
                DEFAULT_VISIBLE_LIMIT,
                Arc::clone(&publish_store) as Arc<dyn PublishStore>,
            ),
            publish_store,
        }
    }

    /// Sign in with `nsec` and seed a kind:10002 NIP-65 write-relay list for
    /// the active account so the (fail-closed) outbox resolver has targets and
    /// publish commands produce non-empty outbound frames.
    pub fn sign_in_and_seed_nip65(&mut self, nsec: &str, write_relays: &[&str]) {
        add_signer(
            &mut self.identity,
            &mut self.kernel,
            SignerSource::LocalNsec(zeroize::Zeroizing::new(nsec.to_string())),
            true,
            false,
        );
        let pubkey = self
            .identity
            .active_pubkey()
            .expect("active account after sign-in");
        self.kernel.seed_kind10002_for_test(&pubkey, write_relays);
    }

    /// The active account's pubkey hex, if signed in.
    pub fn active_pubkey(&self) -> Option<String> {
        self.identity.active_pubkey()
    }

    /// The last user-visible error toast, if any.
    pub fn last_error_toast(&self) -> Option<String> {
        self.kernel.last_error_toast_snapshot().cloned()
    }

    /// Seed a kind:1 note into the kernel read-cache so a subsequent
    /// `emit_reaction` against `id` exercises the warm path (`event_author`)
    /// rather than the cold fallback. `tags` carries whatever NIP-10 structure
    /// the test needs.
    pub fn seed_note(&mut self, id: &str, author: &str, tags: Vec<Vec<String>>) {
        self.kernel
            .seed_kind1_for_reply_test(id, author, 100, tags, "seeded note");
    }

    /// Seed an existing kind:3 contact list for `author` so a subsequent
    /// `emit_follow` mutates that list rather than starting from empty.
    pub fn seed_contact_list(&mut self, author: &str, follows: &[&str]) {
        let p_tags: Vec<Vec<String>> = follows
            .iter()
            .map(|p| vec!["p".to_string(), (*p).to_string()])
            .collect();
        self.kernel.inject_replaceable_event(
            &"3".repeat(64),
            author,
            1_700_000_000,
            3,
            p_tags,
            "wss://conformance-seed.test",
            1,
        );
    }

    /// Drive `react` (kind:7). Returns the emitted `EVENT` JSON object.
    pub fn emit_reaction(&mut self, target_event_id: &str, reaction: &str) -> Value {
        let outbound = react(
            &self.identity,
            &mut self.kernel,
            target_event_id,
            reaction,
            None,
            &mut Vec::new(),
        );
        last_event_json(&outbound)
    }

    /// Drive `follow` (kind:3 add/remove). Returns the emitted `EVENT` JSON.
    pub fn emit_follow(&mut self, pubkey: &str, add: bool) -> Value {
        let outbound = follow(
            &self.identity,
            &mut self.kernel,
            pubkey,
            add,
            None,
            &mut Vec::new(),
        );
        last_event_json(&outbound)
    }


    /// Drive a kind:1 short-text note publish. Returns the emitted `EVENT` JSON
    /// object. When `reply_to` is `Some(parent_id)` the note is built as a
    /// NIP-10 marked-form reply: the parent event is looked up from the kernel
    /// read-cache (via `seed_note`) and the reply carries `e`(root) + `e`(reply)
    /// markers plus a `p` tag re-notifying the parent's author.
    ///
    /// When `reply_to` is `None` the note is a plain top-level kind:1 with no
    /// `e` or `p` tags (NIP-01 §short-text-note).
    pub fn emit_note(&mut self, content: &str, reply_to: Option<&str>) -> Value {
        let mut tags: Vec<Vec<String>> = Vec::new();
        if let Some(parent_id) = reply_to {
            // NIP-10 marked form: both root and reply point at the parent when
            // the parent itself is the thread root (no root ref in its tags).
            // The conformance test always seeds a root-level parent via
            // `seed_note(id, author, vec![])`, so this holds in all test cases.
            tags.push(e_tag(parent_id, None, Some("root")));
            tags.push(e_tag(parent_id, None, Some("reply")));
            if let Some(author) = self.kernel.event_author(parent_id) {
                tags.push(p_tag(&author, None));
            }
        }
        let unsigned = UnsignedEvent {
            pubkey: String::new(),
            kind: 1,
            tags,
            content: content.to_string(),
            created_at: self.kernel.now_secs(),
        };
        let outbound = publish_unsigned_event(
            &self.identity,
            &mut self.kernel,
            unsigned,
            None,
            None,
            &mut Vec::new(),
        );
        last_event_json(&outbound)
    }

    /// Drive `publish_unsigned_event` for an arbitrary kind (used for kind:0
    /// metadata, which has no dedicated command handler). Returns the emitted
    /// `EVENT` JSON object.
    pub fn emit_unsigned(&mut self, kind: u32, tags: Vec<Vec<String>>, content: &str) -> Value {
        let unsigned = UnsignedEvent {
            pubkey: "ignored-by-signer".to_string(),
            kind,
            tags,
            content: content.to_string(),
            created_at: 1_700_000_000,
        };
        // Conformance harness is non-dispatch — `None` keeps the engine's
        // `correlation_id_override` `None`-fallback (the publish handle == event
        // id is reported in `action_results`), preserving prior behaviour.
        let outbound = publish_unsigned_event(
            &self.identity,
            &mut self.kernel,
            unsigned,
            None,
            // Conformance harness signs with the active account.
            None,
            &mut Vec::new(),
        );
        last_event_json(&outbound)
    }

    /// Drive `create_account`, which emits kind:0 (metadata), kind:10002
    /// (relay list) and kind:3 (initial follows). The kernel routes those
    /// through `publish_signed` directly, so the emitted EVENT frames land in
    /// the publish queue rather than the returned outbound vec — callers read
    /// them back via [`Self::published_event_of_kind`].
    pub fn create_account(
        &mut self,
        profile: HashMap<String, String>,
        relays: &[(String, String)],
    ) {
        create_account(
            &mut self.identity,
            &mut self.kernel,
            false,
            &profile,
            relays,
            false,
            true,
        );
    }

    /// The published event of `kind` reconstructed as an `EVENT`-style JSON
    /// object (`id`/`pubkey`/`kind`/`tags`/`content`), read back from the
    /// kernel's publish store. Used for the kinds `create_account` routes
    /// through `publish_signed` without returning the outbound frame
    /// (kind:0 metadata, kind:10002 relay list). `None` if no such kind was
    /// published.
    pub fn published_event_of_kind(&self, kind: u32) -> Option<Value> {
        let records = self.publish_store.load_pending().ok()?;
        records
            .into_iter()
            .map(|r| r.event)
            .find(|ev| ev.unsigned.kind == kind)
            .map(|ev| {
                serde_json::json!({
                    "id": ev.id,
                    "pubkey": ev.unsigned.pubkey,
                    "kind": ev.unsigned.kind,
                    "tags": ev.unsigned.tags,
                    "content": ev.unsigned.content,
                })
            })
    }
}

/// Extract the most recent `["EVENT", {...}]` frame from a set of outbound
/// messages and return the inner event object. Panics with a clear message if
/// no EVENT frame is present (the test wants one — a missing frame is a bug).
fn last_event_json(outbound: &[OutboundMessage]) -> Value {
    let frame = outbound
        .iter()
        .rev()
        .find(|m| m.text.starts_with("[\"EVENT\""))
        .expect("expected at least one outbound EVENT frame");
    parse_event_frame(&frame.text)
}

/// Parse a `["EVENT", <event>]` wire frame into the inner event object.
fn parse_event_frame(text: &str) -> Value {
    let parsed: Value = serde_json::from_str(text).expect("EVENT frame is valid JSON");
    parsed
        .as_array()
        .and_then(|arr| arr.get(1).cloned())
        .expect("EVENT frame shape is [\"EVENT\", <event>]")
}
