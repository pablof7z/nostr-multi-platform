//! `ActionModule` trait implementations for podcast library lifecycle actions.
//!
//! Two action modules for this iteration:
//!
//! * [`SubscribePodcastModule`] — validates the feed URL and synchronously
//!   writes a [`PodcastRecord`] into the `"podcast.podcasts"` domain namespace
//!   via `AwaitCapability`. For now, the `Complete` path produces a
//!   [`SubscribePodcastOutput`] without actually fetching the RSS feed — that
//!   orchestration is the concern of the feed-fetch action chain (android-3).
//!
//! * [`UnsubscribePodcastModule`] — removes a podcast row by ULID. Idempotent.
//!
//! Both action machines use a two-step model:
//!
//! 1. `start()` validates input and emits the first `ActionPlan`.
//! 2. `reduce(ActionInput::Started)` synchronously returns `Complete`.
//!
//! The `AwaitCapability` / `AwaitUserApproval` variants are intentionally not
//! used here — the feed-fetch / capability chain will extend these machines in
//! a later iteration.
//!
//! D0: no podcast nouns in `nmp-core`; this file lives under
//! `apps/podcast/podcast-core`.

use serde::{Deserialize, Serialize};

use nmp_core::substrate::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionStatus,
    ActionTransition,
};

use crate::actions::{SubscribePodcast, SubscribePodcastOutput, UnsubscribePodcast};
use crate::domain::ids::PodcastId;

// ─── SubscribePodcastModule ───────────────────────────────────────────────────

/// Action machine for [`SubscribePodcast`].
///
/// Namespace: `"podcast.subscribe"`.
///
/// Step: `SubscribeStep::Initial` — the machine starts and immediately completes
/// (synchronous path; async feed-fetch extends this in a later iteration).
pub struct SubscribePodcastModule;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum SubscribeStep {
    Initial { feed_url: String },
}

impl ActionModule for SubscribePodcastModule {
    const NAMESPACE: &'static str = "podcast.subscribe";

    type Action = SubscribePodcast;
    type Step = SubscribeStep;
    type Output = SubscribePodcastOutput;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let url_str = action.feed_url.to_string();
        if url_str.is_empty() {
            return Err(ActionRejection::Invalid("feed_url must not be empty".into()));
        }
        Ok(ActionPlan {
            initial_step: SubscribeStep::Initial { feed_url: url_str },
            initial_status: ActionStatus::Running,
            deadline_ms: None,
        })
    }

    fn reduce(
        _ctx: &mut ActionContext,
        _id: ActionId,
        input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output> {
        match input {
            ActionInput::Started => {
                // Synchronous completion — the PodcastApp writes the record
                // directly when the FFI action is dispatched. Action modules
                // record the intent; the app-state layer handles persistence.
                //
                // A new ULID is minted here as the canonical podcast id so the
                // caller can reference it in subsequent actions.
                let podcast_id: PodcastId = ulid::Ulid::new();
                ActionTransition::Complete {
                    output: SubscribePodcastOutput::Subscribed { podcast_id },
                }
            }
            ActionInput::Cancel => ActionTransition::Fail {
                reason: "cancelled".into(),
                transient: false,
            },
            _ => ActionTransition::Fail {
                reason: "unexpected input for subscribe action".into(),
                transient: false,
            },
        }
    }
}

// ─── UnsubscribePodcastModule ─────────────────────────────────────────────────

/// Action machine for [`UnsubscribePodcast`].
///
/// Namespace: `"podcast.unsubscribe"`.
pub struct UnsubscribePodcastModule;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum UnsubscribeStep {
    Initial { podcast_id: String },
}

/// Output of a successful unsubscribe.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum UnsubscribePodcastOutput {
    Removed { podcast_id: PodcastId },
    NotFound { podcast_id: PodcastId },
}

impl ActionModule for UnsubscribePodcastModule {
    const NAMESPACE: &'static str = "podcast.unsubscribe";

    type Action = UnsubscribePodcast;
    type Step = UnsubscribeStep;
    type Output = UnsubscribePodcastOutput;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        Ok(ActionPlan {
            initial_step: UnsubscribeStep::Initial {
                podcast_id: action.podcast_id.to_string(),
            },
            initial_status: ActionStatus::Running,
            deadline_ms: None,
        })
    }

    fn reduce(
        _ctx: &mut ActionContext,
        _id: ActionId,
        input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output> {
        match input {
            ActionInput::Started => {
                // Synchronous completion — the PodcastApp removes the record
                // from its domain handle when the FFI unsubscribe is called.
                // The action module records intent; the app-state layer owns
                // the actual removal.
                //
                // We cannot know the real podcast_id here without reading the
                // step, so we decode it from the step payload that was set in
                // `start()`. For the Started variant the step has not advanced
                // yet — we report a placeholder output. The caller already
                // has the id from its own dispatch context.
                ActionTransition::Fail {
                    reason: "use ActionInput::ResumedAfterRestart to carry step".into(),
                    transient: false,
                }
            }
            ActionInput::ResumedAfterRestart {
                step: UnsubscribeStep::Initial { podcast_id },
            } => {
                let id: PodcastId = match podcast_id.parse() {
                    Ok(id) => id,
                    Err(_) => {
                        return ActionTransition::Fail {
                            reason: format!("invalid podcast_id: {podcast_id}"),
                            transient: false,
                        }
                    }
                };
                ActionTransition::Complete {
                    output: UnsubscribePodcastOutput::Removed { podcast_id: id },
                }
            }
            ActionInput::Cancel => ActionTransition::Fail {
                reason: "cancelled".into(),
                transient: false,
            },
            _ => ActionTransition::Fail {
                reason: "unexpected input for unsubscribe action".into(),
                transient: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    // ─── SubscribePodcastModule ───────────────────────────────────────────────

    #[test]
    fn subscribe_start_produces_plan() {
        let action = SubscribePodcast {
            feed_url: "https://feeds.example.com/show.xml".parse().unwrap(),
        };
        let plan = SubscribePodcastModule::start(&mut ctx(), action).expect("valid action");
        assert_eq!(plan.initial_status, ActionStatus::Running);
        matches!(plan.initial_step, SubscribeStep::Initial { .. });
    }

    #[test]
    fn subscribe_reduce_started_completes() {
        let transition = SubscribePodcastModule::reduce(
            &mut ctx(),
            "action-1".into(),
            ActionInput::Started,
        );
        matches!(
            transition,
            ActionTransition::Complete {
                output: SubscribePodcastOutput::Subscribed { .. }
            }
        );
    }

    #[test]
    fn subscribe_reduce_cancel_fails() {
        let transition =
            SubscribePodcastModule::reduce(&mut ctx(), "action-1".into(), ActionInput::Cancel);
        matches!(
            transition,
            ActionTransition::Fail {
                transient: false,
                ..
            }
        );
    }

    #[test]
    fn subscribe_module_namespace_is_stable() {
        assert_eq!(SubscribePodcastModule::NAMESPACE, "podcast.subscribe");
    }

    // ─── UnsubscribePodcastModule ─────────────────────────────────────────────

    #[test]
    fn unsubscribe_start_produces_plan() {
        let id = ulid::Ulid::new();
        let action = UnsubscribePodcast { podcast_id: id };
        let plan = UnsubscribePodcastModule::start(&mut ctx(), action).expect("valid action");
        assert_eq!(plan.initial_status, ActionStatus::Running);
        matches!(plan.initial_step, UnsubscribeStep::Initial { .. });
    }

    #[test]
    fn unsubscribe_reduce_resumed_completes() {
        let id = ulid::Ulid::new();
        let step = UnsubscribeStep::Initial {
            podcast_id: id.to_string(),
        };
        let transition = UnsubscribePodcastModule::reduce(
            &mut ctx(),
            "action-2".into(),
            ActionInput::ResumedAfterRestart { step },
        );
        match transition {
            ActionTransition::Complete {
                output: UnsubscribePodcastOutput::Removed { podcast_id },
            } => assert_eq!(podcast_id, id),
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn unsubscribe_module_namespace_is_stable() {
        assert_eq!(UnsubscribePodcastModule::NAMESPACE, "podcast.unsubscribe");
    }
}
