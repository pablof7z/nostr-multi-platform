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

    /// **Workstream-E3 / ADR-0053 drift gate** — the declared keys that are
    /// absent from `decodable`, the framework's authoritative emittable/decodable
    /// Tier-2 key set
    /// ([`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS)).
    ///
    /// A non-empty result is **drift**: the host declared a key the kernel never
    /// emits — a typo, a name left stale after a producer-side rename, or a
    /// Tier-1 host/protocol key that must NOT be declared here (Tier-1 self-gates
    /// by registration). A stray key has no gating effect of its own, but its
    /// mere presence flips the set into narrowing mode ([`Self::is_narrowing`]),
    /// so a *renamed* built-in declared under its old name silently drops the
    /// real key from every emitted frame. This is the mechanical
    /// "declared ⊆ decodable" check: a host cannot declare a key the framework
    /// does not emit/decode.
    ///
    /// Results are in deterministic (`BTreeSet`) order. An empty declared set
    /// declares nothing and therefore yields no strays — the "no opinion /
    /// no narrowing" state (ADR-0053 Decision 4) is untouched by this check.
    #[must_use]
    pub fn stray_keys<'a, I>(&self, decodable: I) -> Vec<String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let decodable: BTreeSet<&str> = decodable.into_iter().collect();
        self.keys
            .iter()
            .filter(|k| !decodable.contains(k.as_str()))
            .cloned()
            .collect()
    }

    /// **Workstream-E3 / ADR-0053 drift gate enforcement.** Assert the declared
    /// set is a subset of the framework's authoritative emittable/decodable set
    /// ([`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS),
    /// pinned to the real `make_update` insertion sites by
    /// `builtin_projection_keys_const_matches_runtime`).
    ///
    /// Any [`stray_keys`](Self::stray_keys) member is drift and is ALWAYS a bug
    /// (never the empty=permissive case, which declares nothing): a `debug_assert!`
    /// fails the offending host's debug/test build, while release builds stay
    /// behaviour-preserving and surface it through a non-fatal `tracing::warn!`.
    /// Called from the single registry declaration chokepoint, so every host —
    /// via the C-ABI, the `AppHost`/`NmpAppBuilder` seams, or the Chirp shell
    /// helper — is checked. Leaves the empty-set "no narrowing" semantic
    /// (ADR-0053 Decision 4 / Workstream-E4) untouched.
    pub(crate) fn enforce_no_drift(&self) {
        let stray = self.stray_keys(crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS.iter().copied());
        if !stray.is_empty() {
            tracing::warn!(
                stray = ?stray,
                "declare_consumed_projections: host declared projection key(s) that are \
                 not kernel-owned Tier-2 built-ins (KERNEL_BUILTIN_PROJECTION_KEYS). A \
                 stray key has no gating effect but flips the set into narrowing mode, so \
                 a renamed/typo'd built-in silently drops the real key from every frame \
                 (ADR-0053 / Workstream-E3 drift gate). Declare only kernel built-ins; \
                 Tier-1 host/protocol projections self-gate by registration and must not \
                 be declared here."
            );
            debug_assert!(
                stray.is_empty(),
                "declared consumed-projection key(s) not in \
                 KERNEL_BUILTIN_PROJECTION_KEYS (declared \u{2284} decodable): {stray:?}"
            );
        }
    }
}

impl super::SnapshotRegistry {
    /// ADR-0053 — declare (union into) the set of Tier-2 built-in projection
    /// keys this host consumes.
    ///
    /// Additive: call more than once and the sets union (e.g. a base set from
    /// `nmp-defaults` plus an app-specific extension). Intended as a host-init
    /// call, before `nmp_app_start`. An empty declared set leaves the kernel
    /// emitting every Tier-2 built-in (no narrowing); a non-empty set narrows
    /// the kernel-owned built-ins to the declared members. Tier-1 host/protocol
    /// projections are unaffected — they self-gate by registration.
    ///
    /// **Workstream-E3 single chokepoint.** Every declaration path funnels
    /// through here (the C-ABI `nmp_app_declare_consumed_projections`, the
    /// `AppHost`/`NmpAppBuilder` Rust seams, and the Chirp shell helper), so it
    /// enforces declared ⊆ decodable via [`DeclaredProjections::enforce_no_drift`].
    pub fn declare_consumed_projections<I, K>(&mut self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        self.declared_projections.declare(keys);
        self.declared_projections.enforce_no_drift();
    }

    /// Read the host-declared consumed-projection set — the gate the kernel
    /// consults per Tier-2 built-in key in `make_update`.
    #[must_use]
    pub fn declared_projections(&self) -> &DeclaredProjections {
        &self.declared_projections
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

    // ── Workstream-E3 — declared ⊆ decodable drift gate (`stray_keys`) ──

    /// An empty declared set declares nothing, so it never reports a stray —
    /// the "no opinion / no narrowing" semantic (ADR-0053 Decision 4) is
    /// untouched by the drift gate.
    #[test]
    fn empty_set_has_no_strays() {
        let d = DeclaredProjections::new();
        assert!(d
            .stray_keys(crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS.iter().copied())
            .is_empty());
    }

    /// A declaration drawn entirely from the framework's emittable set has no
    /// strays — the "green on master" shape (every real declaration is clean).
    #[test]
    fn declaration_of_only_builtins_has_no_strays() {
        let mut d = DeclaredProjections::new();
        d.declare(
            crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS
                .iter()
                .map(|k| k.to_string()),
        );
        assert!(
            d.stray_keys(crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS.iter().copied())
                .is_empty(),
            "the full built-in set declared back must be drift-free"
        );
    }

    /// **Non-vacuity** — a declared key that is NOT in the decodable set is
    /// reported as a stray, while clean siblings are not. This proves the gate
    /// fires: a typo'd / renamed / Tier-1 key cannot slip past it.
    #[test]
    fn stray_keys_flags_a_non_decodable_declaration() {
        let mut d = DeclaredProjections::new();
        // `relay_diagnstics` is a typo of the real `relay_diagnostics`; `profile`
        // is a real built-in; `nmp.feed.home` is a Tier-1 key that must not be
        // declared here (it self-gates by registration).
        d.declare(["profile", "relay_diagnstics", "nmp.feed.home"]);
        let mut stray =
            d.stray_keys(crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS.iter().copied());
        stray.sort();
        assert_eq!(
            stray,
            vec!["nmp.feed.home".to_string(), "relay_diagnstics".to_string()],
            "the typo and the Tier-1 key are strays; the real built-in `profile` is not"
        );
    }
}
