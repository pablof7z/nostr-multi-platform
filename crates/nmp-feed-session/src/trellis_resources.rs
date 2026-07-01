use std::hash::Hash;

use nmp_feed::{FeedRender, ProjectionKey};
use nmp_planner::{stable_hash::stable_hash64, InterestLifecycle, InterestScope, InterestShape};

const RESOURCE_NS: &str = "nmp.feed-session.resource.v1";
const SCOPE_NS: &str = "nmp.feed-session.scope.v1";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FeedSessionResourceKey {
    value: String,
}

impl FeedSessionResourceKey {
    fn new(value: String) -> Self {
        Self { value }
    }

    #[must_use]
    pub(crate) fn interest(demand: &InterestDemand) -> Self {
        Self::new(format!(
            "{RESOURCE_NS}:interest:scope={}:lifecycle={}:provenance={}:shape={}",
            demand.scope.key_part(),
            lifecycle_part(&demand.lifecycle),
            demand.provenance.key_part(),
            digest(("interest-shape", &demand.shape)),
        ))
    }

    #[must_use]
    pub(crate) fn projection(attachment: &ProjectionAttachment) -> Self {
        Self::new(format!(
            "{RESOURCE_NS}:projection:projection={}:render={}",
            digest(("projection", attachment.projection.as_str())),
            render_part(&attachment.render),
        ))
    }

    #[must_use]
    pub(crate) fn replay(demand: &ReplayDemand) -> Self {
        let interest_keys: Vec<String> = demand
            .interests
            .iter()
            .map(|interest| interest.resource_key().into_string())
            .collect();
        Self::new(format!(
            "{RESOURCE_NS}:replay:projection={}:interests={}",
            digest(("projection", demand.projection.as_str())),
            digest(("replay-interests", interest_keys)),
        ))
    }

    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.value
    }

    fn into_string(self) -> String {
        self.value
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct InterestDemand {
    pub(crate) scope: FeedSessionInterestScope,
    pub(crate) shape: InterestShape,
    pub(crate) lifecycle: InterestLifecycle,
    pub(crate) provenance: FeedSessionRouteProvenance,
}

impl InterestDemand {
    #[must_use]
    pub(crate) fn new(
        scope: &InterestScope,
        shape: InterestShape,
        lifecycle: InterestLifecycle,
        provenance: FeedSessionRouteProvenance,
    ) -> Self {
        Self {
            scope: FeedSessionInterestScope::from(scope),
            shape,
            lifecycle,
            provenance,
        }
    }

    #[must_use]
    pub(crate) fn tailing(
        scope: &InterestScope,
        shape: InterestShape,
        provenance: FeedSessionRouteProvenance,
    ) -> Self {
        Self::new(scope, shape, InterestLifecycle::Tailing, provenance)
    }

    #[must_use]
    pub(crate) fn resource_key(&self) -> FeedSessionResourceKey {
        FeedSessionResourceKey::interest(self)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum FeedSessionInterestScope {
    ActiveAccount,
    Account(String),
    Global,
}

impl FeedSessionInterestScope {
    fn key_part(&self) -> String {
        match self {
            Self::ActiveAccount => "active-account".to_string(),
            Self::Account(pubkey) => format!("account-{}", digest(("account", pubkey))),
            Self::Global => "global".to_string(),
        }
    }
}

impl From<&InterestScope> for FeedSessionInterestScope {
    fn from(value: &InterestScope) -> Self {
        match value {
            InterestScope::ActiveAccount => Self::ActiveAccount,
            InterestScope::Account(pubkey) => Self::Account(pubkey.clone()),
            InterestScope::Global => Self::Global,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum FeedSessionRouteProvenance {
    ActiveFollowTimeline,
    Nip51ListMembers,
    Nip29GroupTimeline,
    StaticFeedScope,
    SetAlgebra,
}

impl FeedSessionRouteProvenance {
    fn key_part(&self) -> &'static str {
        match self {
            Self::ActiveFollowTimeline => "active-follow-timeline",
            Self::Nip51ListMembers => "nip51-list-members",
            Self::Nip29GroupTimeline => "nip29-group-timeline",
            Self::StaticFeedScope => "static-feed-scope",
            Self::SetAlgebra => "set-algebra",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FeedSessionScopeKey {
    value: String,
}

impl FeedSessionScopeKey {
    #[must_use]
    pub(crate) fn projection(projection: &ProjectionKey) -> Self {
        Self {
            value: format!(
                "{SCOPE_NS}:projection={}",
                digest(("projection", projection.as_str()))
            ),
        }
    }

    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FeedSessionResourceCommand {
    OpenInterest(InterestDemand),
    CloseInterest(InterestDemand),
    ReplaceInterestSet(InterestSetDemand),
    ReplayFromStore(ReplayDemand),
    AttachProjection(ProjectionAttachment),
    DetachProjection(ProjectionAttachment),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InterestSetDemand {
    pub(crate) owner: FeedSessionScopeKey,
    pub(crate) children: Vec<InterestDemand>,
    pub(crate) reason: InterestSetReason,
}

impl InterestSetDemand {
    #[must_use]
    pub(crate) fn new(
        owner: FeedSessionScopeKey,
        mut children: Vec<InterestDemand>,
        reason: InterestSetReason,
    ) -> Self {
        children.sort_by_key(InterestDemand::resource_key);
        Self {
            owner,
            children,
            reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InterestSetReason {
    SourceChanged,
    SessionClosed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionAttachment {
    pub(crate) projection: ProjectionKey,
    pub(crate) render: FeedRender,
}

impl ProjectionAttachment {
    #[must_use]
    pub(crate) fn new(projection: ProjectionKey, render: FeedRender) -> Self {
        Self { projection, render }
    }

    #[must_use]
    pub(crate) fn resource_key(&self) -> FeedSessionResourceKey {
        FeedSessionResourceKey::projection(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReplayDemand {
    pub(crate) projection: ProjectionKey,
    pub(crate) interests: Vec<InterestDemand>,
}

impl ReplayDemand {
    #[must_use]
    pub(crate) fn new(projection: ProjectionKey, mut interests: Vec<InterestDemand>) -> Self {
        interests.sort_by_key(InterestDemand::resource_key);
        Self {
            projection,
            interests,
        }
    }

    #[must_use]
    pub(crate) fn resource_key(&self) -> FeedSessionResourceKey {
        FeedSessionResourceKey::replay(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostStatusIdentity {
    pub(crate) resource_key: FeedSessionResourceKey,
    pub(crate) scope: FeedSessionScopeKey,
    pub(crate) command_revision: u64,
}

impl HostStatusIdentity {
    #[must_use]
    pub(crate) fn new(
        resource_key: FeedSessionResourceKey,
        scope: FeedSessionScopeKey,
        command_revision: u64,
    ) -> Self {
        Self {
            resource_key,
            scope,
            command_revision,
        }
    }
}

fn digest(value: impl Hash) -> String {
    format!("{:016x}", stable_hash64(value))
}

fn lifecycle_part(value: &InterestLifecycle) -> &'static str {
    match value {
        InterestLifecycle::Tailing => "tailing",
        InterestLifecycle::OneShot => "one-shot",
    }
}

fn render_part(value: &FeedRender) -> &'static str {
    match value {
        FeedRender::OpCentric => "op-centric",
        FeedRender::Flat => "flat",
    }
}
