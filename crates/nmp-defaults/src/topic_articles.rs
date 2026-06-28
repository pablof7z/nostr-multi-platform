//! Action-driven subscription for NIP-23 long-form articles by topic tag.
//!
//! **This module is the builder-guide example for the action → subscription
//! pattern.** Copy it as the skeleton for any action module that needs to
//! open (and later close) a kernel-owned Nostr subscription in response to
//! a user-triggered action.
//!
//! # The pattern in four steps
//!
//! 1. **Shell dispatches Claim.** The shell calls `dispatch_action` with the
//!    `"op":"claim"` variant when the user opens a discovery view.
//! 2. **Kernel opens the subscriptions.** `execute()` sends
//!    `InterestsCommand::EnsureInterest` for the direct article lane and for the
//!    generic-repost lane. On the next planner tick the kernel emits REQs to
//!    the relay(s). No relay logic is in the shell.
//! 3. **Events arrive reactively.** Matching kind:30023 events and kind:16
//!    wrappers flow through any registered `ObservedProjectionSink` into the app's
//!    read model, then into the push projection the shell reads off each
//!    snapshot frame. The shell does not poll; the kernel pushes.
//! 4. **Shell dispatches Release.** When the view closes the shell dispatches
//!    the `"op":"release"` variant with the same `topic` and `consumer_id`.
//!    `execute()` sends `InterestsCommand::DropInterestOwner` for both lanes.
//!    When the last owner drops, the registry GCs the slots and sends CLOSE.
//!
//! # Why Claim/Release live in the same module
//!
//! Both variants must derive the *same* [`SubIdentity`] from the same inputs.
//! Keeping them in one module makes that structurally guaranteed — a separate
//! "withdraw" module that re-derives the identity from user-supplied strings
//! risks a mismatch (wrong owner dropped → subscription leaks forever).
//! See `nmp-relations::visible_relations` for the live production analogue that
//! established this pattern.
//!
//! # Multi-owner refcounting
//!
//! `consumer_id` is the caller's stable view-instance identifier (e.g.
//! `"discover-view"`, `"sidebar-widget"`). Multiple consumers may hold
//! independent Claim registrations for the same `topic` — the registry keeps
//! one direct lane and one repost lane alive and GCs each only when every
//! consumer has Released. Use a
//! stable, unique `consumer_id` per call-site; do not reuse the same id
//! across unrelated views unless you intentionally want them to share the
//! refcount.
//!
//! # Adapting this pattern
//!
//! | What to change | How |
//! |---|---|
//! | Event kind | Replace `KIND_LONG_FORM_ARTICLE` and the `kinds` field |
//! | Filter axis | Replace `("t", topic)` in `tag_refs` with your tag, or use `authors`, `ids`, etc. |
//! | Lifecycle | `Tailing` for live streams; `OneShot` for one-time fetches (closes after EOSE) |
//! | Indexer opt-in | `is_indexer_discovery: true` for sparse kinds, `false` for inbox-style data |
//! | Namespace | Replace `TOPIC_ARTICLES_NAMESPACE` throughout; keep it globally unique |
//!
//! # What NOT to do
//!
//! Do not use `dispatch_capability` with a relay-flavoured namespace to
//! fetch Nostr events. The capability seam is for host-side I/O the kernel
//! cannot do (keyring, audio, file storage). Relay operations belong to the
//! kernel exclusively; this module is how you reach them.

use nmp_core::actor::ActorCommand;
use nmp_core::actor::InterestsCommand;
use nmp_core::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, ViewDependencies,
};
pub use nmp_kinds::KIND_LONG_FORM_ARTICLE;
use nmp_planner::stable_hash::stable_hash64;
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
use serde::{Deserialize, Serialize};

/// Initial page size for discovery subscriptions. Articles are
/// parameterised-replaceable events so relays do not bound them as tightly
/// as kind:1; 50 is a conservative limit for a discovery context.
pub const TOPIC_ARTICLES_LIMIT: u32 = 50;

pub const TOPIC_ARTICLES_NAMESPACE: &str = "nmp.app.topic_articles";
const TOPIC_ARTICLES_REPOST_LANE: &str = "reposts";

// ── Interest helpers ──────────────────────────────────────────────────────────

/// Stable [`InterestId`] for the direct topic-articles subscription keyed to `topic`.
///
/// Derived by hashing the module namespace + the topic string so the same
/// (namespace, topic) pair always maps to the same registry slot, across
/// restarts and processes.
#[must_use]
pub fn topic_articles_interest_id(topic: &str) -> InterestId {
    InterestId(stable_hash64((TOPIC_ARTICLES_NAMESPACE, topic)))
}

/// Stable [`InterestId`] for the topic-article repost lane keyed to `topic`.
#[must_use]
pub fn topic_article_reposts_interest_id(topic: &str) -> InterestId {
    InterestId(stable_hash64((
        TOPIC_ARTICLES_NAMESPACE,
        TOPIC_ARTICLES_REPOST_LANE,
        topic,
    )))
}

/// Build the tailing direct [`LogicalInterest`] for kind:30023 events tagged
/// `#t=topic`.
///
/// `is_indexer_discovery: true` routes the initial bootstrap through the
/// search indexer — articles by topic are sparse on general-purpose relays.
/// `Tailing` keeps the subscription open so new articles stream in live.
#[must_use]
pub fn topic_articles_interest(topic: &str) -> LogicalInterest {
    let mut interest = ViewDependencies {
        kinds: vec![KIND_LONG_FORM_ARTICLE],
        tag_refs: vec![("t".to_string(), topic.to_string())],
        limit: Some(TOPIC_ARTICLES_LIMIT),
        ..Default::default()
    }
    .into_logical_interest(
        topic_articles_interest_id(topic),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    );
    interest.is_indexer_discovery = true;
    interest
}

/// Build the tailing repost [`LogicalInterest`] for generic kind:16 wrappers.
///
/// A single Nostr filter cannot express `(kind:30023 AND #t=topic) OR
/// (kind:16 AND #k=30023)`. The direct topic lane stays constrained by `#t`;
/// this wrapper lane asks only for long-form repost wrappers, and the
/// long-form feed adapter admits a row for a topic only when the embedded or
/// locally-known target article has that topic. It does not fetch missing
/// targets.
#[must_use]
pub fn topic_article_reposts_interest(topic: &str) -> LogicalInterest {
    let mut interest = ViewDependencies {
        kinds: vec![nmp_nip18::KIND_GENERIC_REPOST],
        tag_refs: vec![("k".to_string(), KIND_LONG_FORM_ARTICLE.to_string())],
        limit: Some(TOPIC_ARTICLES_LIMIT),
        ..Default::default()
    }
    .into_logical_interest(
        topic_article_reposts_interest_id(topic),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    );
    interest.is_indexer_discovery = true;
    interest
}

/// Build the [`SubIdentity`] ownership triple for a `(topic, consumer_id)` pair.
///
/// The owner key folds in the module namespace so keys from different modules
/// never collide even if `topic` and `consumer_id` strings happen to match.
/// The slot key folds only `topic` (not `consumer_id`) so all consumers of the
/// same topic share the direct registry slot and direct REQ on the wire.
#[must_use]
pub fn topic_articles_identity(topic: &str, consumer_id: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new((TOPIC_ARTICLES_NAMESPACE, "owner", topic, consumer_id)),
        SubKey::builder(TOPIC_ARTICLES_NAMESPACE)
            .with(topic)
            .finish(),
        SubScope::Global,
    )
}

/// Build the [`SubIdentity`] ownership triple for the kind:16 repost lane.
#[must_use]
pub fn topic_article_reposts_identity(topic: &str, consumer_id: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new((
            TOPIC_ARTICLES_NAMESPACE,
            "owner",
            TOPIC_ARTICLES_REPOST_LANE,
            topic,
            consumer_id,
        )),
        SubKey::builder(TOPIC_ARTICLES_NAMESPACE)
            .with(topic)
            .with(TOPIC_ARTICLES_REPOST_LANE)
            .finish(),
        SubScope::Global,
    )
}

// ── Action module ─────────────────────────────────────────────────────────────

/// Tagged action for opening and closing a topic-articles subscription.
///
/// Dispatch examples (JSON over `dispatch_action`):
///
/// ```json
/// {"namespace":"nmp.app.topic_articles","action":{"op":"claim","topic":"bitcoin","consumer_id":"discover-view"}}
/// {"namespace":"nmp.app.topic_articles","action":{"op":"release","topic":"bitcoin","consumer_id":"discover-view"}}
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum TopicArticlesAction {
    /// Open (or join) the direct kind:30023 and kind:16 wrapper lanes for `topic`.
    ///
    /// Idempotent: a second Claim from the same `consumer_id` on the same
    /// `topic` is a no-op at the registry level. A Claim from a different
    /// `consumer_id` on the same `topic` attaches another owner — the
    /// kernel keeps one REQ open per lane for both.
    Claim {
        /// The `#t` tag value to filter on (e.g. `"bitcoin"`, `"nostr"`).
        topic: String,
        /// Stable, unique identifier for the calling view or component.
        /// Used to scope the refcount so each independent consumer can
        /// Release without affecting others. Must be non-empty.
        consumer_id: String,
    },
    /// Release this consumer's ownership of the `topic` feed lanes.
    ///
    /// When the last owner releases, the registry GCs the slots and the
    /// kernel sends CLOSE to the relay.
    Release {
        /// Must match the `topic` passed to the corresponding Claim.
        topic: String,
        /// Must match the `consumer_id` passed to the corresponding Claim.
        consumer_id: String,
    },
}

pub struct TopicArticlesModule;

impl ActionModule for TopicArticlesModule {
    const NAMESPACE: &'static str = TOPIC_ARTICLES_NAMESPACE;
    type Action = TopicArticlesAction;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let (topic, consumer_id) = action.parts();
        if topic.is_empty() {
            return Err(ActionRejection::Invalid(
                "topic_articles: `topic` must not be empty".to_string(),
            ));
        }
        if consumer_id.is_empty() {
            return Err(ActionRejection::Invalid(
                "topic_articles: `consumer_id` must not be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            TopicArticlesAction::Claim {
                ref topic,
                ref consumer_id,
            } => {
                send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                    identity: topic_articles_identity(topic, consumer_id),
                    interest: topic_articles_interest(topic),
                }));
                send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                    identity: topic_article_reposts_identity(topic, consumer_id),
                    interest: topic_article_reposts_interest(topic),
                }));
            }
            TopicArticlesAction::Release {
                ref topic,
                ref consumer_id,
            } => {
                send(ActorCommand::Interests(
                    InterestsCommand::DropInterestOwner(topic_articles_identity(
                        topic,
                        consumer_id,
                    )),
                ));
                send(ActorCommand::Interests(
                    InterestsCommand::DropInterestOwner(topic_article_reposts_identity(
                        topic,
                        consumer_id,
                    )),
                ));
            }
        }
        Ok(())
    }
}

impl TopicArticlesAction {
    fn parts(&self) -> (&str, &str) {
        match self {
            Self::Claim { topic, consumer_id } | Self::Release { topic, consumer_id } => {
                (topic, consumer_id)
            }
        }
    }
}

/// Register [`TopicArticlesModule`] on `app`.
///
/// Call this from your app's composition root (alongside
/// [`nmp_defaults::register_defaults`]) before `nmp_app_start`.
pub fn register_topic_articles_actions(app: &mut impl ActionRegistrar) {
    app.register_action(TopicArticlesModule)
        .expect("duplicate registration: nmp-defaults TopicArticlesModule"); // doctrine-allow: D6 — startup-only call; RegistrationError here is a programmer error
}

#[cfg(test)]
#[path = "topic_articles_tests.rs"]
mod tests;
