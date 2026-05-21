//! Marmot `ActionModule` impls.
//!
//! `PublishKeyPackageAction` — validates relay-list coverage before the actor
//! calls `service::MarmotService::publish_key_package`. Per ADR-0025
//! Constraint #1, capabilities without handle-scoped MLS state MUST be routed
//! through `dispatch_action`; this is the first such Marmot action wired.

mod actions;

pub use actions::{PublishKeyPackageAction, PublishKeyPackageInput};
