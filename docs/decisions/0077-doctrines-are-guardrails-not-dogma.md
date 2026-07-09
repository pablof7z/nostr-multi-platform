# ADR-0077: Doctrines are advisory guardrails, not dogma

## Decision

Doctrines are advisory guardrails, not dogma — and may never block their own
improvement.

Doctrines exist for one reason: to keep bad design from creeping in. They
encode hard-won lessons; they are not ends in themselves. Therefore:

1. A doctrine is a strong default, overridable with a documented good reason.
   The escape hatch (`doctrine-allow:`) exists for genuine, justified
   exceptions; using it isn't failure — using it without a reason is.
2. No doctrine may be used to block its own improvement. When a doctrine is
   too strict, too narrow, miscategorized, or superseded by a better gate or
   design, the correct response is to change the doctrine — relax, harden,
   re-scope, or remove it — not to contort code to satisfy a wrong rule, and
   not to treat the rule as untouchable.
3. A one-off exception is not a systematic miscategorization. Reaching for the
   escape hatch repeatedly for the same legitimate pattern is a signal the
   doctrine is wrong — fix the rule, don't sprinkle allows.
4. A refined doctrine is still enforced. The goal is a correct gate, not no
   gate. And this meta-doctrine binds itself: if a better model emerges,
   change it too.

Canonical example (#3113): a display-separation rule that conflated a
canonical codec (hex↔bech32) with presentation-formatting would have forced
one codec reimplemented across three platforms — the exact SSOT violation the
rule's siblings exist to prevent. The rule, not the code, was wrong.

## Context

`doctrine-lint` (`crates/nmp-testing/bin/doctrine-lint/`) encodes dozens of
D-rules, each a codified regression lesson. That codification is valuable
exactly because it is durable — but durability was starting to be read as
immutability. #3113 caught the failure mode directly: D19/D27 banned
`crate::display::`/`to_npub`/`short_npub` as a single "display leak" category,
when `to_npub` (hex→bech32) is a deterministic, lossless, context-free codec —
the same class of conversion as hex↔base64 — and `short_npub` (truncation) is
a lossy, context-dependent presentation decision. Only the second belongs to
the shell. Enforcing the rule as written would have meant reimplementing the
bech32 codec once per native/wasm/TS shell: the precise single-source-of-truth
violation the display-separation doctrine's sibling rules exist to catch.

Without a standing principle, the reflexive response to a wrong doctrine
finding is either (a) contort the code to satisfy it, or (b) drop a
`doctrine-allow:` on it and move on — both leave the miscategorization in
place for the next contributor to trip over. Neither response is available
once the doctrine is understood as a means, not an end.

## Consequences

- A PR that relaxes, narrows, re-scopes, or removes a doctrine-lint rule is a
  normal, reviewable change — not a violation of the framework's integrity —
  provided the PR explains why the old rule was wrong and proves the
  refinement (regression fixtures showing what still fires and what is now
  correctly exempt).
- `doctrine-allow:` markers remain reserved for genuine one-off exceptions.
  Seeing the same marker recur across files for the same pattern is a signal
  to open an issue against the rule, not to keep copy-pasting the escape.
- This ADR is cited, not restated: `AGENTS.md` and the doctrine-lint source
  point here instead of duplicating the text (single source of truth per
  fact, per the repository's own planning-discipline rule).

## Boundaries

Permitted:

- relaxing, narrowing, re-scoping, or removing a doctrine-lint rule once it is
  shown to be wrong, miscategorized, or superseded, in the same PR that fixes
  the rule and proves the fix with fixtures/tests;
- a one-off `doctrine-allow: <rule> — <reason>` for a genuine, documented
  exception that does not generalize;
- amending this ADR itself when a better governance model emerges.

Forbidden:

- treating any doctrine-lint rule as untouchable and contorting product code
  to satisfy a rule that is itself wrong;
- reaching for `doctrine-allow:` repeatedly across files for the same
  legitimate pattern instead of fixing the rule that misclassifies it;
- disabling a doctrine wholesale (deleting the gate) as a substitute for
  refining it — the goal is a correct gate, not no gate;
- an escape-hatch marker with no reason, or a reason that does not survive
  the "is this a one-off, or is the rule wrong" test.

## Enforcement

This is a meta-doctrine about doctrines: no lint rule can judge whether a
doctrine change reflects genuine design improvement versus rule-dodging, so
enforcement is reviewer judgment against the boundaries above, same as any
other architectural decision. `doctrine-lint`'s stale-`doctrine-allow`
hardening (D27, #1712) already rejects markers that suppress nothing; this
ADR is the standing rationale contributors and reviewers cite when a doctrine
rule itself — not the code under it — is the thing that needs to change.

`crates/nmp-testing/bin/doctrine-lint/rules/mod.rs` and `AGENTS.md` point here
rather than restating the rule.

## Related

- [docs/decisions/0073-adr-reset-and-rolling-ratchets.md](0073-adr-reset-and-rolling-ratchets.md) -
  the analogous governance rule for the ADR directory itself.
- #3113 - D19/D27 codec-vs-presentation conflation (the canonical example
  above).
- #3110 - the `to_npub` doctrine-allow marker this ADR's refinement makes
  unnecessary.
- #1712 - stale-`doctrine-allow` hardening (D27).
