//! `Placeholder<T>` — always-renderable display field wrapper (D1).
//!
//! # Doctrine
//!
//! D1 (best-effort rendering) requires that every display field on a view
//! payload carries a value at all times — there is no `None`, no "loading"
//! gate, and no empty string that forces the UI to branch on optionality.
//! When authoritative data (e.g. a kind:0 profile picture URL) has not yet
//! arrived, the field carries a deterministic placeholder value derived from
//! the pubkey, so the UI can render immediately without special-casing.
//!
//! # Design (ADR-0017)
//!
//! `Placeholder<T>` is a zero-cost newtype over `T`.  It:
//!
//! - Serialises as the inner `T` (bare string, not a tagged enum) so the FFI
//!   JSON wire format is unchanged — Swift `String?` fields decode `String`
//!   without modification.
//! - Implements `Display`, `Deref<Target = T>`, and `AsRef<str>` (when
//!   `T: AsRef<str>`) so callers can use it transparently wherever `T` is
//!   expected.
//! - Does **not** carry a `Pending`/`Authoritative` tag at this layer; the
//!   `author_avatar_source` field (`"placeholder"` vs `"kind0"`) on
//!   `TimelineItem` is the discriminator.  Adding a variant here would
//!   duplicate that signal across the wire format for no gain.
//!
//! # Picture URL placeholders
//!
//! For picture URL fields the placeholder is an opaque URI of the form
//! `identicon:<pubkey>`.  This is:
//!
//! - **Deterministic**: same pubkey always produces the same placeholder,
//!   so diffing-based renderers never see spurious updates.
//! - **Detectable**: the `identicon:` scheme prefix lets the UI decide to
//!   show avatar initials + color instead of attempting a network fetch.
//! - **Non-empty**: satisfies D1's "no empty string" invariant.
//!
//! Helper: [`picture_placeholder`].

use serde::Serialize;
use std::fmt;
use std::ops::Deref;

/// A wrapper that makes a display field always renderable (D1).
///
/// Construct via [`From<T>`] or one of the module-level helpers.
/// Serialises transparently as the inner `T`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Placeholder<T>(pub T);

impl<T: fmt::Display> fmt::Display for Placeholder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T> Deref for Placeholder<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: AsRef<str>> AsRef<str> for Placeholder<T> {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl<T> From<T> for Placeholder<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

/// Build a deterministic picture-URL placeholder for a pubkey.
///
/// Returns `"identicon:<first16-hex-chars-of-pubkey>"`.  The `identicon:`
/// scheme is detectable by the UI to render avatar initials + color instead
/// of a network image fetch.
#[must_use]
pub fn picture_placeholder(pubkey: &str) -> String {
    // Char-based truncation: pubkeys are ASCII hex in practice, but slicing on
    // a raw byte index would panic on a non-char-boundary if a non-ASCII string
    // ever crosses this public helper. `take(16)` is byte-equivalent for ASCII.
    let prefix: String = pubkey.chars().take(16).collect();
    format!("identicon:{prefix}")
}
