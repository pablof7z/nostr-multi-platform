//! Compatibility re-export facade for kernel data types.
//!
//! #962 — the kernel data model that previously piled into one `types.rs`
//! god-file now lives in cohesive per-owner sibling modules. Every type is
//! re-exported here so the established `super::types::…` / `types::…` import
//! paths across the kernel resolve unchanged (pure move, no behaviour change).
//!
//! | Owner module | Concern |
//! |--------------|---------|
//! | [`super::read_cache`] | Timeline read-cache entry (`StoredEvent`) |
//! | [`super::profile_card`] | Raw kind:0 profile card (`ProfileCard`) |
//! | [`super::relay_health`] | Per-relay transport health + wire-sub state + their projections |
//! | [`super::kernel_snapshot`] | Per-tick host update envelope + metrics/timing sub-state |
//! | [`super::publish_outbox_dto`] | Publish-outbox projection DTOs |
//! | [`super::claimed_event_dto`] | `refs.event` claimed-event row payload |

pub(super) use super::read_cache::StoredEvent;

pub(super) use super::profile_card::ProfileCard;

// `pub(crate)` so the feature-gated `crate::codegen_schema` re-exports can name
// these projection types (see the `…ForCodegen` aliases in `kernel/mod.rs`).
pub(super) use super::relay_health::{
    Counters, NoticeEntry, RelayHealth, WireSubscriptionState, MAX_NOTICE_LOG,
};
pub(crate) use super::relay_health::{LogicalInterestStatus, RelayStatus, WireSubscriptionStatus};

pub(super) use super::kernel_snapshot::{DiagnosticFirehoseState, TimingMilestones};
pub(crate) use super::kernel_snapshot::{KernelSnapshot, Metrics};

pub(super) use super::publish_outbox_dto::{
    OutboxSummarySnapshot, PublishOutboxItem, PublishOutboxRelay,
};

pub(crate) use super::claimed_event_dto::ClaimedEventDto;

// External-owned types re-exported through `types::` for path stability.
// `WireSub` lives in `kernel/wire_sub.rs`; the negentropy stats/const live in
// `kernel/negentropy_types.rs`.
pub(crate) use super::negentropy_types::NegentropySyncStats;
pub(super) use super::negentropy_types::AVG_EVENT_BYTES;
pub(super) use super::wire_sub::WireSub;
