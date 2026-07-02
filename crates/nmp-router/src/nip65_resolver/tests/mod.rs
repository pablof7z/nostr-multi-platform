//! Unit tests for `Nip65OutboxResolver`, split by test-scenario / behavior
//! area.
//!
//! T-publish-resolver-indexer (codex f81f735): the indexer-fallback tests
//! assert the fail-closed semantics — an author with no kind:10002 resolves
//! to an empty relay set, causing `NoTargets` upstream, rather than silently
//! widening to arbitrary public relays.
//!
//! ## Submodules
//!
//! - [`fixtures`] — shared relay-slot builders, `kind:10002` seeding
//!   helpers, `mk_resolver`, and the pubkey/relay constants.
//! - [`author_write_relays`] — code path 1: author kind:10002 write relays
//!   take precedence and carry `RelaySelectionReason::AuthorWriteRelay`.
//! - [`local_config_bootstrap`] — code path 2: the cold-start
//!   `local_write_relays` fallback, and the audit-finding-13 fail-closed
//!   regression guard for an explicitly-empty kind:10002 write set.
//! - [`discovery_indexer_fanout`] — code path 3: discovery-kind fan-out to
//!   indexer relays, including survival under p-tag threshold suppression.
//! - [`recipient_inbox_fanout`] — code path 4: recipient `#p` read-relay
//!   union, the fan-out threshold, and the `RecipientInbox` reason variant.
//! - [`explicit_targets`] — code path 5: `PublishTarget::explicit` pass
//!   through and its `Explicit` reason variant.
//! - [`malformed_and_edge_case_tags`] — kind:10002 input tolerance: malformed
//!   tags, unmarked-tag-is-both, and invalid-hex-author fail-closed.

mod author_write_relays;
mod discovery_indexer_fanout;
mod explicit_targets;
mod fixtures;
mod local_config_bootstrap;
mod malformed_and_edge_case_tags;
mod recipient_inbox_fanout;
