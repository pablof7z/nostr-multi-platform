---
type: research-record
date: 2026-06-11
session: 7f143c67-6e46-424a-90a8-5bf844947fee
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7f143c67-6e46-424a-90a8-5bf844947fee.jsonl
source_lines: 1155-1282
agent_attribution: a0b1dcda99d2cf66e
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "D0 violation audit of nmp-core kernel — found 12 instances of NIP/kind-specific knowledge in the pure substrate layer, tiered by migration priority"
captured_at: 2026-06-11T22:57:46Z
---

D0 violation audit of nmp-core kernel — found 12 instances of NIP/kind-specific knowledge in the pure substrate layer, tiered by migration priority

---

User: [Agent task result: Agent "Explore D0 violations in nmp-core projection layer" completed]
Perfect! Now let me create a comprehensive final report:

## D0 Audit Report: Kernel NIP/Kind-Specific Violations

I've completed a thorough audit of `crates/nmp-core/src/kernel/` for D0 violations (NIP-specific and kind-specific knowledge in the pure substrate kernel). I've identified **12 total violations**, including the one already documented in issue #920.

### Summary of Findings

**D0 Rule Violation**: `nmp-core` is a pure substrate (Layer 3) with zero NIP knowledge. It should own only:
- The snapshot envelope (`UpdateEnvelope`, `WireEnvelope`, `SNAPSHOT_SCHEMA_VERSION`)
- Generic substrate types (event storage, relay health, wire subscriptions)

Any struct/field encoding knowledge of a specific Nostr kind, NIP, or social concept is a violation.

### All Violations Found

#### **1. TimelineItem (Already filed — Issue #920)**
- **Location**: `crates/nmp-core/src/kernel/types.rs:108-158`
- **Violations**:
  - `is_repost: bool` — kind:6 (NIP-18) specific
  - `nav_target_id: String` — kind:6 routing logic
  - `repost_inner_content: String` — NIP-18 JSON parsing
  - `author_lnurl: Option<String>` — NIP-57 zap metadata
- **Correct Home**: `nmp-nip01` social timeline projection

#### **2. ingest/profile.rs — Kind:0 Metadata Ingest**
- **Location**: `crates/nmp-core/src/kernel/ingest/profile.rs:1-24`
- **Violation**: Module header states "Kind:0 (profile metadata) ingest"
- **Correct Home**: `nmp-nip01`

#### **3. ingest/contacts.rs — Kind:3 Contact List (NIP-02)**
- **Location**: `crates/nmp-core/src/kernel/ingest/contacts.rs:1-72`
- **Violation**: Module header states "Kind:3 (contact list) ingest"
- **Correct Home**: `nmp-nip02`

#### **4. ingest/timeline.rs — Kind:1 Metric Counter**
- **Location**: `crates/nmp-core/src/kernel/ingest/timeline.rs:214-216`
- **Code**: `if cached.kind == 1 { self.metric_note_events += 1; }`
- **Violation**: Hardcoded kind:1 → "note event" semantics
- **Correct Home**: `nmp-nip01`

#### **5. update/views.rs — Kind:6 Repost Parsing (NIP-18)**
- **Location**: `crates/nmp-core/src/kernel/update/views.rs:96-105`
- **Code**:
  ```rust
  let is_repost = event.kind == 6;
  let (nav_target_id, repost_inner_content) = if is_repost {
      let (inner_id, inner_content) = parse_repost_inner(&event.content);
      // ...
  }
  ```
- **Violations**: NIP-18 repost inner-event JSON parsing baked into timeline projection
- **Correct Home**: `nmp-nip01`

#### **6. update/helpers.rs — parse_repost_inner() NIP-18 Extractor**
- **Location**: `crates/nmp-core/src/kernel/update/helpers.rs:13-28`
- **Code Comment**: "Extract the two fields a kind:6 row needs from the NIP-18 embedded event JSON"
- **Violation**: Pure NIP-18 protocol parsing in substrate layer
- **Correct Home**: `nmp-nip01` or dedicated NIP-18 parser

#### **7. local_publish_intent.rs — Kind:3 Contacts Intent**
- **Location**: `crates/nmp-core/src/kernel/local_publish_intent.rs:30-32`
- **Code**: `if signed.unsigned.kind != 3 { return; }`
- **Violation**: Hardcoded kind:3 checking for local contact-list publishes
- **Correct Home**: `nmp-nip02`

#### **8. publish_outbox.rs — Kind-Based Title/Icon/Preview (Multiple NIPs)**
- **Location**: `crates/nmp-core/src/kernel/publish_outbox.rs:383-432`
- **Violations**:
  - `publish_event_title()` matches kinds 0, 1, 3, 7, 10002 to display strings
  - `publish_event_system_image()` matches kinds 0, 1, 3, 7, 10002 to SF Symbols
  - `publish_event_preview()` handles kinds 4, 44 (legacy DMs), plus other NIP-specific kinds
- **Correct Home**: Per-NIP projection builders (each NIP's display logic)

#### **9. nostr.rs — Kind:0 Profile Intent**
- **Location**: `crates/nmp-core/src/kernel/nostr.rs:84-90`
- **Code**: `if event.unsigned.kind != 0 { return None; }`
- **Violation**: `parse_profile_intent()` hardcoded to kind:0
- **Correct Home**: `nmp-nip01`

#### **10. auth.rs — Kind:22242 NIP-42 AUTH Validation**
- **Location**: `crates/nmp-core/src/kernel/auth.rs:84-88`
- **Code**: `if signed.unsigned.kind != 22242 { ... }`
- **Violation**: Hardcoded NIP-42 AUTH kind validation in substrate layer
- **Correct Home**: `nmp-nip42`

#### **11. reply.rs — Kind:1 Parent Check (NIP-10)**
- **Location**: `crates/nmp-core/src/kernel/reply.rs:51-53`
- **Code**: `if parent.kind != 1 { return None; }`
- **Violation**: NIP-10 reply-tag construction tied to kind:1 parents
- **Correct Home**: `nmp-nip01`
- **Architectural Note**: Blocked by cycle — `nmp-nip01 → nmp-core` already exists; reversing would create a dependency cycle

#### **12. ingest/mod.rs — Hardcoded Kind Dispatch**
- **Location**: `crates/nmp-core/src/kernel/ingest/mod.rs:400-443`
- **Code**:
  ```rust
  match event.kind {
      1 | 6 => { ingest_timeline_event(...) }
      0 => { ingest_profile(...) }
      3 => { ingest_contacts(...) }
      _ => { /* wildcard arm delegates to substrate parsers */ }
  }
  ```
- **Violations**: Hardcoded routing for kinds 0, 1, 3, 6
- **Architectural Mitigation**: The wildcard arm correctly delegates to substrate parsers (ADR-0009), but the explicit arms still couple the kernel to these specific kinds
- **Correct Home**: Architectural constraint — the ingest router mirrors the explicit (and limited) "kernel-owned" ingest paths for kinds the kernel naturally carries

### Recommendation Priority

**Tier 1 (High impact, clear ownership):**
1. TimelineItem → `nmp-nip01` (already filed)
2. parse_repost_inner & NIP-18 repost parsing → `nmp-nip01`
3. ingest/profile.rs → move logic to `nmp-nip01`
4. ingest/contacts.rs → move logic to `nmp-nip02`

**Tier 2 (Medium, cascading from Tier 1):**
5. nostr.rs parse_profile_intent → `nmp-nip01`
6. local_publish_intent.rs kind:3 check → `nmp-nip02`
7. ingest/timeline.rs kind:1 metric → `nmp-nip01`

**Tier 3 (Blocked or architectural):**
8. reply.rs kind:1 check → blocked by cycle (document as technical debt)
9. auth.rs kind:22242 → `nmp-nip42` (or accept as transport-layer exception)
10. publish_outbox.rs kind matching → refactor to per-NIP projection builders

I have updated issue #920 with this complete audit. The violations range from straightforward misplacements (ingest modules) to architectural cycles (reply.rs) that require dependency restructuring to resolve.
