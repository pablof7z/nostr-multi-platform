# nmp-gallery cross-platform — WIP state (live)

Working notes for the in-flight verification + fixes. Companion to `nmp-gallery-verification-matrix.md` (the DONE gate).

## Component self-claiming (author byline owns kind:0 claim) — per platform
- iOS: ✅ MERGED #833 (`NostrProfileName(pubkey:)` self-claims; embed renderers compose it).
- Desktop: ✅ MERGED #837 (`claim_and_resolve_author`; also removed central pre-warming).
- TUI: ❌ PR #838 OPEN, compile-broken in the `#[cfg(test)]` module ONLY (production renderer code is fine). The test module had phantom `KindAuthorHost`/`nmp_content::npub`, and `..Default::default()` on `ShortNoteProjection`/`EmbeddedEventEnvelope` which derive NO Default. PLUS a 2nd failure: `committed_registry_json_matches_generated_output` (web/registry/public/registry.json stale — regenerate via `cargo run -p nmp-cli --bin nmp -- export jsrepo --registry crates/nmp-cli/registry --output web/registry/public`). REAL TYPES (verified): trait `NostrMentionProfileHost` at `crate::nostr_mention_chip` with `profile_for_pubkey(&str)->Option<ContentProfileRenderData>` + `claim_profile(&str,&str)`; `ContentProfileRenderData` (fields pubkey/display_name/npub/picture_url) at `crates/nmp-cli/registry/tui/content-core/content_render_data.rs`; `ShortNoteProjection{id,author_pubkey,author_display_name,author_picture_url,created_at,content_tree,media_urls}`; `EmbeddedEventEnvelope{uri,primary_id,render_context:RenderContextWire,projection,collapsed,collapse_reason}`. Fix agent: **abafd7f4e633ca288** (must verify cargo test -p nmp-cli AND -p nmp-gallery-tui green BEFORE pushing). My own quick push e456ff1e was incomplete (still fails) — abafd7f4 supersedes it.
- Android: ❌ PR #839 OPEN, cargo test "fail" — must confirm whether real or flaky (PR touches only 3 Kotlin files, zero Rust → likely flaky relay_worker, but VERIFY the failing test name before retrying).

## OPEN BUGS found by user (must fix — these are the real gating issues)
1. **Event embeds stuck "loading embedded event" / "Fetching … from relay…" on iOS AND Android.** Only the kind:0 profile mention resolves. NOT #828 (clock→coverage only). Existing event_claim_tests pass on master because they inject via `ingest_pre_verified_event` (bypasses the wire) — THAT is the test gap. Asymmetry: profiles route the indexer lane (works), events route the claim-expansion/oneshot REQ compiler + store retrieval (broken).
   **CONFIRMED PRIME SUSPECT: #836 = commit 8211c189 "V-52 single-relay browsing with cache ORIGIN TRACKING".** Touches the exact suspect files: nmp-store mem/insert.rs (+59), events.rs (+17), store_impl.rs (+5), types/ids.rs (+12), mem/mod.rs (+67), nmp-router/router.rs, new browse/mod.rs. "Origin tracking" = events now keyed/filtered by origin relay → a claimed event fetched via the oneshot lane may be stored/retrieved with a mismatched origin so `lookup_for_primary_id`→get_by_id/get_param_replaceable never finds it. (I earlier mis-told an agent "#836 is unrelated NWC" — that was WRONG; corrected.)
   Empirical-bisect agent **a21063fa2b1599445** (opus, worktree): writes a wire-level failing test, checks out pre-#836 store files to confirm, fixes surgically (keep V-52 feature, fix claimed-event retrieval), keeps the regression test. THIS GATES EVERY EMBED CELL.
2. **Android embed pages do NOT render the surrounding note text.** iOS composes a content tree `text + eventRef + text` so it shows e.g. `"this is a great point " [card] " what do you think?"`. Android `EmbedComponentPages.kt` renders a bare `EventDisplayCard` in a Column with only a meta-label + footnote — NO inline surrounding prose. Must rebuild Android embed pages to compose the surrounding text inline around the card, mirroring iOS. Surrounding strings to use (from iOS):
   - article: `"hey, check out my article "` … `" I hope you enjoy it!"`
   - profile: `"met "` … `" at a nostr conference last week, brilliant mind"`
   - note: `"this is a great point "` … `" what do you think?"`
   - highlight: `"found this interesting "` …
   (TUI/Desktop must be checked for the same gap.)

## Screenshot methodology (for the final pass — learned the hard way)
- Do NOT force-stop the app per component. Cold restart starts a fresh kernel that must re-fetch kind:0; 12s is too tight → names show npub fallback even when correct. Use ONE warm session, navigate via back/tap without restarting, let profiles resolve before capturing.
- Android nav: `am start` resumes the last destination (back-stack restored on cold start). Press BACK to root ("NMP Component Gallery") before tapping a section.
- Android `embed-profile` ProfileEmbedPage hardcodes event=null → its "resolved" state IS the `Profile: npub1l2vyh…utajft` placeholder by design (but per bug #2 it also lacks surrounding text).

## Sequencing
1. Fix bug #1 (event-claim resolution regression) — gates all embeds. HIGHEST.
2. Fix TUI #838 compile break.
3. Confirm/merge Android #839.
4. Fix bug #2 (Android — and check TUI/Desktop — embed surrounding text inline).
5. Kernel removal of request_profile_for_rendered_note + claimed_events author enrichment (LAST, after all platforms self-claim).
6. Final warm-session screenshot pass across all 4 platforms → verification PDF.
