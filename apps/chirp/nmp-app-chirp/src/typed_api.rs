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
use serde_json::{json, Value};
use nmp_ffi::{nmp_app_dispatch_action, nmp_app_free_string, NmpApp};

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

        let namespace =
            CString::new(namespace).map_err(|_| "action namespace contains NUL byte".to_string())?;
        let action = CString::new(action_json)
            .map_err(|_| "action JSON contains NUL byte".to_string())?;

        // SAFETY: `app` is a valid, non-null pointer. FFI always returns a
        // valid (non-null) JSON string for a valid app (D6).
        // nmp_app_dispatch_action and nmp_app_free_string are not marked as `unsafe`
        // because FFI boilerplate automatically dereferences raw pointers internally.
        let ptr = nmp_app_dispatch_action(self.app, namespace.as_ptr(), action.as_ptr());

        if ptr.is_null() {
            return Err("action dispatch returned null".to_string());
        }

        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        // FFI allocated this; we must free it.
        nmp_app_free_string(ptr);

        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("action dispatch returned invalid JSON: {e}"))?;

        parse_dispatch_envelope(&value)
    }

    // ── Social actions ─────────────────────────────────────────────────

    /// Publish a note.
    ///
    /// Returns the correlation ID on success; error if the action was rejected
    /// by the action registry.
    pub fn publish_note(&self, content: &str, reply_to_id: Option<&str>) -> Result<String, String> {
        let (namespace, action) = publish_note_action(content, reply_to_id);
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
        let (namespace, action) = zap_action(recipient_pubkey, amount_msats, target_event_id, comment);
        self.dispatch_action(&namespace, &action)
    }

    // ── Account lifecycle ──────────────────────────────────────────────

    /// Create a new account with the given profile metadata and relay list.
    ///
    /// `relays` is a list of `(url, role)` tuples, where role is typically
    /// `"read"`, `"write"`, or `"read+write"`.
    pub fn create_account(
        &self,
        name: &str,
        about: &str,
        picture: &str,
        relays: &[(&str, &str)],
    ) -> Result<String, String> {
        let (namespace, action) = create_account_action(name, about, picture, relays);
        self.dispatch_action(&namespace, &action)
    }

    /// Sign in with an nsec (secret key in Nostr format).
    pub fn sign_in_nsec(&self, secret: &str) -> Result<String, String> {
        let (namespace, action) = sign_in_nsec_action(secret);
        self.dispatch_action(&namespace, &action)
    }

    /// Switch the active account to the given pubkey.
    pub fn switch_account(&self, pubkey: &str) -> Result<String, String> {
        let (namespace, action) = switch_account_action(pubkey);
        self.dispatch_action(&namespace, &action)
    }

    /// Remove an account by pubkey (deletes it from the keyring).
    pub fn remove_account(&self, pubkey: &str) -> Result<String, String> {
        let (namespace, action) = remove_account_action(pubkey);
        self.dispatch_action(&namespace, &action)
    }

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

/// Build a PublishNote action envelope.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn publish_note_action(content: &str, reply_to_id: Option<&str>) -> (String, String) {
    let action = json!({
        "PublishNote": {
            "content": content,
            "reply_to_id": reply_to_id,
            "target": "Auto"
        }
    })
    .to_string();
    ("nmp.publish".to_string(), action)
}

/// Build a React (like/repost) action envelope.
///
/// `reaction` is a single character or emoji string (commonly "+" for like,
/// "🔄" for repost, etc.).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn react_action(event_id: &str, reaction: &str) -> (String, String) {
    let action = json!({
        "target_event_id": event_id,
        "reaction": reaction
    })
    .to_string();
    ("nmp.nip25.react".to_string(), action)
}

/// Build a Follow action envelope (add to contacts list).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn follow_action(pubkey: &str) -> (String, String) {
    let action = json!({ "pubkey": pubkey }).to_string();
    ("nmp.follow".to_string(), action)
}

/// Build an Unfollow action envelope (remove from contacts list).
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn unfollow_action(pubkey: &str) -> (String, String) {
    let action = json!({ "pubkey": pubkey }).to_string();
    ("nmp.unfollow".to_string(), action)
}

/// Build a SendDM (NIP-17 private encrypted message) action envelope.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn send_dm_action(recipient_pubkey: &str, content: &str) -> (String, String) {
    let action = json!({
        "recipient_pubkey": recipient_pubkey,
        "content": content,
    })
    .to_string();
    ("nmp.nip17.send".to_string(), action)
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
    let action = json!({
        "recipient_pubkey": recipient_pubkey,
        "amount_msats": amount_msats,
        "target_event_id": target_event_id,
        "comment": comment
    })
    .to_string();
    ("nmp.nip57.zap".to_string(), action)
}

/// Build a CreateAccount action envelope.
///
/// `relays` is a list of `(url, role)` tuples, where role is typically
/// `"read"`, `"write"`, or `"read+write"`.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn create_account_action(
    name: &str,
    about: &str,
    picture: &str,
    relays: &[(&str, &str)],
) -> (String, String) {
    let mut profile_fields = serde_json::Map::new();
    if !name.is_empty() {
        profile_fields.insert("name".to_string(), Value::String(name.to_string()));
    }
    if !about.is_empty() {
        profile_fields.insert("about".to_string(), Value::String(about.to_string()));
    }
    if !picture.is_empty() {
        profile_fields.insert("picture".to_string(), Value::String(picture.to_string()));
    }

    let relays_json: Vec<serde_json::Value> = relays
        .iter()
        .map(|(url, role)| json!({ "url": url, "role": role }))
        .collect();

    let action = json!({
        "CreateAccount": {
            "profile": Value::Object(profile_fields),
            "relays": relays_json,
            "mls": false
        }
    })
    .to_string();
    ("nmp.create_account".to_string(), action)
}

/// Build a SignInNsec action envelope.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn sign_in_nsec_action(secret: &str) -> (String, String) {
    let action = json!({ "SignInNsec": { "secret": secret } }).to_string();
    ("nmp.sign_in_nsec".to_string(), action)
}

/// Build a SwitchAccount action envelope.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn switch_account_action(pubkey: &str) -> (String, String) {
    let action = json!({ "pubkey": pubkey }).to_string();
    ("nmp.switch_account".to_string(), action)
}

/// Build a RemoveAccount action envelope.
///
/// Returns `(namespace, action_json)` suitable for passing to `dispatch_action`.
pub fn remove_account_action(pubkey: &str) -> (String, String) {
    let action = json!({ "pubkey": pubkey }).to_string();
    ("nmp.remove_account".to_string(), action)
}

/// Build a PublishProfile action envelope.
///
/// Returns the action JSON string (call with "nmp.publish" namespace).
pub fn publish_profile_action(name: &str, about: &str, picture: &str) -> String {
    let mut fields = serde_json::Map::new();
    if !name.is_empty() {
        fields.insert("name".to_string(), Value::String(name.to_string()));
    }
    if !about.is_empty() {
        fields.insert("about".to_string(), Value::String(about.to_string()));
    }
    if !picture.is_empty() {
        fields.insert("picture".to_string(), Value::String(picture.to_string()));
    }
    json!({ "PublishProfile": { "fields": Value::Object(fields) } }).to_string()
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
        assert_eq!(
            parse_dispatch_envelope(&value),
            Ok("abc123".to_string())
        );
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
