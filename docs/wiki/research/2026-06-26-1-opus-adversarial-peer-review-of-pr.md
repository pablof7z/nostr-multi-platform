---
type: research-record
date: 2026-06-26
session: 55264cfe-6420-4b06-a655-e0a935729211
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/55264cfe-6420-4b06-a655-e0a935729211.jsonl
source_lines: 2772-2811
agent_attribution: a59ce3889bd826717
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Opus adversarial peer review of PR #2083: privacy-gate soundness verified via code flow tracing (kind classifier, tag-before-sign order, leak-prevention), D0 boundaries audited, non-breaking changes confirmed, empirical test evidence (8/9 unit tests pass), verdict SHIP."
captured_at: 2026-06-26T11:55:13Z
---

Opus adversarial peer review of PR #2083: privacy-gate soundness verified via code flow tracing (kind classifier, tag-before-sign order, leak-prevention), D0 boundaries audited, non-breaking changes confirmed, empirical test evidence (8/9 unit tests pass), verdict SHIP.

---

User: [Agent task result: Agent "Opus review of client-identity PR" finished]
The baseline correctly lowers publish.rs (740→678), actor/mod.rs (764→761), tests.rs (864→812), and removes lib.rs (600 → now under threshold). The new files (publish_failures.rs 79, outbound_tags.rs 123, jitter_tests.rs 65, testing.rs 187, nip89/lib.rs 190) are all under the size gate. Good — this matches the "split, never raise baseline" rule.

I have everything I need. Let me compile the final review.

## Adversarial Review: `feat/client-identity-ua-nip89`

I reviewed the full two-commit diff, traced both data flows end-to-end, ran the privacy-gate unit tests (all pass), the nip89 tests (9 pass), and compiled the touched crates.

### Verified correct (no findings)

**Flow B privacy gate (highest priority) — SOUND.** `finalize_outbound_tags` (`crates/nmp-core/src/publish/outbound_tags.rs:25`) appends only when `classify_publish_behavior(kind) == PublishBehavior::PublicRoutable`. I traced the classifier (`crates/nmp-core/src/publish/policy.rs:123`): kind 14 and 1059 → `PrivateFailClosed`; kind 0/3/10003 → `ReservedBuilderOnly`; kind 10002 → `DiscoveryIndexable`; kind 1/30023 → `PublicRoutable`. Only the last gets the tag. The 8 unit tests assert exactly this (giftwrap 1059, chat 14, profile 0, discovery 10002 all NOT tagged) and pass.

- **No leak path.** All sign-then-publish paths in `commands/publish.rs` are: the two instrumented arms (`publish_unsigned_event:68`, `publish_unsigned_event_to_relays:173`), plus `publish_profile` (kind:0), `follow`/`follow_many` (kind:3). The latter three are `ReservedBuilderOnly`, so they correctly never receive a client tag (NIP-89 tags belong on notes, not profile/contact metadata). No `PublicRoutable` publish bypasses `finalize_outbound_tags`.
- **`publish_signed_event` (pre-signed) correctly does NOT call finalize** — you cannot mutate tags on an already-signed event without breaking the sig. Its existing D10 routing gate (`policy.rs:312`) still protects private envelopes. Consequence: pre-signed *public* events won't carry the client tag, which is the only correct behavior here.
- **Tag-before-sign ordering correct:** `finalize_outbound_tags` at line 68 runs before `sign_active_nonblocking` at line 72 — tags are in the signed payload.

**Flow A transport — SOUND, unbroken chain.** `NmpDefaults.client_identity` → `set_relay_user_agent` (`defaults/src/lib.rs:417`) → FFI slot (`relay_config.rs:124`, pre-start-gated) → `CompositionConfig.user_agent` → `ActorConfigSources.user_agent` → `ActorConfig.user_agent` → `build_pool` (`config.rs:206`) → `PoolConfig.user_agent` → `PoolInner` → `spawn_relay_worker_with_keepalive(... user_agent)` → `run_relay_worker` → `open_relay_socket(url, ua)` → `resolve_user_agent` → handshake header. No break.

- **CRLF/header-injection safe.** `HeaderValue::from_str(&ua).unwrap_or_else(fallback)` (`connect.rs:66`): `from_str` rejects CR/LF/control bytes, so a malicious app name falls back to `nmp/<ver>` rather than injecting headers. Fallback is the right behavior.

**D0 / crate boundaries — CLEAN.** nmp-core has no `nmp-nip89` dep; the only `ClientIdentity` mention in nmp-core is a doc-comment; the kernel field is the generic `outbound_public_tags: Vec<Vec<String>>`. `nmp-nip89` is a leaf crate depending only on `nostr`.

**Non-breaking — SAFE.** `client_identity: None` / `attach_client_tag: false` defaults; `..Default::default()` used at both construction sites. All 4 `RoutingFactoryRegistrar` implementors (NmpApp, builder, 2 test spies) updated, so the trait change is an internal hard-compile-break, not silent — external rev-pinned consumers don't implement this internal trait.

**File-size splits — behavior-preserving.** Visibility changed `fn` → `pub(super) fn` for the moved publish-failure helpers (correct, same-module-tree access). `pub mod testing` retains its exact cfg gate `#[cfg(all(any(test, feature = "test-support"), feature = "native"))]`. Baseline correctly lowered (split, not ratcheted).

**NIP-89 rendering — correct.** `31990:<pubkey>:<d>` coordinate, tag order `["client", name, coord?, relay_hint?]`, empty-d allowed, empty relay-hint rejected, pubkey validated via `nostr::PublicKey::from_hex`. 9 tests pass.

### Findings

**MEDIUM — `crates/nmp-network/src/relay_worker/tests.rs:25`: dead import.** After the jitter tests moved to `jitter_tests.rs`, `jittered_backoff` remains in the `use` list but is no longer referenced in `tests.rs`. Confirmed via `cargo build -p nmp-network --tests`: `warning: unused import: jittered_backoff`. Not a hard CI failure (CI sets `RUSTFLAGS: ""`, no `-D warnings`, no clippy gate found in workflows), but it's a real regression from the split. **Fix:** remove `jittered_backoff` from the import list at line 25.

**LOW — Flow-B coverage is unit-only.** The privacy gate is tested at the decision-function level (excellent, 8 cases) but there's no actor/integration test asserting a real `publish_unsigned_event` for kind:1 emits a `client` tag while a DM publish does not. The decision site is single and well-tested, so this is acceptable, but an end-to-end assertion would harden against future refactors that might route around `finalize_outbound_tags`.

**NIT — `connect.rs:66`:** the fallback silently swaps an invalid configured UA for `nmp/<ver>` with no log. A `tracing::warn!` on the rejected UA would aid diagnosis if an app ships a bad name. Optional.

### Verdict

**SHIP** — no blockers or highs. The privacy gate (the leak-risk dimension) is correct and tested; both flows are wired end-to-end; D0 is clean; non-breaking. The one MEDIUM (dead `jittered_backoff` import at `tests.rs:25`) is a trivial one-line cleanup that should be fixed before merge but does not block CI.
