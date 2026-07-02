//! V-75 per-lane RouteAttempt observability tests.
//!
//! Verifies that `GenericOutboxRouter` emits one `RouteAttempt` per lane that
//! ran in the generic algorithm, with the correct `lane` and `outcome`. The
//! primary scenario is "lanes 1–6 empty, Lane 7 (AppRelayFallback) fires" on
//! both publish and subscribe paths.
//!
//! Split by behavior area (500-LOC hard cap) into `tests_v75/`:
//!   * `fixtures` — shared routing-context builders and the `AttemptCapture`
//!     trace observer.
//!   * `publish_path` — Lane-7 fallback core scenario, Lane-1-match
//!     suppresses-fallback, and the no-observer zero-alloc guard (D8).
//!   * `subscribe_path` — mirrors the publish-path scenarios on
//!     `route_subscription`.
//!   * `lane_attribution` — emission-order invariant, only-applicable-lanes
//!     emit, and the Hint-lane admissible-count fix.
//!
//! Companion files:
//! - `tests.rs` — lanes 1, 6, 7 + V-51 observer
//! - `tests_lanes.rs` — lanes 2/3/4 coverage
//! - `tests_v75/` — this module: per-lane RouteAttempt attribution (V-75)

#[path = "tests_v75/fixtures.rs"]
mod fixtures;

#[path = "tests_v75/publish_path.rs"]
mod publish_path;

#[path = "tests_v75/subscribe_path.rs"]
mod subscribe_path;

#[path = "tests_v75/lane_attribution.rs"]
mod lane_attribution;
