# Per-Relay Publish Status with Selection Rationale

## What We Want

Every event the kernel publishes should be inspectable by the user, from any app, with no app-side logic required. The user should be able to select any outbox item — published, pending, retrying, failed, anything — and see:

1. **Which relays** the kernel targeted for that event.
2. **What the current status is** for each relay (pending, sending, ok, retrying, failed).
3. **Why that relay was targeted** — a plain-English explanation that the backend computes and ships pre-formatted:
   - "Your NIP-65 write relay" — relay comes from the signer's own kind:10002
   - "App relay (local config)" — relay is configured in the app; no kind:10002 has been published yet
   - "Discovery indexer relay" — relay receives all replaceable events (kind 0/3/10000–19999) so they are discoverable
   - "Inbox relay for npub1abc…" — relay is a read relay of the author tagged in a `#p` tag; NIP-65 fan-out rule
   - "Explicit relay" — caller passed `PublishTarget::Explicit`; the user or app chose this relay directly
4. **Why the current status is what it is** — the `message` field already exists on `PublishOutboxRelay` ("Waiting for relay connection", "Relay accepted the event", etc.) and already works. The new `reason` field is additive.

The design principle: **the backend owns all of this.** Any app — iOS, Android, chirp-tui, a future web shell, a CLI tool — renders these pre-formatted strings verbatim. No app needs to understand NIP-65 indexer logic, p-tag fanout thresholds, or relay role semantics.

---

## Root Problem Being Solved

### Immediate bug (already fixed)

`publish_outbox_status()` checked for any `Pending` relay state before checking for any `Ok` state. When relay.primal.net accepted a reaction (`Ok`) but an author's read relay (added by p-tag fan-out) never connected and stayed `Pending`, the overall status was "Pending" even though the event was published. Fixed by checking for `Ok` before `Pending`.

### The deeper design gap this PR addresses

Neither the kernel's outbox projection nor any shell exposes WHY a relay was selected. This is a usability problem and a debugging problem:

- A user sees "Pending" on a relay URL they didn't configure and doesn't know why it's in the list.
- A developer can't tell from the outbox whether a relay is there because it's the user's own relay or because some p-tagged author's kind:10002 lists it as a read relay.
- The TUI never exposes per-relay detail at all — OutboxLine strips the relay array entirely before sending to the UI.

### Why reactions always went to "Pending"

When a user reacts to an event by author A:

1. `react()` in `commands/publish.rs` builds a kind:7 event with a `p` tag pointing at A.
2. `Nip65OutboxResolver::resolve()` runs the p-tag fan-out step: for each `#p` pubkey, it adds that pubkey's kind:10002 **read** relays to the publish set. If A's kind:10002 was fetched during timeline loading, A's read relays are included.
3. The publish targets become: `{ "wss://relay.primal.net": Pending, "wss://A_read_relay": Pending }`.
4. Both start InFlight. relay.primal.net responds OK → `Ok`. A's relay fails → `mark_relay_unavailable()` reverts InFlight back to `Pending`.
5. Final state: `{ "wss://relay.primal.net": Ok, "wss://A_read_relay": Pending }`.
6. `publish_outbox_status()` (before fix) checked Pending before Ok → "pending" forever.
7. `is_complete()` requires ALL states to be terminal → row never evicts from outbox.

The fix addresses point 6. This PR addresses the user's inability to understand point 2–5 without reading source code.

---

## Full Architecture of the Change

### Current pipeline (what exists)

```
PublishAction
    → start_publish_inner()
        → OutboxResolver::resolve()          returns BTreeSet<RelayUrl>   ← REASON DISCARDED HERE
        → canonicalize_relay_set()
        → BTreeMap<RelayUrl, PerRelayState>  all Pending
        → InFlight { per_relay, ... }        no reason stored
    → dispatch_pending()
        → per relay: Pending → InFlight → Ok/Error/Timeout

Kernel snapshot:
    publish_outbox_items()
        → PublishOutboxRelay { url, status, status_label, attempt, attempt_label, message }
                                                                          ← NO REASON FIELD
        → PublishOutboxItem { relays: Vec<PublishOutboxRelay>, ... }
        → JSON

TUI snapshot consumer:
    outbox_from() in feature_snapshot.rs
        → OutboxLine { handle, title, status_label, preview, can_retry }
                                                                          ← RELAY ARRAY STRIPPED
```

### Target pipeline (what this PR builds)

```
PublishAction
    → start_publish_inner()
        → OutboxResolver::resolve()          returns Vec<ResolvedRelay>   ← REASON PRESERVED
        → split into per_relay map + relay_reasons map
        → InFlight { per_relay, relay_reasons, ... }                      ← STORED

Kernel snapshot:
    publish_outbox_items()
        → looks up relay_reasons for each URL
        → PublishOutboxRelay { url, status, status_label, attempt, attempt_label, message, relay_reason }
                                                                          ← NEW FIELD
        → PublishOutboxItem { relays: Vec<PublishOutboxRelay>, ... }
        → JSON (relay_reason: "" is omitted via skip_serializing_if)

TUI snapshot consumer:
    outbox_from() in feature_snapshot.rs
        → OutboxLine { handle, title, status_label, preview, can_retry, relays: Vec<OutboxRelayLine> }
                                                                          ← RELAY ARRAY EXPOSED

TUI UI:
    Settings pane 3 (Outbox)
        → list of items, j/k navigates, Enter selects
        → when item selected: show per-relay breakdown inline
        → each row: status dot, relay URL, reason, message
```

---

## Layer-by-Layer Changes

### Layer 1: New `ResolvedRelay` struct + `OutboxResolver` trait change

**File:** `crates/nmp-core/src/publish/traits.rs`

Add:
```rust
/// A relay URL paired with the human-readable reason it was selected.
/// The reason string is pre-formatted by the resolver; callers render it verbatim.
pub struct ResolvedRelay {
    pub url: RelayUrl,
    pub reason: String,
}
```

Change the trait method signature:
```rust
// BEFORE
fn resolve(&self, author_pubkey: &str, p_tags: &[String], target: &PublishTarget, kind: u32) -> BTreeSet<RelayUrl>;

// AFTER
fn resolve(&self, author_pubkey: &str, p_tags: &[String], target: &PublishTarget, kind: u32) -> Vec<ResolvedRelay>;
```

Using a named struct (not a `(RelayUrl, String)` tuple) allows adding future fields (e.g. `role: RelayRole`, `source_event_id: Option<String>`) without breaking callers.

**Callers affected:**
- `Nip65OutboxResolver` (nmp-router) — real implementation, needs full annotation work (Layer 2)
- `StaticOutbox` (nmp-core/publish/traits.rs test stub) — trivially maps each URL to `ResolvedRelay { url, reason: "static".into() }`
- `NoopOutboxResolver` (nmp-core/publish/traits.rs test stub) — returns `vec![]`

No other callers exist.

---

### Layer 2: `Nip65OutboxResolver` — annotate every relay selection

**File:** `crates/nmp-router/src/nip65_resolver.rs`

The `resolve()` method has exactly 5 code paths that add relay URLs to the output. Each one must attach a reason string at the point of selection — this is the only place in the codebase where the information is available.

**Code path 1 — Author kind:10002 write relays (lines 180–181)**

```rust
if let Some((writes, _reads)) = self.lookup_kind10002(author_pubkey) {
    for url in writes {
        out.push(ResolvedRelay { url, reason: "NIP-65 write relay".into() });
    }
}
```

This is the standard case: the author has published their relay list and we're writing to their declared write relays.

**Code path 2 — Local write relays fallback (lines 183–186)**

```rust
if out.is_empty() && self.is_active_account(author_pubkey) {
    if let Ok(guard) = self.local_write_relays.lock() {
        for url in guard.as_slice() {
            out.push(ResolvedRelay { url: url.clone(), reason: "App relay (local config)".into() });
        }
    }
}
```

This fires when the active account has not yet fetched or published their kind:10002. The relays come from the app's local configuration. This is exactly what the user described as "because it's an app relay" — the label distinguishes it from the NIP-65 case.

**Code path 3 — Discovery kind indexer relays (lines 195–199)**

```rust
if is_discovery_kind(kind) {
    for url in self.indexer_relays.lock()... {
        out.push(ResolvedRelay { url, reason: format!("Discovery indexer (kind {kind})") });
    }
}
```

Applies to kinds 0, 3, and 10000–19999 (replaceable events). Including the kind in the reason string makes it diagnostic ("Discovery indexer (kind 0)" vs "Discovery indexer (kind 10002)").

**Code path 4 — Recipient inbox relay from #p tags (lines 204–210)**

```rust
for p in p_tags {
    if let Some((_writes, reads)) = self.lookup_kind10002(p) {
        for url in reads {
            out.push(ResolvedRelay {
                url,
                reason: format!("Inbox relay for {}", short_npub(p)),
            });
        }
    }
}
```

This is the case that caused the pending reaction bug. The user sees "Inbox relay for npub1abc…" and immediately understands: "this relay is here because the author I tagged has it in their NIP-65 read list." The `short_npub()` helper already exists in nmp-core (`nmp_core::display::short_npub`).

**Code path 5 — Explicit target (lines 165–167)**

```rust
if let PublishTarget::Explicit { relays } = target {
    return relays.iter().map(|url| ResolvedRelay {
        url: url.clone(),
        reason: "Explicit relay".into(),
    }).collect();
}
```

Short-circuits like it does today.

**Deduplication note:** The current implementation collects into a `BTreeSet` which deduplicates. With `Vec<ResolvedRelay>`, a relay could appear multiple times with different reasons if it happens to be both the author's write relay and an indexer relay. The engine currently canonicalizes and deduplicates — that logic needs to be aware of the new type. Decision: keep deduplication in the engine, but when a URL appears multiple times with different reasons, join the reasons with "; " (e.g., "NIP-65 write relay; Discovery indexer (kind 0)"). This is an edge case but provides better information than silently dropping one of the reasons.

---

### Layer 3: Engine — store reasons in `InFlight`

**File:** `crates/nmp-core/src/publish/engine.rs`

**InFlight struct** (currently at line 78):
```rust
// BEFORE
pub(super) struct InFlight {
    pub event: SignedEvent,
    pub per_relay: BTreeMap<RelayUrl, PerRelayState>,
    pub pending_retries: BTreeMap<RelayUrl, u64>,
    pub dirty: bool,
    pub correlation_id_override: Option<String>,
}

// AFTER
pub(super) struct InFlight {
    pub event: SignedEvent,
    pub per_relay: BTreeMap<RelayUrl, PerRelayState>,
    pub relay_reasons: BTreeMap<RelayUrl, String>,   // write-once at publish time
    pub pending_retries: BTreeMap<RelayUrl, u64>,
    pub dirty: bool,
    pub correlation_id_override: Option<String>,
}
```

`relay_reasons` is a parallel map — it shares the same key space as `per_relay` but is never mutated by retry logic, relay connected/disconnected events, or dispatch cycles. It is written once in `start_publish_inner()` and read only during projection.

**In `start_publish_inner()`**, after calling `resolve()`:
```rust
let resolved = self.outbox.resolve(&event.unsigned.pubkey, &p_tags, &target, kind);

// Deduplicate: if a URL appears with multiple reasons, join them.
let mut relay_map: BTreeMap<RelayUrl, String> = BTreeMap::new();
for r in resolved {
    let canonical = helpers::canonical_relay_identity(&r.url);
    relay_map.entry(canonical)
        .and_modify(|existing| {
            if !existing.contains(&r.reason) {
                existing.push_str("; ");
                existing.push_str(&r.reason);
            }
        })
        .or_insert(r.reason);
}
if relay_map.is_empty() {
    return Err(PublishEngineError::NoTargets);
}
let per_relay = relay_map.keys().map(|url| (url.clone(), PerRelayState::Pending)).collect();
let relay_reasons = relay_map;
```

This replaces the current `BTreeSet` canonicalization path and keeps the semantics identical while threading through reasons.

---

### Layer 4: Kernel projection — `relay_reason` field on `PublishOutboxRelay`

**File:** `crates/nmp-core/src/kernel/types.rs`

```rust
// BEFORE
pub(super) struct PublishOutboxRelay {
    pub(super) relay_url: String,
    pub(super) status: String,
    pub(super) status_label: String,
    pub(super) attempt: u32,
    pub(super) attempt_label: String,
    pub(super) message: String,
}

// AFTER
pub(super) struct PublishOutboxRelay {
    pub(super) relay_url: String,
    pub(super) status: String,
    pub(super) status_label: String,
    pub(super) attempt: u32,
    pub(super) attempt_label: String,
    pub(super) message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) relay_reason: String,
}
```

`skip_serializing_if = "String::is_empty"` keeps the JSON payload unchanged for any event where the reason was not computed (backwards compat for old data in flight).

**File:** `crates/nmp-core/src/kernel/publish_outbox.rs`

Update `publish_outbox_relay()`:
```rust
fn publish_outbox_relay(relay_url: &str, state: &PerRelayState, reason: &str) -> PublishOutboxRelay {
    let (status, attempt, message) = match state { ... }; // unchanged
    PublishOutboxRelay {
        relay_url: relay_url.to_string(),
        status,
        status_label,
        attempt,
        attempt_label,
        message,
        relay_reason: reason.to_string(),
    }
}
```

Update the call site in `publish_outbox_items()`:
```rust
let relays = row.per_relay.iter().map(|(url, state)| {
    let reason = row.relay_reasons.get(url).map(String::as_str).unwrap_or("");
    publish_outbox_relay(url, state, reason)
}).collect::<Vec<_>>();
```

---

### Layer 5: chirp-tui — expose relay detail in snapshot

**File:** `apps/chirp/chirp-tui/src/feature_snapshot.rs`

Add:
```rust
pub struct OutboxRelayLine {
    pub relay_url: String,
    pub status_label: String,   // "Pending", "Sending", "Ok", "Retrying", "Failed"
    pub reason: String,         // pre-formatted from kernel: "NIP-65 write relay", etc.
    pub message: String,        // pre-formatted from kernel: "Relay accepted the event", etc.
}
```

Extend `OutboxLine`:
```rust
pub struct OutboxLine {
    pub handle: String,
    pub title: String,
    pub status_label: String,
    pub preview: String,
    pub can_retry: bool,
    pub relays: Vec<OutboxRelayLine>,   // NEW — was stripped; now passed through
}
```

Update `outbox_from()` to parse the `relays` array from each `publish_outbox` row:
```rust
fn relay_lines_from(row: &Value) -> Vec<OutboxRelayLine> {
    row.get("relays")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|r| OutboxRelayLine {
            relay_url: string_field(r, "relay_url"),
            status_label: string_field(r, "status_label"),
            reason: string_field(r, "relay_reason"),
            message: string_field(r, "message"),
        })
        .collect()
}
```

---

### Layer 6: chirp-tui — outbox item selection + detail pane

**File:** `apps/chirp/chirp-tui/src/app.rs`

Add to `AppState`:
```rust
pub outbox_selected: Option<usize>,  // index into state.features.outbox when Settings tab is active
```

**File:** `apps/chirp/chirp-tui/src/input.rs`

In the Settings tab arm of the key handler:
- `j` / `Down` → increment `outbox_selected` (if `Some`, clamp to list length)
- `k` / `Up` → decrement `outbox_selected`
- `Enter` → set `outbox_selected = Some(n)` where n is the currently highlighted row (or toggle off if already selected)
- `Esc` → `outbox_selected = None`

**File:** `apps/chirp/chirp-tui/src/ui/settings.rs`

`render_outbox()` currently renders a flat list of `OutboxLine` items (up to 10, one line each). When `state.outbox_selected` is `Some(i)`:

Split the pane vertically:
- Upper section: the item list, same as today but with a cursor indicator on the selected item
- Lower section: the per-relay detail for the selected item

Per-relay detail rows look like:
```
● relay.primal.net                     Ok
  Your NIP-65 write relay
  Relay accepted the event

◌ relay.nostr.band                     Pending
  Inbox relay for npub1abc…
  Waiting for relay connection
```

Status dot colors reuse the existing `status_dot()` helper. `reason` is rendered in `DIM_TEXT`. `message` is rendered in `DIMMER_TEXT`. This is purely additive to the existing rendering path — the flat list view when nothing is selected is unchanged.

---

### What changes iOS needs (trivial)

`PublishOutboxRelay` in the JSON now includes `relay_reason: String`. iOS (`NotificationsView+OutboxRow.swift`) already iterates `item.relays` and renders `OutboxRelayRow` for each one. `OutboxRelayRow` would add:

```swift
if !item.relayReason.isEmpty {
    Text(item.relayReason)
        .font(.caption2)
        .foregroundColor(.secondary)
}
```

The Swift `Codable` struct for `PublishOutboxRelay` adds:
```swift
let relayReason: String
```

That is the entire iOS change. No logic, no branching, no understanding of NIP-65.

---

## What Is NOT in This PR

- **Kind:10002 republication reason** — the user mentioned "because we are republishing someone else's 10002 event we didn't find in an indexer relay." This is a separate publish flow (not a note or reaction), and the resolver already handles it correctly in code path 1 (author write relays). The reason string "NIP-65 write relay" is accurate from the relay's perspective. If a separate UI label like "Republishing relay list for npub1…" is desired, that requires a new `PublishAction` variant or a flag on the existing one. Post-v1 candidate.

- **`is_complete()` semantic change** — whether a publish row should evict when ANY relay accepts (NIP-01: one accepting relay = published) vs. ALL relays settle is a product decision. The surface fix already prevents "Pending" from showing when relay.primal.net has accepted. The row staying in the outbox while secondary fan-out relays are still attempting is debatable but not wrong. This PR does not change eviction semantics.

- **Global outbox access from any tab** — "select any event from any tab" is a navigation feature. Currently the outbox is only accessible from Settings. Making it accessible from Home (e.g., press 'o' on a selected post to open its publish status) is a follow-up TUI feature that this PR makes possible by building the data layer.

---

## Execution Order

| Step | Files | Crate(s) |
|------|-------|----------|
| 1 | `ResolvedRelay` struct; `OutboxResolver::resolve()` return type; update `StaticOutbox` and `NoopOutboxResolver` | `nmp-core` |
| 2 | `Nip65OutboxResolver::resolve()` annotation | `nmp-router` |
| 3 | `InFlight.relay_reasons`; `start_publish_inner()` split | `nmp-core/publish/engine` |
| 4 | `PublishOutboxRelay.relay_reason`; `publish_outbox_relay()` signature; call site | `nmp-core/kernel` |
| 5 | `OutboxRelayLine`; `OutboxLine.relays`; `outbox_from()` parser | `chirp-tui` |
| 6 | `AppState.outbox_selected`; input handling; `render_outbox()` detail mode | `chirp-tui` |
| 7 | Swift `PublishOutboxRelay.relayReason`; `OutboxRelayRow` new line | `ios/Chirp` |

Steps 1–4 ship as one PR (`nmp-core` + `nmp-router`). Steps 5–6 ship as a second PR (`chirp-tui`). Step 7 ships as a third PR (`ios`). Steps 5–7 depend on step 4 being merged; within each group steps are sequential.

---

## Test Coverage Requirements

- `nmp-router` tests: add a test asserting that each code path in `Nip65OutboxResolver::resolve()` produces the expected `reason` string for a given input scenario (author with kind:10002, active account without, indexer kind, p-tag with kind:10002).
- `nmp-core` engine tests: assert that `InFlight.relay_reasons` is populated correctly after `start_publish_inner()` and that the reason survives a `mark_relay_unavailable()` + `mark_relay_available()` cycle unchanged.
- `nmp-core` projection tests: assert that `publish_outbox_items()` includes `relay_reason` in the projected relay rows.
- `chirp-tui` snapshot tests (insta): update snapshots to include the new `relays` field in OutboxLine.
- Doctrine lint: run `cargo test -p nmp-testing --test doctrine_lint_smoke` — no new tokens expected.
