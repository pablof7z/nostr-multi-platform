//! Host-declared **consumed-projection set** (ADR-0053).
//!
//! The output-side sibling of the relay `push_interest` lattice: a host declares,
//! once at app init, the static set of snapshot **projection keys it consumes**.
//! The kernel uses it to gate the Tier-2 kernel-owned built-ins
//! ([`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS))
//! so it serializes only what some screen of the app can read.
//!
//! ## Why it lives on the `SnapshotRegistry`
//!
//! The registry is already the single `Arc<Mutex<…>>` slot shared between the host
//! (registration side) and the actor-thread kernel (`make_update` read side), and it
//! already survives `Reset`. Parking the declared set here means no new shared slot,
//! no new actor parameter, and no new Reset-survival contract — the kernel reads the
//! set on the same lock it already takes once per tick.
//!
//! ## Semantics (ADR-0053 Decision 4) — empty = no narrowing
//!
//! An **empty** declared set means the host expressed *no opinion*: every Tier-2
//! built-in is emitted (the pre-ADR-0053 behaviour). This is the relay interest-set
//! semantic — an empty filter set does not subscribe to nothing; narrowing is
//! additive. A **non-empty** set narrows: only its members are emitted; every other
//! Tier-2 built-in is skipped (its producer is never run). This keeps the kernel's
//! own Rust consumers (chirp-tui, chirp-desktop) and the test helpers working with no
//! declaration, while every app that declares a set opts into the optimization.
//!
//! Tier-1 host/protocol projections (`SnapshotRegistry::register*`) are **not** gated
//! here — they already self-gate by registration (registration *is* the declaration),
//! and the dynamic per-view feeds gate by their `remove()`-on-close lifecycle.

use std::collections::BTreeSet;

/// The host-declared set of consumed Tier-2 built-in projection keys.
///
/// `BTreeSet` for deterministic iteration and cheap membership; the set is tiny
/// (≤ the count of [`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS),
/// today 18). Declarations are **additive** (union) — a host may call the declare
/// seam more than once (e.g. a base set from `nmp-defaults` plus an app-specific
/// extension) and the sets union.
#[derive(Debug, Default, Clone)]
pub struct DeclaredProjections {
    keys: BTreeSet<String>,
}

impl DeclaredProjections {
    /// Construct an empty declared set — the "no opinion / no narrowing" state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Union `keys` into the declared set (additive; idempotent per key).
    pub fn declare<I, K>(&mut self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.keys.extend(keys.into_iter().map(Into::into));
    }

    /// `true` when the host has declared at least one key (i.e. narrowing is in
    /// effect). An empty set returns `false` — the "no narrowing" state.
    #[must_use]
    pub fn is_narrowing(&self) -> bool {
        !self.keys.is_empty()
    }

    /// Whether the Tier-2 built-in `key` should be emitted this frame.
    ///
    /// ADR-0053 Decision 4: an empty declared set emits everything (no narrowing);
    /// a non-empty set emits `key` iff it is a declared member.
    #[must_use]
    pub fn permits(&self, key: &str) -> bool {
        self.keys.is_empty() || self.keys.contains(key)
    }

    /// Read-only view of the declared keys (test/introspection).
    #[must_use]
    pub fn keys(&self) -> &BTreeSet<String> {
        &self.keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_permits_everything() {
        let d = DeclaredProjections::new();
        assert!(!d.is_narrowing());
        assert!(d.permits("relay_diagnostics"));
        assert!(d.permits("anything_at_all"));
    }

    #[test]
    fn non_empty_set_narrows_to_members() {
        let mut d = DeclaredProjections::new();
        d.declare(["profile", "accounts"]);
        assert!(d.is_narrowing());
        assert!(d.permits("profile"));
        assert!(d.permits("accounts"));
        assert!(!d.permits("relay_diagnostics"));
    }

    #[test]
    fn declarations_are_additive() {
        let mut d = DeclaredProjections::new();
        d.declare(["profile"]);
        d.declare(["accounts", "profile"]);
        assert_eq!(d.keys().len(), 2);
        assert!(d.permits("profile"));
        assert!(d.permits("accounts"));
    }
}
