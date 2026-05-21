//! D10 — provenance: gift-wrap publish never escapes to public relays.
//!
//! NIP-59 gift-wraps (kind:1059) — and the kinds they encapsulate (kind:13
//! seal, kind:14 chat-message rumor, kind:444 Marmot Welcome rumor) — MUST
//! NOT publish through the author's NIP-65 outbox (`PublishTarget::Auto`).
//! Doing so leaks the *existence* of an encrypted DM / Welcome to every
//! public relay the author advertises for normal traffic — defeating the
//! unlinkability the gift-wrap construction exists to provide.
//!
//! Legitimate destinations:
//!
//! - `PublishTarget::Explicit { relays }` — an EXPLICIT pin to the
//!   recipient's kind:10050 DM-inbox relays (NIP-17 § 2), the group's relays
//!   (the Marmot inbox-routing approximation), or another caller-supplied
//!   list.
//! - A pin derived from `recipient_dm_relays` (the kernel's kind:10050
//!   cache lookup helper).
//!
//! Documented in: `docs/doctrine/D10-provenance.md` (the provenance
//! doctrine; the routing rule is its outbound half).
//!
//! ## What this catches
//!
//! D10 is a **grep-lint** with **function-scope marker** opt-in (variant
//! (b) in PR-K's design — variant (a), a typed `PrivatePublishTarget`
//! wrapper, would have required restructuring the actor's `ActorCommand`
//! variants and the kernel's `nmp_app_publish_signed_event*` C-ABI symbols,
//! pushing the change well past PR-K's blast radius. The runtime
//! `publish_to` guard in `nmp-marmot/src/projection/publish.rs` is the
//! defense-in-depth complement to this lint).
//!
//! Inside any function whose body contains a `// D10: private-kind publish`
//! marker comment, D10 flags lines containing:
//!
//! - `PublishTarget::Auto`                       — the explicit Auto literal
//! - `publish_signed(`                            — the Auto-routing variant
//!                                                  (its `_to` sibling pins)
//! - `publish_signed_with_correlation(`           — Auto-routing twin
//! - `publish_unsigned_event(` (not `_to_relays`) — Auto-routing twin
//! - `publish_signed_event(`                      — the verified-publish entry
//!                                                  point in `commands::publish`
//!                                                  that maps empty relays →
//!                                                  Auto. A guarded caller MUST
//!                                                  prove non-emptiness before
//!                                                  the call (or it is a D10
//!                                                  leak by construction) and
//!                                                  attach a reasoned
//!                                                  `doctrine-allow: D10 — …`
//!                                                  annotation pointing to the
//!                                                  upstream guard.
//!
//! Each is a routing seam that resolves to the NIP-65 outbox; in a
//! private-kind publisher that is a D10 violation by construction. The
//! escape hatch `// doctrine-allow: D10 — <reason>` covers the rare
//! legitimately-Auto private-kind path (e.g. a runtime guard upstream of
//! the call has already proven `relays` non-empty). Unlike the generic
//! `allow::line_allows`, D10's per-rule [`line_allows_d10`] REQUIRES a
//! non-whitespace justification after the `— ` separator — a bare
//! `// doctrine-allow: D10` is rejected so every escape carries a written
//! reason a reviewer can audit.
//!
//! ## Scope (file allow-list)
//!
//! - `crates/nmp-core/src/` — the kernel's actor + publish surface.
//! - `crates/nmp-nip17/src/` — the NIP-17 builder + inbox projection.
//! - `crates/nmp-marmot/src/` — the Marmot MLS-over-Nostr projection.
//!
//! Outside this triplet the rule is silent (D10 is private-publish
//! oriented; other crates have no kind:1059 publishers).
//!
//! ## How to opt in a function
//!
//! ```ignore
//! fn send_gift_wrapped_dm(...) -> Vec<OutboundMessage> {
//!     // D10: private-kind publish
//!     // …subsequent lines must NOT route through Auto…
//!     let envelope = nmp_nip59::gift_wrap(keys, &recipient, rumor, None)?;
//!     let pin = kernel.recipient_dm_relays(&hex).unwrap_or_default();
//!     publish_signed_event(kernel, raw, &pin, None) // pin is the route
//! }
//! ```
//!
//! Without the marker the rule is dormant (zero findings on the current
//! tree). Adding the marker is the explicit opt-in: authors of a kind:1059
//! publisher take on the discipline of NEVER routing the envelope to the
//! author's public outbox.
//!
//! ## Known limitation — marker-in-docstring
//!
//! The tracker advances on every line including comments (its brace
//! counter ignores `//`-prefixed lines but still tests the *line text* for
//! the marker substring). A docstring that quotes the marker verbatim
//! (e.g. `/// See `// D10: private-kind publish`...`) inside a function
//! body would open the marked scope on that line and flag subsequent
//! Auto-routing seams. In practice no such docstring exists today; if one
//! is ever needed the author can refer to the rule by id ("D10") without
//! reproducing the literal marker substring.
//!
//! ## Why this is the right variant for PR-K
//!
//! The compile-time alternative (variant (a), a typed
//! `PrivatePublishTarget` constructible only from `Explicit { relays }` or
//! a future `Dm` variant) would force gift-wrap callers to use the wrapper
//! and make Auto-routing unrepresentable for kind:1059. It is the gold
//! standard. The blocker is the FFI seam: the kernel's
//! `nmp_app_publish_signed_event_to` C-ABI symbol takes a JSON relay list,
//! not a typed enum, and the `ActorCommand::PublishSignedEvent { raw,
//! relays, correlation_id }` actor variant is kind-agnostic on purpose
//! (D0 — the kernel does not branch on app-layer / NIP kind nouns). Adding
//! a parallel `PublishSignedPrivate` command + parallel FFI symbol is the
//! refactor variant (a) demands; that is multi-PR work. PR-K ships
//! variant (b) NOW with the marker-gated lint + the Marmot runtime guard
//! as the immediate defence; variant (a) remains a future option.

use std::path::Path;

pub const ID: &str = "D10";

/// True iff the file lives under one of the D10-scoped trees:
/// `crates/nmp-core/src/`, `crates/nmp-nip17/src/`, `crates/nmp-marmot/src/`.
/// Other crates have no kind:1059 publishers and stay out of scope.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/crates/nmp-core/src/")
        || s.starts_with("crates/nmp-core/src/")
        || s.contains("/crates/nmp-nip17/src/")
        || s.starts_with("crates/nmp-nip17/src/")
        || s.contains("/crates/nmp-marmot/src/")
        || s.starts_with("crates/nmp-marmot/src/")
}

/// The marker comment that opts a function into D10. Must appear inside
/// the function body on a line of its own (the brace tracker only enters
/// the marked state when it sees the marker AFTER opening the function
/// scope).
const MARKER: &str = "D10: private-kind publish";

/// The substrings a D10-marked function may NOT contain — each is an
/// Auto-routing publish signature. Plain substring match; the brace-aware
/// tracker scopes the check to marked functions only.
const BANNED_AUTO_ROUTES: &[&str] = &[
    "PublishTarget::Auto",
    // The Auto-variant kernel calls. Their `_to`-suffixed siblings
    // (`publish_signed_to`, `publish_signed_to_with_correlation`) pin
    // relays and are NOT flagged here.
    "publish_signed(",
    "publish_signed_with_correlation(",
    // The Auto-variant actor command. `publish_unsigned_event_to_relays`
    // is the Explicit-pin sibling and is NOT flagged.
    "publish_unsigned_event(",
    // `commands::publish::publish_signed_event` is the verified-publish
    // entry point used by the NIP-17 gift-wrap send path AND the dispatch
    // arm for `ActorCommand::PublishSignedEvent`. It maps `relays.is_empty()`
    // → `PublishTarget::Auto` (the documented D3 fallback), which is the
    // exact behaviour D10 forbids for a kind:1059 publish. A marked caller
    // MUST therefore prove non-emptiness upstream (a guard like
    // `is_empty_relays_kind1059_block` in `commands::dm`) and attach a
    // reasoned `doctrine-allow: D10 — <reason>` annotation pointing to that
    // guard. There is no `_with_correlation` sibling — `publish_signed_event`
    // already threads the correlation_id through its signature.
    "publish_signed_event(",
];

/// Per-file brace-depth tracker that decides whether the current line sits
/// inside a function whose body has declared the
/// `// D10: private-kind publish` marker.
///
/// Algorithm — mirrors `d8::HotPathTracker`:
///
/// 1. The brace counter advances on every line (including comments — the
///    walker reports brace counts conservatively).
/// 2. When a marker comment is observed at brace depth `d`, the tracker
///    remembers depth `d - 1` as the "marked scope opener" (the enclosing
///    fn body opens at `d - 1` and contains the marker at `d`). Subsequent
///    lines until the brace counter falls back to `d - 1` are
///    `in_marked_fn == true`.
/// 3. Multiple marked fns are tracked via a stack so nested or sibling
///    markers work correctly.
#[derive(Default)]
pub struct PrivatePublishTracker {
    /// Running brace depth across the file (all `{` minus all `}`).
    cur_depth: i32,
    /// Depths at which a marked function opened. Pop when `cur_depth`
    /// falls back to that depth (i.e. the marked fn closed).
    marked_open_depths: Vec<i32>,
}

impl PrivatePublishTracker {
    /// True iff the current cursor position is inside a marked function.
    pub fn in_marked_fn(&self) -> bool {
        !self.marked_open_depths.is_empty()
    }

    /// Advance the tracker by one source line. Must be called for every
    /// line in the file in order.
    ///
    /// `text` is the raw source line (the brace counter is whitespace- and
    /// comment-tolerant via the `count_braces_ignoring_strings` walker
    /// helper used by D8 — D10 instead does a coarse "count `{` minus
    /// `}` ignoring obvious comments" because braces in strings inside
    /// marked private-publish fns are not realistic).
    pub fn observe_line(&mut self, text: &str) {
        // Marker check FIRST — the depth at which the marker appears
        // determines the marked fn body's open depth. The marker MUST live
        // inside the fn body (depth ≥ 1) for the gate to make sense.
        if !text.contains("//") {
            // Marker is always inside a comment.
        } else if text.contains(MARKER) {
            // The marked fn body opened at `cur_depth - 1` (one shallower
            // than the marker itself). Track it ONCE per fn — a duplicate
            // marker inside the same fn must not push twice.
            let open_depth = self.cur_depth - 1;
            if !self.marked_open_depths.iter().any(|d| *d == open_depth) {
                self.marked_open_depths.push(open_depth);
            }
        }
        // Brace bookkeeping. Skip `//`-prefixed lines (line comments don't
        // contain real source braces in this codebase's style); for inline
        // mixed lines the lint accepts conservative undercounting because
        // marked private-publish fns are short and never embed braced
        // string literals in practice.
        let trimmed = text.trim_start();
        if !trimmed.starts_with("//") {
            let opens = text.chars().filter(|c| *c == '{').count() as i32;
            let closes = text.chars().filter(|c| *c == '}').count() as i32;
            self.cur_depth += opens;
            self.cur_depth -= closes;
        }
        // Pop any marked-fn entries whose open depth is now ≥ cur_depth
        // (the fn closed — the closing `}` dropped depth back to or below
        // the open depth).
        while let Some(&top) = self.marked_open_depths.last() {
            if self.cur_depth <= top {
                self.marked_open_depths.pop();
            } else {
                break;
            }
        }
    }
}

/// D10-specific escape-hatch parser.
///
/// Unlike the generic `allow::line_allows` (which accepts a bare
/// `// doctrine-allow: D10` with no reason), D10 REQUIRES a written
/// justification after a separator (`— ` or ` - `) so every escape carries
/// an auditable reason. The orchestrator deliberately scopes this tightening
/// to D10 only — other rules keep the lenient parser until they opt in to
/// their own per-rule variant.
///
/// Accepted shapes:
///
/// ```text
///     foo(); // doctrine-allow: D10 — kind:1059 empty-relay guarded above
///     foo(); // doctrine-allow: D10 - alternative ASCII-only separator
///     foo(); // doctrine-allow: D6,D10 — multi-rule annotation with reason
/// ```
///
/// Rejected shapes (D10 NOT silenced):
///
/// ```text
///     foo(); // doctrine-allow: D10
///     foo(); // doctrine-allow: D10 —
///     foo(); // doctrine-allow: D10 —    (only whitespace after separator)
///     foo(); // doctrine-allow: D10,D6   (no separator anywhere)
/// ```
///
/// A reason is "present" iff a non-whitespace character appears after the
/// separator. Multi-rule annotations only need ONE separator+reason for the
/// whole annotation (not one per rule id), mirroring the generic parser's
/// shape.
pub fn line_allows_d10(line: &str) -> bool {
    let Some(after) = line.split("// doctrine-allow:").nth(1) else {
        return false;
    };
    // Split at the first separator that signals "reason follows". Order:
    // em-dash first (preferred), then ASCII `" - "` fallback.
    let (head, reason) = if let Some((h, r)) = after.split_once('—') {
        (h, r)
    } else if let Some((h, r)) = after.split_once(" - ") {
        (h, r)
    } else {
        // No separator at all → no reason → D10 is NOT silenced.
        return false;
    };
    // The reason must contain at least one non-whitespace character.
    if reason.trim().is_empty() {
        return false;
    }
    // Head is the comma-separated rule-id list; the first whitespace-
    // delimited token of each chunk is the id. Mirrors the generic parser's
    // parsing for cross-rule consistency.
    head.split(',').any(|r| {
        r.split_whitespace()
            .next()
            .map(|t| t == ID)
            .unwrap_or(false)
    })
}

/// Per-line check: when `in_marked_fn` is true AND the line is not a
/// comment, fire a finding for each banned Auto-routing substring it
/// contains.
///
/// Returns `(column_1_indexed, message, suggested_fix)` tuples. Multiple
/// hits on a single line each produce one finding (each Auto seam is its
/// own violation).
pub fn check(line: &str, is_comment: bool, in_marked_fn: bool) -> Vec<(usize, String, String)> {
    if is_comment || !in_marked_fn {
        return Vec::new();
    }
    let mut out = Vec::new();
    for needle in BANNED_AUTO_ROUTES {
        if let Some(byte_pos) = line.find(needle) {
            let col = byte_pos + 1; // 1-indexed for clippy compatibility
            out.push((
                col,
                format!(
                    "`{}` is an Auto-routing publish seam — D10 forbids \
                     publishing private-kind events (kind:1059 / 13 / 14) \
                     through the author's NIP-65 outbox (it leaks the \
                     existence of the encrypted envelope to public relays)",
                    needle
                ),
                format!(
                    "route through `PublishTarget::Explicit {{ relays }}` (a \
                     recipient kind:10050 DM-inbox pin, the group's relays, \
                     or another explicit set) instead of `{}`; see \
                     `publish_signed_event(..., &pin, ...)` or its `_to_relays` \
                     siblings",
                    needle
                ),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── file_in_scope ────────────────────────────────────────────────────

    #[test]
    fn file_in_scope_includes_target_crates() {
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-core/src/actor/commands/dm.rs"
        )));
        assert!(file_in_scope(&PathBuf::from("crates/nmp-nip17/src/action.rs")));
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-marmot/src/projection/publish.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "/abs/path/crates/nmp-core/src/lib.rs"
        )));
    }

    #[test]
    fn file_in_scope_excludes_other_crates() {
        // D10 is private-kind-publish-oriented; other crates have no
        // kind:1059 publishers and stay silent.
        assert!(!file_in_scope(&PathBuf::from("crates/nmp-nip29/src/lib.rs")));
        assert!(!file_in_scope(&PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/ffi.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from("crates/nmp-testing/src/lib.rs")));
    }

    // ── PrivatePublishTracker ────────────────────────────────────────────

    fn tracker_states(src: &str) -> Vec<bool> {
        // Mirror the driver's call order: observe a line, then the state
        // reported for THAT line is the state AT START of the line. Since
        // the tracker advances the brace counter inside `observe_line`,
        // we record `in_marked_fn` BEFORE the call (state at line start).
        let mut tracker = PrivatePublishTracker::default();
        let mut states = Vec::new();
        for line in src.lines() {
            states.push(tracker.in_marked_fn());
            tracker.observe_line(line);
        }
        states
    }

    #[test]
    fn unmarked_fn_is_never_in_scope() {
        let src = "fn plain() {\n    let x = 1;\n    println!(\"{}\", x);\n}\n";
        let s = tracker_states(src);
        assert!(s.iter().all(|b| !*b), "no marker → never in scope: {:?}", s);
    }

    #[test]
    fn marked_fn_body_is_in_scope() {
        // The marker sits inside the fn body; subsequent body lines must
        // report `in_marked_fn == true`. The `fn ... {` line itself opens
        // the scope, so its state at line start is still false.
        let src = "\
fn send() {
    // D10: private-kind publish
    let pin = recipient_dm_relays();
    publish(&envelope, &pin);
}
fn after() {
    let _ = 1;
}
";
        let s = tracker_states(src);
        // Line 1 `fn send() {` — depth 0 at start, no marker yet → false.
        assert!(!s[0]);
        // Line 2 — marker line. State BEFORE observing is still false
        // (marker not yet seen at this point); the assertion that matters
        // is the lines AFTER the marker.
        assert!(!s[1]);
        // Lines 3, 4 — inside marked fn body. True.
        assert!(s[2], "body after marker must be in scope: {:?}", s);
        assert!(s[3], "second body line must still be in scope: {:?}", s);
        // Line 5 `}` — state at start is still inside (we haven't yet
        // processed the closing brace).
        assert!(s[4]);
        // Line 6 `fn after() {` — after the marked fn closed.
        assert!(!s[5]);
        assert!(!s[6]);
    }

    #[test]
    fn second_unmarked_fn_remains_out_of_scope() {
        let src = "\
fn send() {
    // D10: private-kind publish
    let pin = recipient_dm_relays();
}
fn normal() {
    let _ = PublishTarget::Auto;
}
";
        let s = tracker_states(src);
        // The Auto literal in `normal()` must NOT report in_marked_fn.
        assert!(!s[5], "unmarked fn body must not be in scope: {:?}", s);
    }

    // ── check ────────────────────────────────────────────────────────────

    #[test]
    fn check_silent_when_out_of_marked_fn() {
        // Outside a marked fn body, even the most blatant Auto literal is
        // ignored — that's the whole "opt-in marker" design.
        assert!(check("    target: PublishTarget::Auto,", false, false).is_empty());
        assert!(check("    kernel.publish_signed(&signed, &[])", false, false).is_empty());
    }

    #[test]
    fn check_flags_publish_target_auto_inside_marked_fn() {
        let hits = check("        target: PublishTarget::Auto,", false, true);
        assert_eq!(hits.len(), 1, "exactly one D10 finding expected");
        assert!(
            hits[0].1.contains("PublishTarget::Auto"),
            "message must name the offending token: {}",
            hits[0].1
        );
        assert!(
            hits[0].1.contains("D10"),
            "message must mention D10: {}",
            hits[0].1
        );
    }

    #[test]
    fn check_flags_publish_signed_auto_variant_inside_marked_fn() {
        // The `publish_signed(` form (NOT `_to`) is Auto-routing. The
        // `_to` sibling pins explicit relays and is NOT flagged.
        let hits = check("    kernel.publish_signed(&signed, &[])", false, true);
        assert_eq!(hits.len(), 1, "publish_signed( must fire D10: {:?}", hits);
    }

    #[test]
    fn check_does_not_flag_publish_signed_to_variant() {
        // `publish_signed_to` is the Explicit-pin variant — never D10.
        let hits = check(
            "    kernel.publish_signed_to(&signed, &[], target)",
            false,
            true,
        );
        assert!(
            hits.is_empty(),
            "publish_signed_to (Explicit variant) must NOT fire D10: {:?}",
            hits
        );
    }

    #[test]
    fn check_flags_publish_signed_with_correlation_auto_variant() {
        let hits = check(
            "        kernel.publish_signed_with_correlation(&signed, &[], None)",
            false,
            true,
        );
        assert_eq!(
            hits.len(),
            1,
            "publish_signed_with_correlation( must fire D10: {:?}",
            hits
        );
    }

    #[test]
    fn check_does_not_flag_publish_signed_to_with_correlation() {
        // The `_to_with_correlation` variant carries an explicit
        // `PublishTarget` argument — it's the Explicit-pin variant and is
        // NOT a D10 violation.
        let hits = check(
            "    kernel.publish_signed_to_with_correlation(&signed, &[], target, None)",
            false,
            true,
        );
        assert!(
            hits.is_empty(),
            "publish_signed_to_with_correlation (Explicit variant) must NOT fire D10: {:?}",
            hits
        );
    }

    #[test]
    fn check_flags_publish_unsigned_event_auto_variant() {
        // The Auto-routing actor command — the Explicit sibling is
        // `publish_unsigned_event_to_relays`.
        let hits = check(
            "    commands::publish_unsigned_event(identity, kernel, unsigned, ps);",
            false,
            true,
        );
        assert_eq!(
            hits.len(),
            1,
            "publish_unsigned_event( must fire D10: {:?}",
            hits
        );
    }

    #[test]
    fn check_does_not_flag_publish_unsigned_event_to_relays() {
        let hits = check(
            "    commands::publish_unsigned_event_to_relays(id, kernel, ev, relays, ps);",
            false,
            true,
        );
        assert!(
            hits.is_empty(),
            "publish_unsigned_event_to_relays must NOT fire D10: {:?}",
            hits
        );
    }

    #[test]
    fn check_ignores_comment_lines() {
        // The comment must NEVER fire — comments document banned tokens.
        let hits = check(
            "    /// `PublishTarget::Auto` resolves via the outbox.",
            true,
            true,
        );
        assert!(hits.is_empty(), "comment lines must never fire D10");
    }

    #[test]
    fn check_reports_column_at_token_start() {
        // The column points at the offending substring so a developer can
        // jump straight to it.
        let line = "    target: PublishTarget::Auto,";
        let hits = check(line, false, true);
        assert_eq!(hits.len(), 1);
        let expected_col = line.find("PublishTarget::Auto").unwrap() + 1;
        assert_eq!(
            hits[0].0, expected_col,
            "column must point at the start of the offending token"
        );
    }

    #[test]
    fn check_emits_one_finding_per_banned_token_on_a_line() {
        // A pathological "two violations on one line" must produce two
        // separate findings — each is its own structural offence.
        let line = "publish_signed(&ev, &[]); /* and */ publish_unsigned_event(id, kernel, ue, ps);";
        let hits = check(line, false, true);
        assert_eq!(
            hits.len(),
            2,
            "each banned token must produce its own finding: {:?}",
            hits
        );
    }

    #[test]
    fn check_silent_inside_unmarked_fn_even_with_auto() {
        // The marker-gate is what scopes D10; without it the rule is
        // dormant, no matter how many Auto-routing seams a line has.
        let line = "publish_signed(&ev, &[]); /* and */ publish_unsigned_event(id, kernel, ue, ps);";
        let hits = check(line, false, false);
        assert!(
            hits.is_empty(),
            "unmarked-fn lines must NEVER produce D10 findings: {:?}",
            hits
        );
    }

    // ── banned-list: publish_signed_event ────────────────────────────────

    #[test]
    fn check_flags_publish_signed_event_inside_marked_fn() {
        // `commands::publish::publish_signed_event` maps `relays.is_empty()`
        // → `PublishTarget::Auto`. Inside a marked kind:1059 publisher that
        // mapping is a D10 leak by construction — the call must be either
        // guarded upstream (and annotated `doctrine-allow: D10 — …`) or
        // refactored to a non-Auto entry point.
        let hits = check(
            "    outbound.extend(super::publish::publish_signed_event(kernel, raw, &relays, None));",
            false,
            true,
        );
        assert_eq!(
            hits.len(),
            1,
            "publish_signed_event( inside a marked fn must fire D10: {:?}",
            hits
        );
        assert!(
            hits[0].1.contains("publish_signed_event"),
            "the finding message must name the offending token: {}",
            hits[0].1
        );
    }

    #[test]
    fn check_does_not_flag_publish_signed_event_outside_marked_fn() {
        // The `commands::publish::publish_signed_event` call in
        // `actor::dispatch::PublishSignedEvent` is the generic dispatch arm,
        // NOT inside a marked private-kind publisher. It must stay silent —
        // the marker is the opt-in.
        let hits = check(
            "    commands::publish_signed_event(ctx.kernel, raw, &relays, correlation_id);",
            false,
            false,
        );
        assert!(
            hits.is_empty(),
            "publish_signed_event in an unmarked dispatch arm must NOT fire D10: {:?}",
            hits
        );
    }

    // ── line_allows_d10 (tightened escape hatch) ─────────────────────────

    #[test]
    fn line_allows_d10_requires_em_dash_reason() {
        let line = "    foo(); // doctrine-allow: D10 — kind:1059 empty-relay guarded above";
        assert!(
            line_allows_d10(line),
            "an em-dash separator with a non-empty reason must silence D10"
        );
    }

    #[test]
    fn line_allows_d10_accepts_ascii_separator() {
        let line = "    foo(); // doctrine-allow: D10 - guarded above";
        assert!(
            line_allows_d10(line),
            "the ASCII ` - ` fallback separator must also silence D10"
        );
    }

    #[test]
    fn line_allows_d10_rejects_bare_annotation() {
        // The whole point of the tightened parser: a bare
        // `// doctrine-allow: D10` (no separator, no reason) must NOT
        // silence the rule. Authors must justify the escape.
        let line = "    foo(); // doctrine-allow: D10";
        assert!(
            !line_allows_d10(line),
            "a bare D10 annotation with no reason must NOT silence the rule"
        );
    }

    #[test]
    fn line_allows_d10_rejects_empty_reason_after_separator() {
        // A separator with only whitespace after it does not count as a
        // written reason.
        let line = "    foo(); // doctrine-allow: D10 —    ";
        assert!(
            !line_allows_d10(line),
            "whitespace-only after the separator does not count as a reason"
        );
        let line_ascii = "    foo(); // doctrine-allow: D10 -    ";
        assert!(
            !line_allows_d10(line_ascii),
            "whitespace-only after the ASCII separator must also fail"
        );
    }

    #[test]
    fn line_allows_d10_rejects_no_annotation() {
        // No annotation at all → not silenced.
        assert!(!line_allows_d10("    foo();"));
    }

    #[test]
    fn line_allows_d10_works_inside_multi_rule_annotation() {
        // The reason lives once at the end of the multi-rule comma list;
        // D10 must recognize itself as one of the listed ids and accept
        // the shared reason.
        let line = "    foo(); // doctrine-allow: D6,D10 — shared reason";
        assert!(
            line_allows_d10(line),
            "D10 must be recognized inside a multi-rule annotation"
        );
        let line_other_only = "    foo(); // doctrine-allow: D6,D7 — shared reason";
        assert!(
            !line_allows_d10(line_other_only),
            "D10 absent from the id list must NOT be silenced"
        );
    }

    #[test]
    fn line_allows_d10_rejects_when_other_rule_has_reason_but_d10_not_listed() {
        // Sanity: an annotation that explicitly excludes D10 cannot
        // accidentally pick up the silencing via the reason text.
        let line = "    foo(); // doctrine-allow: D8 — sleep is legitimate in this bench";
        assert!(!line_allows_d10(line));
    }
}
