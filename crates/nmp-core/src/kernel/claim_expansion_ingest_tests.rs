//! Production-path tests for W5 claim-expansion controller.
//!
//! These tests drive claims through the ACTUAL `handle_text` / EOSE ingest
//! path (not by calling `on_claim_outcome_hit` / `on_claim_outcome_eose_no_match`
//! directly). They exercise the full chain:
//!
//!   resolve_ref → OneshotApi::request → drain_lifecycle_tick → planner REQ
//!   → register_wire_frames_for_test → claim_sub_index populated
//!   → handle_text(EVENT) → record_claim_expansion_hit → on_claim_outcome_hit
//!   → pending_claims empty, claim_sub_index empty
//!
//! This directly addresses the META codex finding: the 949 pre-fix tests
//! tested the controller in isolation and never exercised the production
//! ingest hook that wires W3 outcomes into the W5 state machine.
//!
//! Split by behavior area (#962 second wave) into `claim_expansion_ingest_tests/`:
//!   - `claim_expansion_ingest_support` — shared fixtures (signed
//!     event/article builders, refs.event sidecar lookup, wire-frame
//!     builders, wired-claim kernel setup).
//!   - `hit_termination_tests` — T-P1/T-P3: EVENT ingest and direct sub_id
//!     hit both drive Terminal(Hit) and drain claim_sub_index (B3).
//!   - `eose_phase_tests` — T-P2/T-P4/T-P5/T-P6: EOSE-no-match phase
//!     advance, relay-failed accounting, oneshot-slot invariant (B2), and
//!     per-relay EOSE attribution (B4).
//!   - `sibling_race_and_naddr_tests` — T-P7/T-P8: sibling EOSE race guard
//!     and naddr kind:30023 resolution via production wire ingest.

mod claim_expansion_ingest_support;
mod eose_phase_tests;
mod hit_termination_tests;
mod sibling_race_and_naddr_tests;
