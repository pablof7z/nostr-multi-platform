//! Host-declared **consumed-projection set** (ADR-0053 / Workstream-E4).
//!
//! The output-side sibling of the relay interest-install lattice: a host declares,
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
//! ## Semantics — explicit intent is MANDATORY (Workstream-E4)
//!
//! Projection-consumption intent is **explicit**. There is exactly one way to mean
//! "everything" — [`DeclaredProjections::All`] (set via
//! `consume_all_builtin_projections`) — and one way to narrow —
//! [`DeclaredProjections::Narrow`] (via `declare_consumed_projections`). The third
//! state, [`DeclaredProjections::Undeclared`], is the **forgotten-declaration
//! footgun**: it is NOT a silent "emit everything" opinion. To stay
//! behaviour-preserving in release it still permits every built-in (so production
//! never crashes and never goes dark), but it is **loud** — `nmp_app_start` trips a
//! `debug_assert!` (panic in dev/test) and emits a non-fatal `tracing::warn!` in
//! release. Internal Rust consumers (chirp-tui / chirp-desktop) make their intent
//! explicit (`consume_all_builtin_projections`). The `Default` is `Undeclared`
//! (which permits-all in release), and the loud `debug_assert!` is compiled out
//! under `#[cfg(any(test, feature = "test-support"))]`, so existing tests rely on
//! `Undeclared`-permits-all and need no per-site declaration without tripping the
//! assert. **Production has no implicit `All` default** — an undeclared production
//! app is the loud forgotten-wiring case (warn + debug-assert), never silent.
//!
//! Tier-1 host/protocol projections (`SnapshotRegistry::register*`) are **not** gated
//! here — they already self-gate by registration (registration *is* the declaration),
//! and the dynamic per-view feeds gate by their `remove()`-on-close lifecycle.

use std::collections::BTreeSet;

/// The host-declared projection-consumption intent — a tri-state that makes
/// "consume everything" an explicit choice and "no declaration" a loud bug
/// rather than a silent firehose (ADR-0053 / Workstream-E4).
///
/// `Narrow` carries a `BTreeSet` for deterministic iteration and cheap
/// membership; the set is tiny (≤ the count of
/// [`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS),
/// today 18). Declarations are **additive** (union) — a host may call the declare
/// seam more than once (e.g. a base set from `explicit composition` plus an app-specific
/// extension) and the sets union.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DeclaredProjections {
    /// No projection-consumption intent was ever expressed — the
    /// forgotten-declaration state (and the default). `permits()` still emits
    /// everything (release behaviour-preserving — and so a kernel-tick under
    /// any consumer that never declared still produces the full set, never
    /// drops it), but this is NOT a silent opinion: `nmp_app_start` makes it
    /// loud (`debug_assert!` + `tracing::warn!`).
    #[default]
    Undeclared,
    /// Narrow to exactly these keys — emit a Tier-2 built-in only if it is a
    /// declared member. Always non-empty (an empty declaration never produces
    /// this variant; it would mean "emit nothing", which is never the intent).
    Narrow(BTreeSet<String>),
    /// The explicit "I consume every Tier-2 built-in" choice — the ONLY
    /// non-footgun way to receive the full set.
    All,
}

impl DeclaredProjections {
    /// Construct the `Undeclared` state — the explicit "no intent yet" value
    /// (independent of the `cfg`-gated [`Default`]).
    #[must_use]
    pub fn new() -> Self {
        DeclaredProjections::Undeclared
    }

    /// Union `keys` into the declared (narrowing) set (additive; idempotent per
    /// key). An empty `keys` is a no-op (never produces an emit-nothing
    /// `Narrow(∅)`); `All` stays `All` (declaring a subset of "everything" is a
    /// no-op); `Undeclared` advances to `Narrow` once a non-empty set arrives.
    pub fn declare<I, K>(&mut self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let incoming: BTreeSet<String> = keys.into_iter().map(Into::into).collect();
        if incoming.is_empty() {
            return;
        }
        match self {
            DeclaredProjections::All => {}
            DeclaredProjections::Narrow(set) => set.extend(incoming),
            DeclaredProjections::Undeclared => *self = DeclaredProjections::Narrow(incoming),
        }
    }

    /// Declare the explicit "consume every Tier-2 built-in" intent — the only
    /// non-footgun way to get the full set (ADR-0053 / Workstream-E4).
    pub fn consume_all(&mut self) {
        *self = DeclaredProjections::All;
    }

    /// `true` when the host narrowed to an explicit subset of built-ins. `All`
    /// and `Undeclared` are NOT narrowing (both permit everything). Two
    /// consumers read it: the `explicit composition` builder tests, and any host
    /// introspection that wants "am I in narrowing mode".
    #[must_use]
    pub fn is_narrowing(&self) -> bool {
        matches!(self, DeclaredProjections::Narrow(set) if !set.is_empty())
    }

    /// `true` when no projection-consumption intent was expressed — the loud
    /// forgotten-declaration state. Read by `nmp_app_start` to fire the
    /// `debug_assert!` + release `tracing::warn!`.
    #[must_use]
    pub fn is_undeclared(&self) -> bool {
        matches!(self, DeclaredProjections::Undeclared)
    }

    /// Whether the Tier-2 built-in `key` should be emitted this frame.
    ///
    /// `Undeclared` and `All` both permit everything (release
    /// behaviour-preserving — `Undeclared` is the loud-but-non-fatal footgun);
    /// `Narrow` permits `key` iff it is a declared member.
    #[must_use]
    pub fn permits(&self, key: &str) -> bool {
        match self {
            DeclaredProjections::Undeclared | DeclaredProjections::All => true,
            DeclaredProjections::Narrow(set) => set.contains(key),
        }
    }

    /// **Workstream-E3 / ADR-0053 drift gate** — the declared keys that are
    /// absent from `decodable`, the framework's authoritative emittable/decodable
    /// Tier-2 key set
    /// ([`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS)).
    ///
    /// A non-empty result is **drift**: the host declared a key the kernel never
    /// emits — a typo, a name left stale after a producer-side rename, or a
    /// Tier-1 host/protocol key that must NOT be declared here (Tier-1 self-gates
    /// by registration). A stray key has no gating effect of its own, but a
    /// *renamed* built-in declared under its old name silently drops the real key
    /// from every emitted frame. This is the mechanical "declared ⊆ decodable"
    /// check: a host cannot declare a key the framework does not emit/decode.
    ///
    /// Only the [`DeclaredProjections::Narrow`] state has declared keys to check;
    /// `Undeclared` and `All` declare no narrow set and therefore yield no
    /// strays. Results are in deterministic (`BTreeSet`) order.
    #[must_use]
    pub fn stray_keys<'a, I>(&self, decodable: I) -> Vec<String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        match self {
            DeclaredProjections::Narrow(keys) => {
                let decodable: BTreeSet<&str> = decodable.into_iter().collect();
                keys.iter()
                    .filter(|k| !decodable.contains(k.as_str()))
                    .cloned()
                    .collect()
            }
            DeclaredProjections::Undeclared | DeclaredProjections::All => Vec::new(),
        }
    }

    /// **Workstream-E3 / ADR-0053 drift gate enforcement.** Assert the declared
    /// (narrow) set is a subset of the framework's authoritative
    /// emittable/decodable set
    /// ([`KERNEL_BUILTIN_PROJECTION_KEYS`](crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS),
    /// pinned to the real `make_update` insertion sites by
    /// `builtin_projection_keys_const_matches_runtime`).
    ///
    /// Any [`stray_keys`](Self::stray_keys) member is drift and is ALWAYS a bug:
    /// a `debug_assert!` fails the offending host's debug/test build, while
    /// release builds stay behaviour-preserving and surface it through a
    /// non-fatal `tracing::warn!`. Called from the single registry declaration
    /// chokepoint, so every host — via the C-ABI, the `AppHost`/`NmpAppBuilder`
    /// seams, or the Chirp shell helper — is checked. `All` / `Undeclared`
    /// declare no narrow set, so they are untouched by this check.
    pub(crate) fn enforce_no_drift(&self) {
        let stray = self.stray_keys(
            crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS
                .iter()
                .copied(),
        );
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
    /// keys this host consumes (the narrowing path).
    ///
    /// Additive: call more than once and the sets union (e.g. a base set from
    /// `explicit composition` plus an app-specific extension). Intended as a host-init
    /// call, before `nmp_app_start`. A non-empty set narrows the kernel-owned
    /// built-ins to the declared members; Tier-1 host/protocol projections are
    /// unaffected — they self-gate by registration.
    ///
    /// **Workstream-E3 single chokepoint.** Every narrowing declaration funnels
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

    /// ADR-0053 / Workstream-E4 — declare the explicit "I consume every Tier-2
    /// built-in" intent ([`DeclaredProjections::All`]).
    ///
    /// This is the ONE non-footgun way to receive the full set. Full Rust
    /// clients (chirp-tui / chirp-desktop) and the Chirp shells call it; it
    /// overrides any prior narrowing (you cannot narrow after asking for
    /// everything). Intended as a host-init call before `nmp_app_start`.
    pub fn consume_all_builtin_projections(&mut self) {
        self.declared_projections.consume_all();
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
    fn undeclared_permits_everything_but_is_not_narrowing() {
        let d = DeclaredProjections::new();
        assert!(d.is_undeclared());
        assert!(!d.is_narrowing());
        assert!(d.permits("relay_diagnostics"));
        assert!(d.permits("anything_at_all"));
    }

    #[test]
    fn consume_all_permits_everything_and_is_not_narrowing_or_undeclared() {
        let mut d = DeclaredProjections::new();
        d.consume_all();
        assert_eq!(d, DeclaredProjections::All);
        assert!(!d.is_undeclared());
        assert!(!d.is_narrowing());
        assert!(d.permits("relay_diagnostics"));
        assert!(d.permits("anything_at_all"));
    }

    #[test]
    fn non_empty_set_narrows_to_members() {
        let mut d = DeclaredProjections::new();
        d.declare(["profile", "accounts"]);
        assert!(d.is_narrowing());
        assert!(!d.is_undeclared());
        assert!(d.permits("profile"));
        assert!(d.permits("accounts"));
        assert!(!d.permits("relay_diagnostics"));
    }

    #[test]
    fn declarations_are_additive() {
        let mut d = DeclaredProjections::new();
        d.declare(["profile"]);
        d.declare(["accounts", "profile"]);
        assert!(d.permits("profile"));
        assert!(d.permits("accounts"));
        assert!(!d.permits("relay_diagnostics"));
    }

    #[test]
    fn empty_declare_is_a_noop_and_never_narrows_to_nothing() {
        let mut d = DeclaredProjections::new();
        d.declare(Vec::<String>::new());
        // Stays Undeclared (permits everything) — never Narrow(∅) (emit nothing).
        assert!(d.is_undeclared());
        assert!(d.permits("profile"));
    }

    #[test]
    fn consume_all_then_declare_stays_all() {
        let mut d = DeclaredProjections::new();
        d.consume_all();
        d.declare(["profile"]);
        assert_eq!(d, DeclaredProjections::All);
        assert!(d.permits("relay_diagnostics"));
    }

    // ── Workstream-E3 — declared ⊆ decodable drift gate (`stray_keys`) ──

    /// `Undeclared` and `All` declare no narrow set, so they never report a
    /// stray — the explicit-everything / no-intent states are untouched by the
    /// drift gate.
    #[test]
    fn undeclared_and_all_have_no_strays() {
        let decodable = || {
            crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS
                .iter()
                .copied()
        };
        assert!(DeclaredProjections::Undeclared
            .stray_keys(decodable())
            .is_empty());
        assert!(DeclaredProjections::All.stray_keys(decodable()).is_empty());
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
            d.stray_keys(
                crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS
                    .iter()
                    .copied()
            )
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
        // is a real built-in; `app.feed.home` is a Tier-1 key that must not be
        // declared here (it self-gates by registration).
        d.declare(["profile", "relay_diagnstics", "app.feed.home"]);
        let mut stray = d.stray_keys(
            crate::kernel::KERNEL_BUILTIN_PROJECTION_KEYS
                .iter()
                .copied(),
        );
        stray.sort();
        assert_eq!(
            stray,
            vec!["app.feed.home".to_string(), "relay_diagnstics".to_string()],
            "the typo and the Tier-1 key are strays; the real built-in `profile` is not"
        );
    }
}
