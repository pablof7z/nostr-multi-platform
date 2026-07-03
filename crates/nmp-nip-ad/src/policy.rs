//! App-injected NIP-AD auto-resolution policy (#2927).
//!
//! Auto-resolution of an AD URL is NOT a framework default. NMP ships the
//! mechanism (the resolver) plus this policy seam; the APP chooses which policy
//! to inject at its composition root — exactly like choosing whether to
//! register a content component for a kind.
//!
//! The predicate is PURE and SYNC (no network). It gates ONLY the
//! content-renderer entry point (moment 1 — passively rendering someone else's
//! note): the renderer consults `should_auto_resolve` before dispatching a
//! resolve. Explicit search/paste (moment 2) is a per-URL user action and is
//! NEVER policy-gated.

use nostr::PublicKey;

/// Context handed to an [`AdResolutionPolicy`] at render time. Carries what a
/// policy needs to decide whether a note's AD URL may auto-resolve.
#[derive(Debug, Clone, Copy)]
pub struct AdRenderContext<'a> {
    /// Author of the note being rendered (whose choice of URL would trigger the
    /// passive fetch).
    pub author: &'a PublicKey,
    /// The candidate AD URL.
    pub url: &'a str,
}

/// Host-provided predicate deciding whether the content renderer may
/// auto-resolve a note's AD URL. Pure and sync — it never blocks render, it
/// only decides whether a *later* non-blocking resolve is attempted.
pub trait AdResolutionPolicy: Send + Sync {
    /// Return `true` iff this note's AD URL may auto-resolve (moment 1).
    fn should_auto_resolve(&self, ctx: &AdRenderContext<'_>) -> bool;
}

/// The "explicit only" posture: the content renderer never auto-fetches. AD
/// URLs render as plain links; resolution happens only via an explicit user
/// action (moment 2). The safest default and the recommended posture for apps
/// that do not want passive `.well-known` beacons.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverAutoResolve;

impl AdResolutionPolicy for NeverAutoResolve {
    fn should_auto_resolve(&self, _ctx: &AdRenderContext<'_>) -> bool {
        false
    }
}

/// Auto-resolve every candidate. The high-risk posture (a passive fetch to an
/// author-chosen domain for every rendered note); shipped for completeness but
/// most apps should not pick this — see the SSRF/beacon notes on #2927.
#[derive(Debug, Clone, Copy, Default)]
pub struct Always;

impl AdResolutionPolicy for Always {
    fn should_auto_resolve(&self, _ctx: &AdRenderContext<'_>) -> bool {
        true
    }
}

/// Auto-resolve only when the note's author is in the user's follow set.
///
/// Generic over a pure membership predicate rather than owning a snapshot of
/// the follow set, so the app can consult its live graph at render time and the
/// crate stays light (no `nmp-nip02`/graph dependency).
#[derive(Debug, Clone, Copy)]
pub struct FollowsOnly<F> {
    is_following: F,
}

impl<F> FollowsOnly<F>
where
    F: Fn(&PublicKey) -> bool + Send + Sync,
{
    /// Construct from a membership predicate (`author -> is-followed`).
    pub fn new(is_following: F) -> Self {
        Self { is_following }
    }
}

impl<F> AdResolutionPolicy for FollowsOnly<F>
where
    F: Fn(&PublicKey) -> bool + Send + Sync,
{
    fn should_auto_resolve(&self, ctx: &AdRenderContext<'_>) -> bool {
        (self.is_following)(ctx.author)
    }
}

/// Auto-resolve when the note's author is within `max_distance` hops in the
/// user's web of trust.
///
/// Generic over a pure distance function (`author -> Some(hops)` / `None` if
/// unreachable) rather than depending on `nmp-wot` directly, keeping this crate
/// light. The app wires `nmp-wot`'s distance lookup in at the composition root.
#[derive(Debug, Clone, Copy)]
pub struct WebOfTrust<D> {
    max_distance: u32,
    distance: D,
}

impl<D> WebOfTrust<D>
where
    D: Fn(&PublicKey) -> Option<u32> + Send + Sync,
{
    /// Construct from the maximum trusted distance and a distance function.
    pub fn new(max_distance: u32, distance: D) -> Self {
        Self {
            max_distance,
            distance,
        }
    }
}

impl<D> AdResolutionPolicy for WebOfTrust<D>
where
    D: Fn(&PublicKey) -> Option<u32> + Send + Sync,
{
    fn should_auto_resolve(&self, ctx: &AdRenderContext<'_>) -> bool {
        (self.distance)(ctx.author)
            .is_some_and(|hops| hops <= self.max_distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn pk(byte: u8) -> PublicKey {
        PublicKey::from_hex(&format!("{byte:02x}").repeat(32)).unwrap()
    }

    #[test]
    fn never_and_always() {
        let author = pk(1);
        let ctx = AdRenderContext {
            author: &author,
            url: "https://example.com/x",
        };
        assert!(!NeverAutoResolve.should_auto_resolve(&ctx));
        assert!(Always.should_auto_resolve(&ctx));
    }

    #[test]
    fn follows_only_gates_on_membership() {
        let followed = pk(1);
        let stranger = pk(2);
        let set: HashSet<PublicKey> = [followed].into_iter().collect();
        let policy = FollowsOnly::new(|a: &PublicKey| set.contains(a));

        let ctx_followed = AdRenderContext {
            author: &followed,
            url: "https://example.com/a",
        };
        let ctx_stranger = AdRenderContext {
            author: &stranger,
            url: "https://example.com/b",
        };
        assert!(policy.should_auto_resolve(&ctx_followed));
        assert!(!policy.should_auto_resolve(&ctx_stranger));
    }

    #[test]
    fn wot_gates_on_distance() {
        let near = pk(1);
        let far = pk(2);
        let unreachable = pk(3);
        let policy = WebOfTrust::new(2, |a: &PublicKey| {
            if *a == near {
                Some(1)
            } else if *a == far {
                Some(5)
            } else {
                None
            }
        });

        for (author, expect) in [(near, true), (far, false), (unreachable, false)] {
            let ctx = AdRenderContext {
                author: &author,
                url: "https://example.com/x",
            };
            assert_eq!(policy.should_auto_resolve(&ctx), expect);
        }
    }
}
