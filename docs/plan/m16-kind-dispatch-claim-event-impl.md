# M16 — Kind-dispatch `claim_event` implementation plan

- **Status:** Director plan, ready for parallel dispatch
- **Scope:** kernel primitive + FFI + content-side bridge + TUI rewire + gallery verification
- **Branch:** `feat/kind-dispatch-content-rendering`
- **Out of scope (separate M16 slices):** iOS/Compose hosts, ContentTreeDto deletion on Android
- **Locks:** ADR-0034 (kind dispatch), ADR-0033 (RenderContext/EmbedKindProjection), AGENTS.md doctrine D0/D4/D6/D8

---

## §1 — Architecture summary

### Data flow

```
                              renderer (sees nostr:nevent1… / nostr:naddr1…)
                              │
                              ▼  on appear
              [NostrContentView] ── claim_handle = registry.claim(target)
                              │           (in-memory dedupe; cold = None)
                              │
                              ▼  if not yet resolved
              [EventClaimSink (trait in nmp-content)]
                              │
                              ▼  host impl
              nmp_app_claim_event(uri, consumer_id)          ◄── new FFI
                              │
                              ▼
              ActorCommand::ClaimEvent { uri, consumer_id } ◄── new variant
                              │
                              ▼
              Kernel::claim_event(uri, consumer_id, …)      ◄── new method
                  ├── parse uri → NostrUri::{Event | Address}
                  ├── event_claims[key] ∪= {consumer_id}    (refcount)
                  ├── if already in self.events  → no fetch
                  └── self.oneshot.request(registry, InterestScope::Global,
                          InterestShape { event_ids:{id} | addresses:{coord}, limit:Some(1), … })
                              │           (D4 — single registration path)
                              ▼
              planner → REQ frame → relay pool → ingest → events: HashMap<id, StoredEvent>
                              │
                              ▼  next emit tick (D8 — push, no polling)
              kernel.make_update():
                  projections["claimed_events"] = { primary_id → ClaimedEventDto { … } }
                              │
                              ▼  snapshot callback
              host walks JSON projections["claimed_events"], decodes per uri,
              calls resolve_embed_projection(event, ctx) → EmbedKindProjection,
              wraps in EmbeddedEventEnvelope, stores in NostrContentView.embedded_events
                              │
                              ▼
              NostrKindRegistry::resolve(projection).render(area, buf)
```

### Snapshot key — load-bearing decision

`claimed_events` is keyed by **`primary_id`**:

- `nevent1…` / `note1…` → 64-char lowercase hex event id (matches `StoredEvent.id`)
- `naddr1…` → coordinate string `kind:author_pubkey_hex:d_tag` (matches the renderer-side `WireUri.primary_id`)

The kernel walks `event_claims` keys; for hex64 keys it looks up `events[key]`; for coordinate keys it scans `events.values()` for the matching `(kind, author, d_tag)`. The renderer-side fallback already does `events.get(&uri.primary_id).or_else(|| events.get(&uri.uri))` (`nostr_content_view.rs:531-534`), so a URI-string entry would also resolve — but the kernel emits **primary_id only** to keep the projection deterministic.

### D0 — no `Embed*` symbols in nmp-core

The kernel primitive is **`claim_event` / `release_event` / `event_claims` / `event_claim_drops_total` / `ClaimedEventDto`** — none of those mention "embed". `EmbedTarget`, `EmbedClaimRegistry`, `EmbedKindProjection`, `EmbeddedEventEnvelope`, `EmbedClaimSink` all stay in `nmp-content`. Grep `crates/nmp-core/` for `Embed` after implementation: result must be empty.

### Content-side bridge — `EventClaimSink` trait in `nmp-content`

`nmp-content` introduces `pub trait EventClaimSink { fn claim(&self, uri: &str, consumer_id: &str); fn release(&self, uri: &str, consumer_id: &str); }`. `NostrContentView` (and any other renderer) takes `Option<&dyn EventClaimSink>` in a builder method. The TUI host (`apps/nmp-gallery/tui`) provides the impl that wraps an `*mut NmpApp` and calls `nmp_app_claim_event`. **The trait lives in `nmp-content` — `nmp-content` never gains an `nmp-ffi` dep**, preserving the existing D0 layering.

### iOS / Compose adapter sketches (NOT in this PR scope)

- **iOS.** A Swift `EmbedHost` actor reads codegen'd `KernelSnapshot.projections.claimed_events`, calls `nmp_app_claim_event(uri, consumer)` from `NostrContentView.onAppear`, `nmp_app_release_event(uri, consumer)` from `onDisappear`. The same `primary_id` map drives `NostrKindRegistry` dispatch on iOS. No additional FFI symbols — symmetric with `nmp_app_claim_profile`.
- **Compose.** Kotlin `EmbedHost` object reads `KernelUpdate.claimedEvents` (Kotlinx-Serializable), calls `NmpFfi.claimEvent` from a `DisposableEffect` keyed on `(uri)`. Same projection key, same widget registry on the dispatch side.

Both are deferred to follow-up PRs but consume the same wire contract this PR locks in.

---

## §2 — Workstream breakdown

Per-workstream file lists are **exhaustive and non-overlapping**. Any file listed below appears in exactly one workstream.

### W1 — kernel (owner: kernel)

**Files (exclusive):**

- `crates/nmp-core/src/kernel/mod.rs`
- `crates/nmp-core/src/kernel/update.rs`
- `crates/nmp-core/src/kernel/requests/mod.rs`
- `crates/nmp-core/src/kernel/requests/event.rs` *(NEW file)*
- `crates/nmp-core/src/kernel/types.rs`
- `crates/nmp-core/src/actor/mod.rs`
- `crates/nmp-core/src/actor/dispatch.rs`
- `crates/nmp-core/src/kernel/event_claim_tests.rs` *(NEW file)*

**Depends on:** none.

**Acceptance:** `cargo test -p nmp-core` green; new test `event_claim_tests::event_claim_resolves_via_snapshot` passes; `cargo test -p nmp-testing --test doctrine_lint_smoke` green; `rg -n "Embed" crates/nmp-core/src/` returns no matches outside pre-existing doc-comment cross-references.

**Implementation steps:**

1. `crates/nmp-core/src/kernel/types.rs` — add a new public DTO **at end of file**:
   ```rust
   #[derive(Clone, Debug, Serialize, PartialEq, Eq)]
   pub(crate) struct ClaimedEventDto {
       pub(super) primary_id: String,   // event-id hex OR "kind:pubkey:d"
       pub(super) id: String,           // canonical 64-hex event id
       pub(super) author_pubkey: String,
       pub(super) kind: u32,
       pub(super) created_at: u64,
       pub(super) tags: Vec<Vec<String>>,
       pub(super) content: String,
   }
   ```
   Add helper `pub(super) fn from_stored(primary_id: String, e: &StoredEvent) -> Self`.

2. `crates/nmp-core/src/kernel/mod.rs`:
   - Add `pub(crate) const MAX_EVENT_CLAIMS_PER_KEY: usize = 256;` next to `MAX_CLAIMS_PER_PUBKEY` (line 364).
   - Add struct fields after `profile_claims` (line 545):
     ```rust
     event_claims: HashMap<String, BTreeSet<String>>,    // primary_id → consumer_ids
     event_claim_requested: BTreeSet<String>,            // primary_ids already submitted to OneshotApi
     event_claim_drops_total: u64,
     ```
   - Initialize them in `Kernel::new` (next to `profile_claims: HashMap::new()` at line 1247).
   - In the `Reset` preserve block (existing `take_*_handle_for_reset` pattern), event_claims need not survive reset (matches profile_claims).
   - Add a test-only accessor `pub(crate) fn event_claims_len_for_test(&self, key: &str) -> usize` mirroring `profile_claims_len_for_test` (line 1467).

3. `crates/nmp-core/src/kernel/requests/mod.rs` — add `pub mod event;` alongside `pub mod profile;`.

4. `crates/nmp-core/src/kernel/requests/event.rs` — NEW file, mirrors `profile.rs` shape. Public methods on `impl Kernel`:

   ```rust
   pub(crate) fn claim_event(&mut self, uri: String, consumer_id: String, can_send: bool) -> Vec<OutboundMessage>
   pub(crate) fn release_event(&mut self, uri: &str, consumer_id: &str) -> Vec<OutboundMessage>
   ```

   Body of `claim_event`:
   - Parse `parse_nostr_uri(&uri)`; on error → log + return `Vec::new()` (D6).
   - Reject `NostrUri::Profile` (that's `claim_profile`'s job) — log + return.
   - Compute `primary_id`:
     - `NostrUri::Event { event_id, .. }` → `event_id.clone()`
     - `NostrUri::Address { kind, pubkey, identifier, .. }` → `format!("{kind}:{pubkey}:{identifier}")`
   - Refcount + bound check identical to `claim_profile` lines 154–165, using `MAX_EVENT_CLAIMS_PER_KEY` and incrementing `self.event_claim_drops_total`.
   - Log + `self.changed_since_emit = true`.
   - Early return when:
     - `primary_id` is hex64 and `self.events.contains_key(&primary_id)`; OR
     - `primary_id` is a coordinate and `self.events.values().any(|e| e.kind == kind && e.author == pubkey && e.has_d_tag(identifier))`; OR
     - `self.event_claim_requested.contains(&primary_id)`.
   - Otherwise, build `InterestShape`:
     - Event case: `InterestShape { event_ids: BTreeSet::from([id.clone()]), limit: Some(1), ..Default::default() }`
     - Address case: `let coord = NaddrCoord { kind, pubkey, d_tag: identifier }; InterestShape { addresses: BTreeSet::from([coord]), limit: Some(1), ..Default::default() }`
   - If `!can_send`, do not register; log "event claim queued until relay connects" and return. (Hook to drain on connect is a follow-up — out of scope for this PR; the planner trigger is still enqueued.)
   - Otherwise:
     ```rust
     let (token, interest_id) = {
         let registry = self.lifecycle.registry_mut();
         self.oneshot.request(registry, InterestScope::Global, shape)
     };
     self.pending_discovery_oneshots.insert(interest_id, token);
     self.event_claim_requested.insert(primary_id);
     self.lifecycle.enqueue_trigger(
         CompileTrigger::ViewOpened { interest_ids: Vec::new() }
     );
     ```
   - Return `Vec::new()` (planner emits the wire frame; D4 — no `req()` dual-write).

   Body of `release_event`: mirror `release_profile` (lines 192–216) — remove the consumer; on empty set, also remove from `event_claim_requested` (so a re-claim can re-fetch). Do NOT release the OneshotApi token here — `complete_unknown_oneshot` already releases on EOSE.

   Add a helper extension on `StoredEvent` (private to module) or inline: `fn has_d_tag(e: &StoredEvent, d: &str) -> bool { e.tags.iter().any(|t| t.len() >= 2 && t[0] == "d" && t[1] == d) }`.

5. `crates/nmp-core/src/kernel/update.rs` — add `claimed_events` projection. **Insert after `mention_profiles` block (after line 480)**:

   ```rust
   // claimed_events projection — keyed by primary_id (event-id hex OR
   // "kind:pubkey:d"). D8: built from current event_claims set lookup
   // against `events`; missing entries are simply absent (host renders
   // the URI as-is until the event arrives).
   let mut claimed_events: BTreeMap<String, ClaimedEventDto> = BTreeMap::new();
   for key in self.event_claims.keys() {
       if let Some(stored) = self.lookup_for_primary_id(key) {
           claimed_events.insert(
               key.clone(),
               ClaimedEventDto::from_stored(key.clone(), stored),
           );
       }
   }
   projections.insert(
       "claimed_events".to_string(),
       serde_json::to_value(&claimed_events)
           .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
   );
   ```

   Add `fn lookup_for_primary_id(&self, key: &str) -> Option<&StoredEvent>` next to `visible_items` (~line 484): hex64 → `self.events.get(key)`; otherwise split `kind:author:d_tag`, scan `self.events.values()` for the first match.

6. `crates/nmp-core/src/actor/mod.rs` — add ActorCommand variants after `ReleaseProfile` (line 532):

   ```rust
   ClaimEvent {
       uri: String,
       consumer_id: String,
   },
   ReleaseEvent {
       uri: String,
       consumer_id: String,
   },
   ```

7. `crates/nmp-core/src/actor/dispatch.rs` — add two arms after `ReleaseProfile` (line 484):

   ```rust
   ActorCommand::ClaimEvent { uri, consumer_id } => {
       let outbound = ctx.kernel.claim_event(uri, consumer_id, ctx.relays_ready);
       maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
       Some(outbound)
   }
   ActorCommand::ReleaseEvent { uri, consumer_id } => {
       let outbound = ctx.kernel.release_event(&uri, &consumer_id);
       maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
       Some(outbound)
   }
   ```

8. `crates/nmp-core/src/kernel/event_claim_tests.rs` — NEW file. Use `#[cfg(test)] mod event_claim_tests;` line in `kernel/mod.rs` next to `profile_claim_tests`. Tests (use `inject_event` from `test_support`):
   - `claim_event_for_known_event_id_resolves_without_relay`
   - `claim_event_emits_oneshot_request_via_lifecycle_registry` (assert `discovery_in_flight() == 1`)
   - `claim_event_naddr_matches_kind_pubkey_dtag_in_store`
   - `release_event_drops_consumer_and_removes_key_on_empty_set`
   - `claim_event_bounded_at_max_event_claims_per_key` (mirror retention_tests pattern)
   - `claimed_events_projection_emits_dto_keyed_by_primary_id`

---

### W2 — ffi (owner: ffi)

**Files (exclusive):**

- `crates/nmp-ffi/src/timeline.rs`

**Depends on:** W1 must merge first (imports `ActorCommand::ClaimEvent` / `ReleaseEvent`). For parallel-wave-1 execution, W1's spec freezes the variant shape so W2 can be written from this plan alone; the compile-link only succeeds after both land.

**Acceptance:** `cargo test -p nmp-ffi` green; new symbols visible via `nm` on the static lib.

**Implementation steps:**

1. After `nmp_app_release_profile` (line 123 of `timeline.rs`), add:

   ```rust
   /// Claim an embedded event by `nostr:` URI (T180 / ADR-0034). Refcounted
   /// per `consumer_id`; the kernel fetches the event over the OneshotApi
   /// when not yet in the store, and surfaces it in `claimed_events`
   /// keyed by `primary_id` (event-id hex for nevent/note; "kind:pubkey:d"
   /// for naddr). FFI-clean (D6): a null/invalid argument is a silent no-op.
   #[no_mangle]
   pub extern "C" fn nmp_app_claim_event(
       app: *mut NmpApp,
       uri: *const c_char,
       consumer_id: *const c_char,
   ) {
       let Some(app) = app_ref(app) else { return };
       let Some(uri) = c_string_argument(uri) else { return };
       let Some(consumer_id) = c_string_argument(consumer_id) else { return };
       app.send_cmd(ActorCommand::ClaimEvent { uri, consumer_id });
   }

   #[no_mangle]
   pub extern "C" fn nmp_app_release_event(
       app: *mut NmpApp,
       uri: *const c_char,
       consumer_id: *const c_char,
   ) {
       let Some(app) = app_ref(app) else { return };
       let Some(uri) = c_string_argument(uri) else { return };
       let Some(consumer_id) = c_string_argument(consumer_id) else { return };
       app.send_cmd(ActorCommand::ReleaseEvent { uri, consumer_id });
   }
   ```

   No `is_hex_pubkey` / `is_hex_id` check — the uri is parsed inside the kernel reducer and rejected on parse error (D6 silent).

2. No header generation step — the workspace already runs cbindgen against `nmp-ffi`. Builds will regenerate.

---

### W3 — embed-registry-bridge (owner: embed-registry-bridge)

**Files (exclusive):**

- `crates/nmp-content/src/embed_registry/mod.rs`
- `crates/nmp-content/src/embed_registry/event_claim_sink.rs` *(NEW file)*
- `crates/nmp-content/src/lib.rs` (only the `pub use` re-export line for `EventClaimSink`)

**Depends on:** none (no nmp-core API changes consumed here).

**Acceptance:** `cargo test -p nmp-content` green; new doctest in `event_claim_sink.rs` showing a manual `EventClaimSink` impl compiles.

**Implementation steps:**

1. `crates/nmp-content/src/embed_registry/event_claim_sink.rs` — NEW. Define:

   ```rust
   /// Host-side bridge that lets a renderer initiate an upstream fetch for
   /// an embedded event (ADR-0034). The trait lives in nmp-content so
   /// nmp-content never gains an nmp-ffi dependency; each platform host
   /// supplies the impl that bridges to its FFI surface.
   pub trait EventClaimSink: Send + Sync {
       fn claim(&self, uri: &str, consumer_id: &str);
       fn release(&self, uri: &str, consumer_id: &str);
   }

   /// No-op sink — fixture/test surfaces use this so renderers can run
   /// without an active kernel.
   pub struct NoopEventClaimSink;
   impl EventClaimSink for NoopEventClaimSink {
       fn claim(&self, _uri: &str, _consumer_id: &str) {}
       fn release(&self, _uri: &str, _consumer_id: &str) {}
   }
   ```

2. `crates/nmp-content/src/embed_registry/mod.rs` — add `pub mod event_claim_sink;` + `pub use event_claim_sink::{EventClaimSink, NoopEventClaimSink};`.

3. `crates/nmp-content/src/lib.rs` — re-export `pub use embed_registry::{EventClaimSink, NoopEventClaimSink};`.

---

### W4 — tui-host (owner: tui-host)

**Files (exclusive):**

- `crates/nmp-cli/registry/tui/content-view/nostr_content_view.rs`

**Depends on:** W3 (consumes `EventClaimSink`). Wave 2.

**Acceptance:** `cargo test -p nmp-gallery-tui` green; new test in `apps/nmp-gallery/tui/src/render.rs::tests` (no — that's W7's; here just compile + existing tests).

**Implementation steps:**

1. Add an optional `claim_sink: Option<&'a dyn nmp_content::EventClaimSink>` field to `NostrContentView<'a>` (line 23) and a builder method `pub fn claim_sink(mut self, sink: Option<&'a dyn EventClaimSink>) -> Self`.
2. Add a `consumer_id: Option<&'a str>` field + builder; default `None` skips the claim entirely (back-compat with fixture callers).
3. In `render_embedded_event` (line 507), before the `envelope_for` lookup, when `self.claim_sink` and `self.consumer_id` are both `Some`, call `sink.claim(&uri.uri, consumer_id)` once per render pass per uri. Track per-render seen-set on the widget instance to avoid double-claims inside one render. **Release is the host's responsibility** — the renderer is render-only and stateless; the host loop releases on view teardown via its own bookkeeping (this PR's TUI host: release-on-quit is sufficient).
4. Do NOT change `envelope_for` — the existing `events.get(&uri.primary_id).or_else(|| events.get(&uri.uri))` fallback is correct.

---

### W5 — content-view-rewire (owner: content-view-rewire)

**Files (exclusive):**

- `apps/nmp-gallery/tui/src/render.rs` (only the `render_embed_showcase` function — verify no other touches)
- `apps/nmp-gallery/tui/src/data.rs` (only the `ContentExample` struct expansion if needed for the live path)

**Depends on:** W4. Wave 2 — runs in parallel with W4 ONLY if the renderer signature is locked first in this plan; otherwise wave 3. Defaulted to **wave 3** to be safe.

**Acceptance:** `cargo test -p nmp-gallery-tui` green; gallery `--dump-lines embed-article` shows the article title produced by `DefaultArticleRenderer`.

**Implementation steps:**

1. `render.rs::render_embed_showcase` — pass `.claim_sink(data.live_sink.as_deref())` and `.consumer_id(Some("nmp-gallery-tui.embed"))` to the `NostrContentView` builder when `data.live_sink` is `Some`. For the dump/static path (`data.live_sink == None`), do nothing — the fixture envelopes resolve via `embedded_events` without a fetch.
2. `data.rs::GalleryData` — add `pub live_sink: Option<Arc<dyn EventClaimSink>>` field. `load(load_images)` sets it `None` (fixture mode). The `live` constructor (W6) sets it `Some`. Existing tests (`render_test_data`) leave it `None`.

---

### W6 — gallery-live (owner: gallery-live)

**Files (exclusive):**

- `apps/nmp-gallery/tui/src/live.rs` *(restored from `33d1e244~1` and extended)*
- `apps/nmp-gallery/tui/src/lib.rs` (only the `pub mod live;` line restoration)
- `apps/nmp-gallery/tui/Cargo.toml` (only re-add the nmp-ffi/nmp-app-gallery deps required by the restored file)

**Depends on:** W1+W2 (uses `nmp_app_claim_event`). Wave 3.

**Acceptance:** `cargo build -p nmp-gallery-tui` green; a `--live --component embed-article --hold-ms 30000` invocation against real relays renders the kind:30023 article body (verified manually in §4 gallery section).

**Implementation steps:**

1. `git show 33d1e244~1:apps/nmp-gallery/tui/src/live.rs > apps/nmp-gallery/tui/src/live.rs` (restoration).
2. Add `pub mod live;` to `apps/nmp-gallery/tui/src/lib.rs` next to `pub mod data;`.
3. `apps/nmp-gallery/tui/Cargo.toml` — restore the deps the restored file requires (`nmp-ffi`, `nmp-app-gallery`, `nmp-core`). Verify with `cargo check -p nmp-gallery-tui` after restoration.
4. Extend `LiveKernel` with:
   ```rust
   fn claim_event(&self, uri: &str) -> Result<(), String> {
       let uri_c = CString::new(uri).map_err(|_| "uri nul".to_string())?;
       let consumer = CString::new(CONSUMER_ID).map_err(|_| "consumer nul".to_string())?;
       nmp_ffi::nmp_app_claim_event(self.app, uri_c.as_ptr(), consumer.as_ptr());
       Ok(())
   }
   ```
5. Implement `EventClaimSink` for an `Arc<LiveKernel>` wrapper — `nmp-content` is depended on by the gallery via the registry path-includes, so the trait is visible. Sink struct:
   ```rust
   pub struct LiveKernelSink { pub app: *mut nmp_ffi::NmpApp }
   unsafe impl Send for LiveKernelSink {}
   unsafe impl Sync for LiveKernelSink {}
   impl EventClaimSink for LiveKernelSink {
       fn claim(&self, uri: &str, consumer_id: &str) {
           let Ok(uri_c) = CString::new(uri) else { return };
           let Ok(cid) = CString::new(consumer_id) else { return };
           unsafe { nmp_ffi::nmp_app_claim_event(self.app, uri_c.as_ptr(), cid.as_ptr()); }
       }
       fn release(&self, uri: &str, consumer_id: &str) {
           let Ok(uri_c) = CString::new(uri) else { return };
           let Ok(cid) = CString::new(consumer_id) else { return };
           unsafe { nmp_ffi::nmp_app_release_event(self.app, uri_c.as_ptr(), cid.as_ptr()); }
       }
   }
   ```
6. In `LiveGallerySource::load`, after the existing `wait_for_event(QUOTE_SOURCE_EVENT_ID, …)`, parse the article-naddr URI from `data.rs::ARTICLE_NADDR`, call `kernel.claim_event(ARTICLE_NADDR)`, and wait on the snapshot for `projections.claimed_events[primary_id_for(ARTICLE_NADDR)]` to be present. Use a new `wait_for_claimed_event(primary_id, timeout)` helper that walks `parse_snapshot(payload) → projections.claimed_events.<primary_id>` and returns the parsed `ClaimedEventDto` JSON shape (id, author_pubkey, kind, created_at, tags, content).
7. Expose the parsed claimed event through `LiveFacts.embedded_article: Option<LiveEmbeddedEvent>` (new struct in this file).

---

### W7 — gallery-embed (owner: gallery-embed)

**Files (exclusive):**

- `apps/nmp-gallery/tui/src/data.rs` (only the `live` constructor and the `embed_article` live-data branch)
- `apps/nmp-gallery/tui/src/main.rs` (only the `--live` flag plumbing)

**Depends on:** W5 + W6. Wave 3 (sequenced after W6 within the wave by file split — main.rs is only touched here, data.rs `live` is only touched here; data.rs `load` is the only overlap region with W5 and is separated by function).

**Acceptance:** `cargo test -p nmp-gallery-tui` green; `cargo run -p nmp-gallery-tui -- --live --component embed-article --hold-ms 30000` renders the live article.

**Implementation steps:**

1. `main.rs` — add `--live` flag parsing. When set, call `GalleryData::live(Duration::from_secs(30))` instead of `GalleryData::load(load_images)`.
2. `data.rs` — add `pub fn live(timeout: Duration) -> Result<Self, String>`:
   - Calls `LiveGallerySource::new(timeout).load()`.
   - Builds the `embed_article` `ContentExample` by:
     - Tokenising the live `quote_source_item.content` (which contains the naddr).
     - Constructing the `EmbeddedEventEnvelope` from `LiveFacts.embedded_article` using `nmp_content::embed_projection::resolve_embed_projection(&kernel_event, &render_ctx)`.
     - Inserting the envelope under `primary_id` (and under the URI string for the renderer fallback).
   - Keeps fixture content for the other embed components (`embed_note`, `embed_highlight`, `embed_profile`) — only `embed_article` becomes a real-fetch demo in this PR.
   - Sets `live_sink: Some(Arc::new(LiveKernelSink { app: source.app() }))` so subsequent renders of other embed components in live mode also trigger claims.

---

## §3 — Parallel execution waves

```
Wave 1 (file-disjoint, simultaneous):
   W1 kernel         (crates/nmp-core/**, plus actor/mod.rs + actor/dispatch.rs)
   W2 ffi            (crates/nmp-ffi/src/timeline.rs)
   W3 embed-bridge   (crates/nmp-content/src/embed_registry/**, lib.rs re-export)
   ─ merge order: W1 first, then W2 (link-time only); W3 independent.

Wave 2 (after Wave 1 merges):
   W4 tui-host       (crates/nmp-cli/registry/tui/content-view/nostr_content_view.rs)

Wave 3 (after Wave 2 merges; W5/W6/W7 file-disjoint, simultaneous):
   W5 content-rewire (apps/nmp-gallery/tui/src/render.rs + data.rs::load arm)
   W6 gallery-live   (apps/nmp-gallery/tui/src/live.rs + lib.rs + Cargo.toml)
   W7 gallery-embed  (apps/nmp-gallery/tui/src/main.rs + data.rs::live arm)
```

**Within a wave** every agent operates on file-disjoint paths. The only shared file across W5/W7 is `data.rs`, split by function (`load` vs `live`); the contract is that W5 only edits inside `pub fn load` and `pub struct GalleryData`'s field set (adding `live_sink`), while W7 only adds `pub fn live` and the live constructor's helpers. The director enforces this by reviewing each PR's diff before merging the wave.

**Test scope per worktree** — agents run **only** `cargo test -p <crate>` on the crates they touched, plus `cargo test -p nmp-testing --test doctrine_lint_smoke`. Workspace-wide `cargo test` is reserved for the supervisor at wave-merge time.

---

## §4 — Test plan

### Per-workstream (scoped)

| Workstream | Command | Notes |
|---|---|---|
| W1 | `cargo test -p nmp-core` | includes new `event_claim_tests` module |
| W2 | `cargo test -p nmp-ffi` | link-only assertion that new symbols exist |
| W3 | `cargo test -p nmp-content` | trait + Noop sink doctest |
| W4 | `cargo test -p nmp-gallery-tui` | back-compat: fixture path still passes |
| W5 | `cargo test -p nmp-gallery-tui` | render dump check for embed-article |
| W6 | `cargo build -p nmp-gallery-tui` | live.rs compiles |
| W7 | `cargo test -p nmp-gallery-tui` + manual `--live --component embed-article` | live fetch verified |
| All | `cargo test -p nmp-testing --test doctrine_lint_smoke` | D0 enforcement |

### Integration test (W1)

`crates/nmp-core/src/kernel/event_claim_tests.rs::claimed_events_projection_emits_dto_keyed_by_primary_id`:

```text
Given a fresh Kernel,
  inject a kind:30023 event with d-tag "kind-dispatch" + author A,
  call kernel.claim_event(ARTICLE_NADDR_URI, "test-consumer", true),
when kernel.make_update_value_for_test(true) is taken,
then projections["claimed_events"]["30023:<A_pubkey>:kind-dispatch"] is a JSON object
   with id == injected event id and kind == 30023.
```

A second variant uses an `nevent1` URI for an injected kind:1 event and asserts the projection key is the 64-hex event id.

A third variant calls `claim_event` BEFORE injecting the event, asserts the snapshot's `claimed_events` is `{}`, then injects, takes another snapshot, and asserts the entry appears (snapshot-push semantics, D8).

### Gallery verification (manual)

1. Build: `cargo build -p nmp-gallery-tui`.
2. Run in fixture mode: `cargo run -p nmp-gallery-tui -- --component embed-article` → renders the static article fixture (ADR-0034 title).
3. Run in live mode: `cargo run -p nmp-gallery-tui -- --live --component embed-article --hold-ms 30000` → after ≤30s the embed card replaces the fixture with the live-fetched article title + summary returned from the relays.
4. Verify no panic in `--live` mode against a cold cache, and that the kernel never re-fetches once the article is in the store (instrument with `discovery_in_flight()` or by inspecting the snapshot `metrics`).

---

## §5 — Risks & mitigations

| Risk | Detection | Mitigation |
|---|---|---|
| **D0 breach** — an `Embed*` symbol slips into nmp-core | `rg -n "Embed" crates/nmp-core/src/` returns hits | Doctrine smoke (`nmp-testing`) plus director grep gate before W1 merges. Kernel names are `claim_event` / `event_claims` / `ClaimedEventDto`. |
| **D4 breach** — `claim_event` dual-writes via `req()` instead of OneshotApi | code review of `requests/event.rs` | Plan spells out the `self.oneshot.request(registry, …)` call; no `self.req(...)` calls anywhere in the new module. Asserted by inspecting outbound `OutboundMessage` count from `claim_event` (always `Vec::new()`). |
| **D6 breach** — FFI surface panics or returns an error | `cargo test -p nmp-ffi` exercises null paths | `nmp_app_claim_event` / `_release_event` mirror `_claim_profile` line-for-line: `app_ref` guard, `c_string_argument` guard, `send_cmd` (infallible). |
| **D8 breach** — UI polls the snapshot in a sleep loop | code review of `live.rs` | The restored `live.rs` already uses blocking `Receiver::recv_timeout` against the update callback; the W6 acceptance flags any `sleep`/loop addition. The renderer never polls — claims happen on render, results appear on the next snapshot tick. |
| **File-collision in a wave** | director diff review at merge time | Per-workstream file lists are exhaustive; W5/W7 share `data.rs` and are split by function. If a Sonnet agent tries to widen its file list, its PR is rejected and re-dispatched. |
| **InterestShape lacks naddr filtering** | verified during plan authoring | `InterestShape.addresses: BTreeSet<NaddrCoord>` (`crates/nmp-planner/src/interest.rs:141`) already supports the coordinate path. No planner change needed. |
| **Bridge crate cycle** | `cargo check -p nmp-content` after W3 | The trait stays a pure marker in `nmp-content`; no FFI dep. The TUI host (`nmp-gallery-tui`) brings both `nmp-content` (via path-include) and `nmp-ffi` together — that crate is the platform-host layer, where the cycle is legitimately broken. |
| **StoredEvent visibility** | compile error on `update.rs` projection serialisation | `ClaimedEventDto` is `pub(crate)` with `Serialize` derive (mirrors `TimelineItem`'s pattern at `types.rs:106`); the projection serialises through `serde_json::to_value(&claimed_events)` exactly like `mention_profiles`. |

---

## §6 — Open questions for the orchestrator

1. **Release-on-snapshot-presence?** Today's plan releases the OneshotApi token via `complete_unknown_oneshot` (EOSE-driven, existing path). The kernel never explicitly releases on `release_event`; the consumer-set decay only removes the `event_claims` row. This is symmetric with `release_profile`, but for embed cards a long-lived claim across many views means the OneshotApi rows are short-lived (resolve + release at EOSE) while the kernel `event_claims` set is long-lived. **Confirm this is intended** — otherwise we need an explicit `oneshot.release(...)` call on the last `release_event`.

2. **Should `claim_event` accept a bare event-id hex / pubkey, not just a `nostr:` URI?** Mirroring `claim_profile`'s pubkey-hex argument would be more uniform. The current plan accepts only `nostr:` URIs (so iOS/Compose hosts hand the same string they already have from `WireUri.uri`). If hex is preferred, the FFI signature changes to `(*const c_char id_or_uri, *const c_char consumer_id)` and the parser falls back to treating a 64-hex string as an event id. Default: URI-only (less ambiguous).

3. **Claim-queued-until-connect?** `claim_profile` parks pending pubkeys in `profile_requests.pending` until the indexer connects, then `pending_profile_claim_requests` drains them. This plan does NOT add an equivalent for `claim_event`; cold-start callers see no fetch until the first relay is connected and `claim_event` is re-called (which the snapshot push will naturally drive once the kernel re-enters the warm path). If full cold-start parity is required, W1 must add a `pending_event_claims: BTreeSet<String>` field + a `pending_event_claim_requests` drain hooked into the post-connect path. Default: defer to a follow-up — most callers register claims after `relays_ready`.

4. **Snapshot DTO naming.** `ClaimedEventDto` is intentionally generic (no "Embed" prefix) to keep D0 honest. iOS/Compose codegen will surface it as `ClaimedEvent` (the DTO suffix is dropped at the FFI boundary, matching existing convention). Confirm naming with the codegen team — the alternative `EventResolution` is also reasonable and avoids the verbed-noun overlap with `claim_event`.
