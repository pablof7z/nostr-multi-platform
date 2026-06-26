---
name: nmp-conformance
description: Scan an app codebase that consumes NMP (nostr-multi-platform) for violations of the consumer-facing doctrine (D0–D10) and the framework-magic contract. USE WHEN review an NMP app, conformance scan, doctrine audit, check NMP usage, "is my app using NMP right", find bespoke Nostr code, audit framework usage, scan for doctrine violations.
---

# NMP app-conformance scanner

You scan an application that **consumes** NMP for drift from correct framework
usage, then produce a severity-ranked report. The catalog of what counts as a
violation is `catalog.md` next to this file — the consumer-facing re-cut of the
doctrine. **Read `catalog.md` first; it is the rule set you enforce.**

The core thesis you are testing: *the app should not re-implement what the
framework already does* (relay routing, dedup, supersession, reactivity, profile
freshness, error handling), and *all Nostr semantics live in Rust, not the
shell*. Every rule in the catalog is one way that thesis breaks.

## Workflow

### 1. Locate the NMP integration surface
Find where the app touches the framework — this scopes everything else:
- The C-ABI / FFI seam (`nmp_app_*`, `libnmp`, UniFFI-generated bindings).
- Where the app consumes snapshots / view payloads (the read side).
- Where the app invokes actions / publishes (the write side).
- Shell UI code (Swift/SwiftUI, Kotlin, TS/React).
Note the app's stack so you apply the right `Layer` column from the catalog.

### 2. Mechanical first pass (cheap, high-recall, low-precision)
For each catalog rule with a non-`semantic` Detection signature, grep the shell
for it. Collect **candidate** hits. Do not report them yet — greps over-fire
(a `created_at` in a comment, a `relays:` on an internal struct). This pass only
narrows where to look.

### 3. Semantic pass (the real work — LLM judgment)
This is where the scanner earns its keep. For each candidate hit **and** for
every `semantic` rule (which has no reliable grep), read the surrounding code
and judge against the catalog rule's intent:
- **A2 / bespoke reimplementation** — is this app talking to relays / signing /
  framing events itself, routing *around* `libnmp`? This is the top-priority
  find.
- **A1 / business logic in shell** — is the shell interpreting event kinds,
  reducing state, or encoding NIP semantics that belong in Rust?
- **I1 / composition** — is the app fetching kind:0 / resolving profiles itself
  instead of composing a self-claiming component?
- **D1–D5 / fallback code** — is the app re-implementing supersession, dedup,
  kind:3 watch, backfill scans, or profile-refresh polls the kernel already
  owns? (The framework API deliberately does *not* expose the question that
  would justify these — their presence means the app reached around the API.)
- **E1 / parallel state** — is there a SwiftData/Room store mirroring kernel
  facts?
Confirm or discard each candidate. **Discard aggressively** — a false positive
costs the builder's trust. When unsure, mark `warn` ("confirm intent"), not
`block`.

### 4. Adversarial verify the high-severity finds
Before reporting any `block`, re-examine it once as a skeptic: is there a
legitimate reason (an ADR waiver, a capability that genuinely must hold an OS
handle, a test fixture)? A `block` that survives skeptical re-reading ships;
one that doesn't drops to `warn` or is dropped. For a thorough audit, spawn
independent verifier agents per `block` finding and keep only those a majority
confirm.

### 5. Report
Emit an **ephemeral** report (this is a review — per repo rule it is **never
committed**; it is promoted to GitHub issues):
- Group by severity (`block` → `warn` → `note`).
- Per finding: `RULE-ID` · `file:line` · one-line *what* · one-line *why it
  reintroduces a bug the framework already handles* (cite the catalog Origin) ·
  the fix (usually: "delete this; the framework does it — see <builder-guide
  anchor>").
- End with a one-line verdict and the count by severity.
- Offer to (a) open GitHub issues for the `block` findings, or (b) apply fixes
  to the working tree if the user asks. Do not auto-commit either.

## Honesty rules
- **Thin-shell apps will be clean** on A1/A3/H1 by construction. Say so; don't
  manufacture findings. The catalog is a forcing function for *future* apps.
- **A grep hit is not a violation.** Only a confirmed semantic judgment is.
- **Cite the canon, don't re-derive it.** Every finding points to a catalog
  Origin (D0–D10 / C1–C13 / F-TTL); the *why* lives there, not in your report.
- **Don't invent rules.** If you spot drift the catalog doesn't cover, report it
  as a `note` flagged "catalog gap" and suggest a new rule — adding a rule means
  editing `catalog.md` (which the drift gate will then bind to a canon bullet).

## Distribution
This skill + catalog are authored in-repo (canon-adjacent, drift-gated). A
generated snapshot is scaffolded into consumer app repos via
`crates/nmp-defaults` so any app can run the scan without the NMP `docs/`
tree present. The in-repo copy is the source of truth; the scaffolded copy is
distribution.
