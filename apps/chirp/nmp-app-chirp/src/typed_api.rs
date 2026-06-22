//! Typed Rust client API for Chirp actions.
//!
//! Provides a high-level [`ChirpClient`] struct that wraps the low-level
//! `nmp_app_dispatch_action` FFI function, constructing properly-typed JSON
//! action envelopes for common Chirp operations. Shells (TUI, desktop, Android,
//! iOS) call typed methods like `publish_note()` instead of manually building
//! JSON and calling `dispatch_action` directly.
//!
//! All methods return a [`Result<String, String>`] where success yields the
//! action's correlation ID (a stable opaque identifier for correlating with
//! action_stages snapshot projections), and error yields an error message.
//!
//! Pure action envelope builders are also exported as free functions, allowing
//! tests and code to construct action JSON without a live kernel instance.

use std::ffi::{CStr, CString};

use serde_json::Value;

use crate::action_specs::{
    follow_spec, publish_note_spec, publish_profile_spec, publish_relay_list_spec, react_spec,
    repost_spec, send_dm_spec, unfollow_spec, zap_spec,
};
use nmp_ffi::{nmp_app_dispatch_action, nmp_free_string, NmpApp};
use nmp_nip01::NoteRecord;

/// Typed Chirp action client.
///
/// Wraps the raw `nmp_app_dispatch_action` FFI symbol and owns the task of
/// constructing well-formed action JSON. Shells create one per app lifecycle
/// and call typed methods instead of manually building JSON.
///
/// # Thread safety
///
/// [`ChirpClient`] holds a raw pointer to [`NmpApp`], which is thread-safe
/// internally (all mutations go through the actor channel). The client itself
/// is [`Send`] and [`Sync`] because:
///
/// 1. The `app` pointer is valid for the entire lifetime of the client.
/// 2. All dispatch calls are non-blocking (they just enqueue an [`ActorCommand`]).
/// 3. No mutable state is stored; the client is a transparent pass-through.
#[derive(Clone, Copy)]
pub struct ChirpClient {
    app: *mut NmpApp,
}

// SAFETY: The app pointer is valid and owned by the runtime. All access is
// through thread-safe FFI calls. The client is a simple wrapper.
unsafe impl Send for ChirpClient {}
unsafe impl Sync for ChirpClient {}

impl ChirpClient {
    /// Create a new client from an `NmpApp` pointer.
    ///
    /// # Safety
    ///
    /// `app` must be a valid, non-null pointer from [`nmp_ffi::nmp_app_new`].
    pub const fn new(app: *mut NmpApp) -> Self {
        Self { app }
    }

    /// Dispatch a raw action JSON through the action registry.
    ///
    /// This is the low-level method underlying all typed action methods.
    /// Callers construct the JSON themselves; for common actions, prefer
    /// the typed methods below (e.g., `publish_note`, `react`, etc.).
    ///
    /// Returns the action's correlation ID (a stable opaque identifier for
    /// the shell to correlate against `action_stages` projections), or an
    /// error message if the action was rejected.
    fn dispatch_action(&self, namespace: &str, action_json: &str) -> Result<String, String> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }

        let namespace = CString::new(namespace)
            .map_err(|_| "action namespace contains NUL byte".to_string())?;
        let action =
            CString::new(action_json).map_err(|_| "action JSON contains NUL byte".to_string())?;

        // SAFETY: `app` is a valid, non-null pointer. FFI always returns a
        // valid (non-null) JSON string for a valid app (D6).
        // nmp_app_dispatch_action and nmp_free_string are not marked as `unsafe`
        // because FFI boilerplate automatically dereferences raw pointers internally.
        let ptr = nmp_app_dispatch_action(self.app, namespace.as_ptr(), action.as_ptr());

        if ptr.is_null() {
            return Err("action dispatch returned null".to_string());
        }

        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        // FFI allocated this; we must free it.
        nmp_free_string(ptr);

        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("action dispatch returned invalid JSON: {e}"))?;

        parse_dispatch_envelope(&value)
    }

    // ── Social actions ─────────────────────────────────────────────────

    /// Publish a note.
    ///
    /// Returns the correlation ID on success; error if the action was rejected
    /// by the action registry.
    pub fn publish_note(
        &self,
        content: &str,
        reply_to: Option<&NoteRecord>,
    ) -> Result<String, String> {
        let (namespace, action) = publish_note_action(content, reply_to)?;
        self.dispatch_action(&namespace, &action)
    }

    /// React to (e.g., like/repost) an event.
    ///
    /// `reaction` is a single character or emoji string (commonly "+" for
    /// like, "🔄" for repost, etc.).
    pub fn react(&self, event_id: &str, reaction: &str) -> Result<String, String> {
        let (namespace, action) = react_action(event_id, reaction);
        self.dispatch_action(&namespace, &action)
    }

    /// Follow a user by pubkey (add to contacts list).
    pub fn follow(&self, pubkey: &str) -> Result<String, String> {
        let (namespace, action) = follow_action(pubkey);
        self.dispatch_action(&namespace, &action)
    }

    /// Unfollow a user by pubkey (remove from contacts list).
    pub fn unfollow(&self, pubkey: &str) -> Result<String, String> {
        let (namespace, action) = unfollow_action(pubkey);
        self.dispatch_action(&namespace, &action)
    }

    /// Repost (kind:6) an event.
    pub fn repost(&self, event_id: &str, author_pubkey: &str) -> Result<String, String> {
        let (namespace, action) = repost_action(event_id, author_pubkey);
        self.dispatch_action(&namespace, &action)
    }

    /// Publish the user's NIP-65 relay list (kind:10002).
    ///
    /// `relays` is a list of `(url, role)` pairs from the host's relay-config
    /// UI; URL canonicalisation and `wss://` gating happen kernel-side.
    pub fn publish_relay_list(&self, relays: &[(&str, &str)]) -> Result<String, String> {
        let (namespace, action) = publish_relay_list_action(relays);
        self.dispatch_action(&namespace, &action)
    }

    /// Send a direct message (NIP-17 private encrypted message).
    pub fn send_dm(&self, recipient_pubkey: &str, content: &str) -> Result<String, String> {
        let (namespace, action) = send_dm_action(recipient_pubkey, content);
        self.dispatch_action(&namespace, &action)
    }

    /// Zap (send sats to) an event or user.
    ///
    /// `amount_msats` is in millisatoshis (divide by 1000 for sats).
    /// `comment` is an optional note to attach to the zap.
    /// `target_event_id` is the event being zapped (or the user's profile
    /// event ID if zapping a user directly).
    pub fn zap(
        &self,
        recipient_pubkey: &str,
        amount_msats: u64,
        target_event_id: &str,
        comment: &str,
    ) -> Result<String, String> {
        let (namespace, action) =
            zap_action(recipient_pubkey, amount_msats, target_event_id, comment);
        self.dispatch_action(&namespace, &action)
    }

    // ── Account lifecycle ──────────────────────────────────────────────

    /// Publish profile metadata (name, about, picture).
    pub fn publish_profile(
        &self,
        name: &str,
        about: &str,
        picture: &str,
    ) -> Result<String, String> {
        let action = publish_profile_action(name, about, picture);
        self.dispatch_action("nmp.publish", &action)
    }
}

// ── Pure action envelope builders (no app pointer required) ─────────────────

/// Build a kind:1 note publish action envelope (`PublishRaw`).
///
/// The NIP-10 tag set is produced by the canonical `nmp_nip01::Note` builder
/// (single source of truth for reply construction across all protocol crates),
/// not hand-assembled here. For a reply it emits the marked-form `root` + `reply`
/// `e` tags and the `p` re-notification tags (parent author first, then the
/// parent's `mentioned_pubkeys`, de-duplicated); a root note carries no tags.
/// `target` defaults to `Auto` (NIP-65 outbox).
///
/// The author/`created_at` slots on the builder's `UnsignedEvent` are discarded:
/// `PublishRaw` is signer- and clock-free at the call site — the actor stamps
/// `pubkey` from the active signer and `created_at` per D7. We harvest only the
/// `tags`. The `p`-tag pubkeys come from the `NoteRecord`'s `author` /
/// `mentioned_pubkeys` fields, so they are correct regardless of the empty
/// build-time author.
///
/// # Errors
///
/// Returns the `NoteBuildError` display string when `content` is blank (D6 —
/// errors never cross as panics).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn publish_note_action(
    content: &str,
    reply_to: Option<&NoteRecord>,
) -> Result<(String, String), String> {
    publish_note_spec(content, reply_to).map(|spec| spec.into_tuple())
}

/// Build a React (like/repost) action envelope.
///
/// `reaction` is a single character or emoji string (commonly "+" for like,
/// "🔄" for repost, etc.).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn react_action(event_id: &str, reaction: &str) -> (String, String) {
    react_spec(event_id, reaction).into_tuple()
}

/// Build a Follow action envelope (add to contacts list).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn follow_action(pubkey: &str) -> (String, String) {
    follow_spec(pubkey).into_tuple()
}

/// Build an Unfollow action envelope (remove from contacts list).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn unfollow_action(pubkey: &str) -> (String, String) {
    unfollow_spec(pubkey).into_tuple()
}

/// Build a SendDM (NIP-17 private encrypted message) action envelope.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn send_dm_action(recipient_pubkey: &str, content: &str) -> (String, String) {
    send_dm_spec(recipient_pubkey, content, None).into_tuple()
}

/// Build a kind:6 repost action envelope (`PublishRaw`).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn repost_action(event_id: &str, author_pubkey: &str) -> (String, String) {
    repost_spec(event_id, author_pubkey).into_tuple()
}

/// Build a NIP-65 relay-list publish action envelope (kind:10002).
///
/// `relays` is a list of `(url, role)` pairs. Returns `(namespace, action_json)`
/// suitable for passing to `dispatch_action`.
#[must_use]
pub fn publish_relay_list_action(relays: &[(&str, &str)]) -> (String, String) {
    publish_relay_list_spec(relays).into_tuple()
}

/// Build a Zap action envelope (send sats to an event or user).
///
/// `amount_msats` is in millisatoshis (divide by 1000 for sats).
/// `comment` is an optional note to attach to the zap.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn zap_action(
    recipient_pubkey: &str,
    amount_msats: u64,
    target_event_id: &str,
    comment: &str,
) -> (String, String) {
    zap_spec(
        recipient_pubkey,
        amount_msats,
        Some(target_event_id),
        Some(comment),
        None,
        Vec::new(),
    )
    .into_tuple()
}

/// Build a PublishProfile action envelope.
///
/// Returns the action JSON string (call with "nmp.publish" namespace).
pub fn publish_profile_action(name: &str, about: &str, picture: &str) -> String {
    publish_profile_spec(name, about, picture).body_json
}

/// Parse a dispatch result envelope.
///
/// The FFI `nmp_app_dispatch_action` returns JSON in one of two forms:
/// - `{"correlation_id":"<32-hex>"}` on success (the action was accepted)
/// - `{"error":"<message>"}` on rejection (validation failed, namespace unknown, etc.)
///
/// This function returns the correlation ID or an error string.
fn parse_dispatch_envelope(value: &Value) -> Result<String, String> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    value
        .get("correlation_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "action dispatch envelope missing correlation_id".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dispatch_envelope_success() {
        let value = serde_json::json!({"correlation_id": "abc123"});
        assert_eq!(parse_dispatch_envelope(&value), Ok("abc123".to_string()));
    }

    #[test]
    fn parse_dispatch_envelope_error() {
        let value = serde_json::json!({"error": "bad action"});
        assert_eq!(
            parse_dispatch_envelope(&value),
            Err("bad action".to_string())
        );
    }

    #[test]
    fn parse_dispatch_envelope_missing_correlation_id() {
        let value = serde_json::json!({"ok": true});
        assert_eq!(
            parse_dispatch_envelope(&value),
            Err("action dispatch envelope missing correlation_id".to_string())
        );
    }

    #[test]
    fn parse_dispatch_envelope_empty_correlation_id() {
        let value = serde_json::json!({"correlation_id": ""});
        assert_eq!(
            parse_dispatch_envelope(&value),
            Err("action dispatch envelope missing correlation_id".to_string())
        );
    }
}
