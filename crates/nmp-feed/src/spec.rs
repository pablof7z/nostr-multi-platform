//! Ergonomic feed descriptors over the canonical [`FeedParams`] contract.
//!
//! [`FeedSpec`] is builder sugar for app Rust code. It is not a second runtime
//! model: opening a spec first builds [`FeedParams`], then uses the same
//! feed-session compiler, registry, output key, and handle-owned lifecycle as
//! direct `FeedParams` callers.

use std::collections::BTreeSet;
use std::fmt;

use crate::params::{
    CustomPerspectiveId, FeedAdmission, FeedItemProjection, FeedKey, FeedOrder, FeedParams,
    FeedShape, FeedSourceExpr, FeedWindowPolicy, ListId, RelaySetId, TagTerm, WotRulesId, WotSeed,
};

/// Builder-style feed descriptor used by app code before compiling to
/// [`FeedParams`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedSpec {
    primary_kinds: Vec<u32>,
    source: Option<FeedSourceExpr>,
    admission: FeedAdmission,
    order: FeedOrder,
    window: FeedWindowPolicy,
    shape: FeedShape,
    item_projection: FeedItemProjection,
}

impl FeedSpec {
    /// Start an event-feed descriptor with no implicit source or primary kinds.
    #[must_use]
    pub fn events() -> Self {
        Self {
            primary_kinds: Vec::new(),
            source: None,
            admission: FeedAdmission::All,
            order: FeedOrder::NewestByFeedPosition,
            window: FeedWindowPolicy::default(),
            shape: FeedShape::default(),
            item_projection: FeedItemProjection::FeedRows,
        }
    }

    /// Set the app's primary content kinds.
    #[must_use]
    pub fn primary_kinds<I>(mut self, kinds: I) -> Self
    where
        I: IntoIterator<Item = u32>,
    {
        self.primary_kinds = kinds.into_iter().collect();
        self
    }

    /// Set the feed acquisition source expression.
    #[must_use]
    pub fn from(mut self, source: FeedSourceExpr) -> Self {
        self.source = Some(source);
        self
    }

    /// Set the projected feed shape.
    #[must_use]
    pub fn shape(mut self, shape: FeedShape) -> Self {
        self.shape = shape;
        self
    }

    /// Set the admission policy.
    #[must_use]
    pub fn admission(mut self, admission: FeedAdmission) -> Self {
        self.admission = admission;
        self
    }

    /// Set the ordering policy.
    #[must_use]
    pub fn order(mut self, order: FeedOrder) -> Self {
        self.order = order;
        self
    }

    /// Set the bounded window policy.
    #[must_use]
    pub fn window(mut self, window: FeedWindowPolicy) -> Self {
        self.window = window;
        self
    }

    /// Set the item projection / row schema contract.
    #[must_use]
    pub fn project(mut self, item_projection: FeedItemProjection) -> Self {
        self.item_projection = item_projection;
        self
    }

    /// Build canonical [`FeedParams`] under the caller-owned feed key.
    ///
    /// # Errors
    ///
    /// Returns [`FeedSpecError`] when the spec omits required app-owned intent:
    /// at least one primary kind and an explicit source expression.
    pub fn into_params(self, key: FeedKey) -> Result<FeedParams, FeedSpecError> {
        if self.primary_kinds.is_empty() {
            return Err(FeedSpecError::MissingPrimaryKinds);
        }
        let source = self.source.ok_or(FeedSpecError::MissingSource)?;
        Ok(FeedParams {
            primary_kinds: self.primary_kinds,
            shape: self.shape,
            source,
            admission: self.admission,
            order: self.order,
            window: self.window,
            key,
            item_projection: self.item_projection,
        })
    }
}

/// Typed feed-spec construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedSpecError {
    /// The app did not declare any primary content kind.
    MissingPrimaryKinds,
    /// The app did not declare the feed source expression.
    MissingSource,
}

impl fmt::Display for FeedSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedSpecError::MissingPrimaryKinds => f.write_str("feed spec has no primary kinds"),
            FeedSpecError::MissingSource => f.write_str("feed spec has no source"),
        }
    }
}

impl std::error::Error for FeedSpecError {}

impl FeedParams {
    /// Build canonical params from an ergonomic app feed spec.
    pub fn from_spec(key: FeedKey, spec: FeedSpec) -> Result<Self, FeedSpecError> {
        spec.into_params(key)
    }
}

/// App-facing feed builder functions.
pub mod feed {
    use super::FeedSpec;

    /// Start an event-feed descriptor.
    #[must_use]
    pub fn events() -> FeedSpec {
        FeedSpec::events()
    }
}

/// App-facing source-expression helpers.
pub mod source {
    use super::*;

    /// Select active-account-owned dynamic sources.
    #[must_use]
    pub fn active_user() -> ActiveUserSource {
        ActiveUserSource
    }

    /// Active-account source helper.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct ActiveUserSource;

    impl ActiveUserSource {
        /// The active account's current kind:3 follow set.
        #[must_use]
        pub fn follows(self) -> FeedSourceExpr {
            FeedSourceExpr::ActiveUserFollows
        }

        /// The active account's current hosted-group set.
        #[must_use]
        pub fn hosted_groups(self) -> FeedSourceExpr {
            FeedSourceExpr::ActiveUserHostedGroups
        }
    }

    /// Static author-set source.
    #[must_use]
    pub fn authors<I, S>(authors: I) -> FeedSourceExpr
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        FeedSourceExpr::Authors {
            authors: authors.into_iter().map(Into::into).collect::<BTreeSet<_>>(),
        }
    }

    /// A specific owner's contact-list source.
    #[must_use]
    pub fn contact_list(owner: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::ContactList {
            owner: owner.into(),
        }
    }

    /// App/defaults-registered list-member source.
    #[must_use]
    pub fn list_members(list: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::ListMembers {
            list: ListId(list.into()),
        }
    }

    /// App-registered relay-set source.
    #[must_use]
    pub fn relay_set(relays: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::RelaySet {
            relays: RelaySetId(relays.into()),
        }
    }

    /// Tag/search source.
    #[must_use]
    pub fn tag(term: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::Tag {
            term: TagTerm(term.into()),
        }
    }

    /// Referrer/thread source.
    #[must_use]
    pub fn referrer(event_id: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::Referrer {
            event_id: event_id.into(),
        }
    }

    /// Pointer-target hydration source. The type name remains explicit so this
    /// cannot be mistaken for ordinary primary feed acquisition.
    #[must_use]
    pub fn pointer_targets<I>(pointers: FeedSourceExpr, pointer_kinds: I) -> FeedSourceExpr
    where
        I: IntoIterator<Item = u32>,
    {
        FeedSourceExpr::PointerTargets {
            pointers: Box::new(pointers),
            pointer_kinds: pointer_kinds.into_iter().collect(),
        }
    }

    /// Web-of-trust source.
    #[must_use]
    pub fn wot(seed: impl Into<String>, rules: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::Wot {
            seed: WotSeed(seed.into()),
            rules: WotRulesId(rules.into()),
        }
    }

    /// Custom registered source/perspective.
    #[must_use]
    pub fn custom(id: impl Into<String>) -> FeedSourceExpr {
        FeedSourceExpr::CustomPerspectiveId(CustomPerspectiveId(id.into()))
    }

    /// Source union.
    #[must_use]
    pub fn union(left: FeedSourceExpr, right: FeedSourceExpr) -> FeedSourceExpr {
        FeedSourceExpr::Union(Box::new(left), Box::new(right))
    }

    /// Source intersection.
    #[must_use]
    pub fn intersection(left: FeedSourceExpr, right: FeedSourceExpr) -> FeedSourceExpr {
        FeedSourceExpr::Intersection(Box::new(left), Box::new(right))
    }

    /// Source difference.
    #[must_use]
    pub fn difference(left: FeedSourceExpr, right: FeedSourceExpr) -> FeedSourceExpr {
        FeedSourceExpr::Difference(Box::new(left), Box::new(right))
    }
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
