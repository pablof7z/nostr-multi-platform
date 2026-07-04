//! NIP-AD resolution wiring (#2927) — moment-1 (passive render) + moment-2
//! (explicit paste/search), both on ONE resolver + ONE delivery doorway.
//!
//! Layer A (`nmp-nip-ad`) ships the pure `.well-known/nostr.json?ad=<path>`
//! resolve, the `AdResolutionPolicy` seam, and the `open_ad_collection`
//! relay-pinned collection doorway. This module is the native runtime's
//! consumer:
//!
//! * **Moment 1 (render):** the content renderer, hitting a
//!   `WireNode::AdCandidateUrl`, calls [`nmp_content::AdUrlResolver`]
//!   (implemented for [`NmpApp`] below). Gated by the app-injected
//!   [`AdResolutionPolicy`](nmp_nip_ad::AdResolutionPolicy): if the note's
//!   author/URL is permitted, the runtime resolves off-thread and opens the
//!   collection. If no policy is injected (the default) or it declines, nothing
//!   fetches — the URL stays a plain link.
//! * **Moment 2 (paste/search):** [`crate::intent`] routes an
//!   `InputIntentTarget::AdCandidate` here — NEVER policy-gated (an explicit
//!   per-URL user action) — and the parallel free-text search fires alongside
//!   (D1) so the user is never blocked.
//!
//! Fail-open (D1/D6): every failure path records [`AdUrlState::ResolutionFailed`]
//! and leaves the plain link. The resolve is ALWAYS off the calling thread (a
//! spawned worker holding a Send [`crate::read_host_handle`] clone); the render
//! / dispatch call returns immediately.

use std::collections::hash_map::{DefaultHasher, Entry};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_content::{AdUrlResolver, AdUrlState};
use nmp_nip_ad::{
    ad_collection_projection_key, close_ad_collection_by_key, open_ad_collection, AdRenderContext,
    AdResolutionPolicy,
};

use crate::NmpApp;

/// Per-URL resolution bookkeeping shared with the resolve worker.
///
/// `state` drives the renderer's plain-link-vs-collection decision; `refcount`
/// is the claim refcount so the collection is torn down when the last observer
/// releases; `session_id` is the opaque token the collection projection key is
/// built from (the shell never computes it — it reads `state`).
#[derive(Clone)]
pub(crate) struct AdUrlEntry {
    state: AdUrlState,
    refcount: usize,
    session_id: String,
}

/// URL → [`AdUrlEntry`]. `Arc<Mutex<..>>` so the resolve worker can update it.
pub(crate) type AdUrlStateMap = Arc<Mutex<HashMap<String, AdUrlEntry>>>;

/// Stable per-process session token for a URL (opaque; keeps the collection
/// projection key well-formed and deduplicates the same URL across notes).
/// `DefaultHasher::new()` is fixed-key (not the randomized `RandomState`), so
/// the same URL maps to the same token within a process.
fn session_id_for_url(url: &str) -> String {
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl NmpApp {
    /// Composition-root seam: install the app's NIP-AD auto-resolution policy
    /// (#2927). Choosing the policy is the app developer's call — exactly like
    /// registering a content component for a kind. With no policy installed the
    /// runtime behaves as `NeverAutoResolve`: moment-1 never passively fetches.
    pub fn set_ad_resolution_policy(&self, policy: Arc<dyn AdResolutionPolicy>) {
        if let Ok(mut slot) = self.ad_resolution_policy.lock() {
            *slot = Some(policy);
        }
    }

    /// Current [`AdUrlState`] for `url` (the read-door query a renderer makes to
    /// decide plain-link vs. resolved collection). `NotAttempted` when the URL
    /// has never been claimed. When `Resolved`, the carried `projection_key`
    /// names the typed `AdCollectionSnapshot` to read from the snapshot frame.
    #[must_use]
    pub fn ad_url_state(&self, url: &str) -> AdUrlState {
        self.ad_url_states
            .lock()
            .ok()
            .and_then(|m| m.get(url).map(|e| e.state.clone()))
            .unwrap_or(AdUrlState::NotAttempted)
    }

    /// Moment-1 claim: attempt NIP-AD resolution of `url` for a note authored by
    /// `author_pubkey_hex`, on behalf of `consumer_id`. Idempotent + infallible
    /// (a repeat claim while already resolving/resolved is a no-op bump). The
    /// injected [`AdResolutionPolicy`] gates whether the fetch fires; the plain
    /// link renders regardless.
    pub(crate) fn claim_ad_url_impl(
        &self,
        url: &str,
        author_pubkey_hex: &str,
        _consumer_id: &str,
    ) {
        let should_resolve = self.policy_permits(url, author_pubkey_hex);

        let session_id = {
            let mut states = match self.ad_url_states.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            match states.entry(url.to_string()) {
                Entry::Occupied(mut e) => {
                    // Already tracked — bump the refcount and reuse state. Never
                    // re-fetch mid-flight or after a terminal state (dedupe/D6).
                    e.get_mut().refcount += 1;
                    return;
                }
                Entry::Vacant(v) => {
                    let session_id = session_id_for_url(url);
                    let state = if should_resolve {
                        AdUrlState::Resolving
                    } else {
                        // Policy declined (or none installed) — the URL renders
                        // as a permanent plain link; no fetch, no worker.
                        AdUrlState::NotAttempted
                    };
                    v.insert(AdUrlEntry {
                        state,
                        refcount: 1,
                        session_id: session_id.clone(),
                    });
                    session_id
                }
            }
        };

        if should_resolve {
            self.spawn_ad_resolve(url.to_string(), session_id);
        }
    }

    /// Moment-1 release: decrement the claim refcount for `url`; on the last
    /// release, close the collection and forget the state. Unknown/double
    /// release is a no-op.
    pub(crate) fn release_ad_url_impl(&self, url: &str, _consumer_id: &str) {
        let close_session = {
            let mut states = match self.ad_url_states.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            let Some(entry) = states.get_mut(url) else {
                return;
            };
            entry.refcount = entry.refcount.saturating_sub(1);
            if entry.refcount == 0 {
                states.remove(url).map(|e| e.session_id)
            } else {
                None
            }
        };
        if let Some(session_id) = close_session {
            let _ = close_ad_collection_by_key(&self.read_host(), &session_id);
        }
    }

    /// Moment-2 explicit dispatch (paste/search). NEVER policy-gated. The
    /// parallel free-text search is fired by `dispatch_input_intent` before this
    /// (D1); here we only attempt the AD resolution + collection open.
    pub(crate) fn act_on_ad_candidate_intent(&self, url: &str) {
        let session_id = session_id_for_url(url);
        {
            // Record intent so `ad_url_state` reflects the in-flight resolve.
            if let Ok(mut states) = self.ad_url_states.lock() {
                states
                    .entry(url.to_string())
                    .or_insert_with(|| AdUrlEntry {
                        state: AdUrlState::Resolving,
                        refcount: 1,
                        session_id: session_id.clone(),
                    })
                    .state = AdUrlState::Resolving;
            }
        }
        self.spawn_ad_resolve(url.to_string(), session_id);
    }

    /// True iff the injected policy permits auto-resolving `url` for `author`.
    /// No policy installed → `false` (the `NeverAutoResolve` default). An
    /// unparseable author pubkey fails closed (never resolve) — the gate must
    /// see a real author.
    fn policy_permits(&self, url: &str, author_pubkey_hex: &str) -> bool {
        let Ok(slot) = self.ad_resolution_policy.lock() else {
            return false;
        };
        let Some(policy) = slot.as_ref() else {
            return false;
        };
        let Ok(author) = nostr::PublicKey::from_hex(author_pubkey_hex) else {
            return false;
        };
        policy.should_auto_resolve(&AdRenderContext {
            author: &author,
            url,
        })
    }

    /// Spawn the off-thread `.well-known` resolve + `open_ad_collection`. The
    /// worker owns a Send read-host clone + a Send state-map clone; it NEVER
    /// touches `&self`. D8: nothing blocks the caller.
    fn spawn_ad_resolve(&self, url: String, session_id: String) {
        let read_host = self.read_host();
        let states = Arc::clone(&self.ad_url_states);
        std::thread::spawn(move || {
            match nmp_nip_ad::resolve_ad_url_blocking(&url) {
                Ok(resolution) => {
                    // Open the relay-pinned collection through the shared engine
                    // (non-blocking; events land in the typed ADCL projection).
                    let _handle = open_ad_collection(&read_host, &resolution, &session_id);
                    let projection_key = ad_collection_projection_key(&session_id);
                    if let Ok(mut m) = states.lock() {
                        if let Some(entry) = m.get_mut(&url) {
                            entry.state = AdUrlState::Resolved { projection_key };
                        }
                    }
                }
                Err(_reason) => {
                    // Fail-open (D6): terminal failure, keep the plain link.
                    if let Ok(mut m) = states.lock() {
                        if let Some(entry) = m.get_mut(&url) {
                            entry.state = AdUrlState::ResolutionFailed {
                                at: now_unix_secs(),
                            };
                        }
                    }
                }
            }
        });
    }
}

/// In-repo Rust hosts (TUI/desktop registries) pass `&NmpApp` as the renderer's
/// `AdUrlResolver`. Fire-and-forget, infallible (D6).
impl AdUrlResolver for NmpApp {
    fn claim_ad_url(&self, url: &str, author_pubkey_hex: &str, consumer_id: &str) {
        self.claim_ad_url_impl(url, author_pubkey_hex, consumer_id);
    }

    fn release_ad_url(&self, url: &str, consumer_id: &str) {
        self.release_ad_url_impl(url, consumer_id);
    }
}

#[cfg(test)]
impl NmpApp {
    /// Test-only claim refcount for `url` (0 when untracked).
    fn ad_url_refcount(&self, url: &str) -> usize {
        self.ad_url_states
            .lock()
            .ok()
            .and_then(|m| m.get(url).map(|e| e.refcount))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nip_ad::{Always, NeverAutoResolve};

    const AUTHOR: &str = "0000000000000000000000000000000000000000000000000000000000000001";
    const URL: &str = "https://example.com/players";

    #[test]
    fn unknown_url_is_not_attempted() {
        let app = crate::new_app();
        assert_eq!(app.ad_url_state(URL), AdUrlState::NotAttempted);
    }

    #[test]
    fn no_policy_declines_claim_and_keeps_plain_link() {
        // Default posture (no policy injected) never passively fetches — the
        // URL stays NotAttempted (a plain link), no worker spawned.
        let app = crate::new_app();
        app.claim_ad_url_impl(URL, AUTHOR, "c1");
        assert_eq!(app.ad_url_state(URL), AdUrlState::NotAttempted);
        assert_eq!(app.ad_url_refcount(URL), 1);
    }

    #[test]
    fn never_policy_declines_claim() {
        let app = crate::new_app();
        app.set_ad_resolution_policy(Arc::new(NeverAutoResolve));
        app.claim_ad_url_impl(URL, AUTHOR, "c1");
        assert_eq!(app.ad_url_state(URL), AdUrlState::NotAttempted);
    }

    #[test]
    fn always_policy_permits_claim_and_never_hangs() {
        // `Always` permits the resolve: the state leaves NotAttempted (Resolving,
        // then a terminal fail-open state once the worker's fetch of this
        // non-AD/unreachable host fails). Whatever the outcome, it is never a
        // permanent NotAttempted and never a hang — the plain-link fallback is
        // always reachable.
        let app = crate::new_app();
        app.set_ad_resolution_policy(Arc::new(Always));
        app.claim_ad_url_impl("https://nonexistent.invalid/x", AUTHOR, "c1");
        assert_ne!(
            app.ad_url_state("https://nonexistent.invalid/x"),
            AdUrlState::NotAttempted,
            "Always policy must permit the resolve attempt"
        );
    }

    #[test]
    fn claim_is_refcounted_and_release_decays() {
        let app = crate::new_app();
        app.claim_ad_url_impl(URL, AUTHOR, "c1");
        app.claim_ad_url_impl(URL, AUTHOR, "c2");
        assert_eq!(app.ad_url_refcount(URL), 2);
        app.release_ad_url_impl(URL, "c1");
        assert_eq!(app.ad_url_refcount(URL), 1);
        app.release_ad_url_impl(URL, "c2");
        assert_eq!(app.ad_url_refcount(URL), 0);
        // Double / unknown release is a no-op.
        app.release_ad_url_impl(URL, "c2");
        assert_eq!(app.ad_url_refcount(URL), 0);
    }

    #[test]
    fn moment2_dispatch_is_never_policy_gated() {
        // Moment-2 (explicit paste/search) resolves even with NO policy / a
        // NeverAutoResolve policy — it is an explicit user action, not a passive
        // render. The state leaves NotAttempted (resolve attempted), unlike a
        // moment-1 claim under the same policy.
        let app = crate::new_app();
        app.set_ad_resolution_policy(Arc::new(NeverAutoResolve));
        app.act_on_ad_candidate_intent("https://nonexistent.invalid/z");
        assert_ne!(
            app.ad_url_state("https://nonexistent.invalid/z"),
            AdUrlState::NotAttempted,
            "moment-2 must resolve regardless of the render policy"
        );
    }

    #[test]
    fn session_id_is_stable_per_url() {
        assert_eq!(session_id_for_url(URL), session_id_for_url(URL));
        assert_ne!(session_id_for_url(URL), session_id_for_url("https://other/y"));
    }
}
