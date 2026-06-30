//! Layer-inversion doctrine gate — durable backstop for the 2026-06 crate-layer
//! audit (issues #2510, #2508, #2512, #2513, #2514, #2515).
//!
//! A "layer inversion" is a sub-L5 crate owning a concern that belongs to a
//! higher layer: render / feed-item shape, display enrichment, an app-named
//! noun, global relation summaries, or a substrate naming a protocol noun.
//! `docs/architecture/crate-boundaries.md` (§2-§10a) is the durable spec; this
//! grep gate is the CI ratchet that prevents *new* inversions from being
//! introduced while the audited debt is paid down.
//!
//! Four independent rules, each scoped to the layer it protects:
//!
//! * **Rule A — display-enrichment-in-primitive.** L1/L4 protocol primitives
//!   (`nmp-nip01`, `nmp-content`, every L4 `nmp-nipNN`, `nmp-feed`,
//!   `nmp-threading`) must not carry kind:0 display strings or rendered
//!   previews as struct/table *fields*. The kind:0 `Profile*` vocabulary is
//!   carved out (it is the legitimate owner of display data).
//! * **Rule B — rejected relation-summary vocabulary.** Reusable crates must
//!   not expose global relation summaries, bucket APIs, `NoteRelationCounts`,
//!   `NoteRelationClassifier`, `TargetInteractionCounts`, or a central
//!   `nmp-relations` owner. Existing debt is capped to open issues #2508 and
//!   #2512 only; new concept-owned active reads must live with their concept
//!   owner instead.
//! * **Rule C — kind-blind transport (`nmp-nip29`).** NIP-29 is kind-blind
//!   `h`-tag transport: it owns ONE generic publish-into-group verb plus pure
//!   envelope/admin ops. Kind-specific verbs (`react`/`repost`/`share`) and
//!   kind constants must not live here.
//! * **Rule D — substrate protocol-noun (`nmp-core`).** The substrate kernel
//!   must not own NIP-19 entity codecs (`nip19`, `Nip19Entity`,
//!   `Nprofile`/`Nevent`/`Naddr` types). NIP-21 `NostrUri` and `parse_nip10`
//!   were judged legitimate generic substrate codecs by the audit and are NOT
//!   banned.
//!
//! # Baseline ratchet
//!
//! The audited violations still exist on `master`, so each rule carries an
//! explicit BASELINE ALLOWLIST of known-violation files, annotated with their
//! owning open issue. A rule fails only on occurrences NOT in its baseline.
//! Baseline entries are tracked debt; the owning fix PR removes its line when it
//! lands. Do NOT add new entries — a new violation must be fixed, not baselined.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test layer_inversion_doctrine_lint
//! ```

#[path = "layer_inversion_doctrine_lint/rule_a.rs"]
mod rule_a;
#[path = "layer_inversion_doctrine_lint/rule_b.rs"]
mod rule_b;
#[path = "layer_inversion_doctrine_lint/rule_b_baseline.rs"]
mod rule_b_baseline;
#[path = "layer_inversion_doctrine_lint/rule_b_matchers.rs"]
mod rule_b_matchers;
#[path = "layer_inversion_doctrine_lint/rule_c.rs"]
mod rule_c;
#[path = "layer_inversion_doctrine_lint/rule_d.rs"]
mod rule_d;
#[path = "layer_inversion_doctrine_lint/support.rs"]
mod support;

#[cfg(test)]
#[path = "layer_inversion_doctrine_lint/matcher_tests.rs"]
mod matcher_tests;
