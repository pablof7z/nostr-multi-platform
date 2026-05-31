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
//! 2. **Kernel opens the subscription.** `execute()` sends
//!    `ActorCommand::EnsureInterest`. On the next planner tick the kernel
//!    emits a REQ to the relay(s). No relay logic is in the shell.
//! 3. **Events arrive reactively.** Matching kind:30023 events flow through
//!    any registered `KernelEventObserver` into the app's read model, then
//!    into the push projection the shell reads off each snapshot frame.
//!    The shell does not poll; the kernel pushes.
//! 4. **Shell dispatches Release.** When the view closes the shell dispatches
//!    the `"op":"release"` variant with the same `topic` and `consumer_id`.
//!    `execute()` sends `ActorCommand::DropInterestOwner`. When the last
//!    owner drops, the registry GCs the slot and sends CLOSE.
//!
//! # Why Claim/Release live in the same module
//!
//! Both variants must derive the *same* [`SubIdentity`] from the same inputs.
//! Keeping them in one module makes that structurally guaranteed — a separate
//! "withdraw" module that re-derives the identity from user-supplied strings
//! risks a mismatch (wrong owner dropped → subscription leaks forever).
//! See `nmp-nip01::visible_relations` for the live production analogue that
//! established this pattern.
//!
//! # Multi-owner refcounting
//!
//! `consumer_id` is the caller's stable view-instance identifier (e.g.
//! `"discover-view"`, `"sidebar-widget"`). Multiple consumers may hold
//! independent Claim registrations for the same `topic` — the registry keeps
//! **one** REQ alive and GCs it only when every consumer has Released. Use a
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

use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
use nmp_core::stable_hash::stable_hash64;
use nmp_core::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionRegistrar, ActionRejection, ViewDependencies,
};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

/// NIP-23 long-form article kind.
pub const KIND_LONG_FORM_ARTICLE: u32 = 30023;

/// Initial page size for discovery subscriptions. Articles are
/// parameterised-replaceable events so relays do not bound them as tightly
/// as kind:1; 50 is a conservative limit for a discovery context.
pub const TOPIC_ARTICLES_LIMIT: u32 = 50;

pub const TOPIC_ARTICLES_NAMESPACE: &str = "nmp.app.topic_articles";

// ── Interest helpers ──────────────────────────────────────────────────────────

/// Stable [`InterestId`] for the topic-articles subscription keyed to `topic`.
///
/// Derived by hashing the module namespace + the topic string so the same
/// (namespace, topic) pair always maps to the same registry slot, across
/// restarts and processes.
#[must_use]
pub fn topic_articles_interest_id(topic: &str) -> InterestId {
    InterestId(stable_hash64((TOPIC_ARTICLES_NAMESPACE, topic)))
}

/// Build the tailing [`LogicalInterest`] for kind:30023 events tagged
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

/// Build the [`SubIdentity`] ownership triple for a `(topic, consumer_id)` pair.
///
/// The owner key folds in the module namespace so keys from different modules
/// never collide even if `topic` and `consumer_id` strings happen to match.
/// The slot key folds only `topic` (not `consumer_id`) so all consumers of the
/// same topic share one registry slot and one REQ on the wire.
#[must_use]
pub fn topic_articles_identity(topic: &str, consumer_id: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new((TOPIC_ARTICLES_NAMESPACE, "owner", topic, consumer_id)),
        SubKey::builder(TOPIC_ARTICLES_NAMESPACE).with(topic).finish(),
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
    /// Open (or join) the kind:30023 subscription for `topic`.
    ///
    /// Idempotent: a second Claim from the same `consumer_id` on the same
    /// `topic` is a no-op at the registry level. A Claim from a different
    /// `consumer_id` on the same `topic` attaches another owner — the
    /// kernel keeps one REQ open for both.
    Claim {
        /// The `#t` tag value to filter on (e.g. `"bitcoin"`, `"nostr"`).
        topic: String,
        /// Stable, unique identifier for the calling view or component.
        /// Used to scope the refcount so each independent consumer can
        /// Release without affecting others. Must be non-empty.
        consumer_id: String,
    },
    /// Release this consumer's ownership of the `topic` subscription.
    ///
    /// When the last owner releases, the registry GCs the slot and the
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

    fn start(_ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
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
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            TopicArticlesAction::Claim {
                ref topic,
                ref consumer_id,
            } => {
                send(ActorCommand::EnsureInterest {
                    identity: topic_articles_identity(topic, consumer_id),
                    interest: topic_articles_interest(topic),
                });
            }
            TopicArticlesAction::Release {
                ref topic,
                ref consumer_id,
            } => {
                send(ActorCommand::DropInterestOwner(topic_articles_identity(
                    topic,
                    consumer_id,
                )));
            }
        }
        Ok(())
    }
}

impl TopicArticlesAction {
    fn parts(&self) -> (&str, &str) {
        match self {
            Self::Claim {
                topic,
                consumer_id,
            }
            | Self::Release {
                topic,
                consumer_id,
            } => (topic, consumer_id),
        }
    }
}

/// Register [`TopicArticlesModule`] on `app`.
///
/// Call this from your app's composition root (alongside
/// [`nmp_app_template::register_defaults`]) before `nmp_app_start`.
pub fn register_topic_articles_actions(app: &mut impl ActionRegistrar) {
    app.register_action::<TopicArticlesModule>();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TOPIC: &str = "bitcoin";
    const CONSUMER: &str = "discover-view";

    fn run_execute(action: TopicArticlesAction) -> Vec<ActorCommand> {
        let cmds = std::cell::RefCell::new(Vec::new());
        TopicArticlesModule::execute(action, "test-cid", &|cmd| cmds.borrow_mut().push(cmd))
            .expect("execute must not fail for valid input");
        cmds.into_inner()
    }

    #[test]
    fn claim_sends_ensure_interest_for_correct_kind_and_tag() {
        let action = TopicArticlesAction::Claim {
            topic: TOPIC.to_string(),
            consumer_id: CONSUMER.to_string(),
        };
        let cmds = run_execute(action);
        assert_eq!(cmds.len(), 1);
        let ActorCommand::EnsureInterest { identity, interest } = &cmds[0] else {
            panic!("expected EnsureInterest, got {:?}", cmds[0]);
        };
        assert_eq!(*identity, topic_articles_identity(TOPIC, CONSUMER));
        assert_eq!(interest.id, topic_articles_interest_id(TOPIC));
        assert!(interest.shape.kinds.contains(&KIND_LONG_FORM_ARTICLE));
        assert_eq!(
            interest.shape.tags.get("t").and_then(|v| v.iter().next().map(|s| s.as_str())),
            Some(TOPIC)
        );
        assert!(interest.is_indexer_discovery);
    }

    #[test]
    fn release_sends_drop_interest_owner_with_matching_identity() {
        let action = TopicArticlesAction::Release {
            topic: TOPIC.to_string(),
            consumer_id: CONSUMER.to_string(),
        };
        let cmds = run_execute(action);
        assert_eq!(cmds.len(), 1);
        let ActorCommand::DropInterestOwner(identity) = &cmds[0] else {
            panic!("expected DropInterestOwner, got {:?}", cmds[0]);
        };
        assert_eq!(*identity, topic_articles_identity(TOPIC, CONSUMER));
    }

    #[test]
    fn claim_and_release_derive_identical_identity() {
        let claim = topic_articles_identity(TOPIC, CONSUMER);
        let release = topic_articles_identity(TOPIC, CONSUMER);
        assert_eq!(claim, release, "claim and release must share the same SubIdentity");
    }

    #[test]
    fn different_consumers_same_topic_have_distinct_owner_keys() {
        let a = topic_articles_identity(TOPIC, "view-a");
        let b = topic_articles_identity(TOPIC, "view-b");
        // Different owners — each holds an independent refcount.
        assert_ne!(a.owner, b.owner);
        // Same slot key — both attach to the same registry slot (one REQ).
        assert_eq!(a.key, b.key);
        assert_eq!(a.scope, b.scope);
    }

    #[test]
    fn different_topics_have_distinct_slot_keys() {
        let btc = topic_articles_identity("bitcoin", CONSUMER);
        let zap = topic_articles_identity("zaps", CONSUMER);
        assert_ne!(btc.key, zap.key, "distinct topics must produce distinct slot keys");
    }

    #[test]
    fn start_rejects_empty_topic() {
        let mut ctx = ActionContext::default();
        let action = TopicArticlesAction::Claim {
            topic: String::new(),
            consumer_id: CONSUMER.to_string(),
        };
        assert!(matches!(
            TopicArticlesModule::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_empty_consumer_id() {
        let mut ctx = ActionContext::default();
        let action = TopicArticlesAction::Claim {
            topic: TOPIC.to_string(),
            consumer_id: String::new(),
        };
        assert!(matches!(
            TopicArticlesModule::start(&mut ctx, action),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn interest_id_is_stable_across_calls() {
        assert_eq!(
            topic_articles_interest_id(TOPIC),
            topic_articles_interest_id(TOPIC),
            "InterestId must be deterministic"
        );
        assert_ne!(
            topic_articles_interest_id("bitcoin"),
            topic_articles_interest_id("nostr"),
            "different topics must produce different InterestIds"
        );
    }
}
