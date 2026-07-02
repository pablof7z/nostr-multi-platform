//! Behavioral coverage for the kernel state-projection layer.
//!
//! ## What this file covers vs. what already exists
//!
//! `kernel/ingest_tests.rs` verifies the *in-memory* effect of ingest: after a
//! kind:0 / kind:3 / kind:10002 / kind:1, the right HashMap / VecDeque is
//! mutated. That is the reducer half of the kernel.
//!
//! This file covers the OTHER half — the **projection boundary**. The kernel's
//! `make_update()` serializes internal state into the JSON snapshot the FFI
//! returns to the Swift / Kotlin shell. A field that the reducer updates but the
//! projection never reads is invisible to users; a field the projection reads
//! from the wrong place shows stale state. Both are silent bugs that the
//! state-level ingest tests cannot catch.
//!
//! Every test here drives a real ingest / lifecycle transition, then calls
//! `kernel.make_update_json_for_test(true)` and asserts on the parsed `serde_json::Value` —
//! i.e. exactly the bytes that cross the C-ABI. `KernelUpdate` is `Serialize`
//! only (no `Deserialize`), so the assertions parse the JSON dynamically rather
//! than round-tripping the typed struct.
//!
//! Split by projection surface: [`liveness_projection_tests`] (schema_version /
//! last_tick_ms heartbeat), [`profile_projection_tests`] (profile-card shape),
//! [`publish_outbox_projection_tests`] (outbox + summary),
//! [`contacts_metrics_projection_tests`] (kind:3 stays out of metrics), and
//! [`relay_status_projection_tests`] (relay connection transitions).

mod projection_fixtures_support;

mod liveness_projection_tests;
mod profile_projection_tests;
mod publish_outbox_projection_tests;
mod contacts_metrics_projection_tests;
mod relay_status_projection_tests;
