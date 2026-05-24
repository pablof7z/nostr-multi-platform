# Opus Direction Review #16 — The Framework Has No Inhabitants

**Date:** 2026-05-24
**HEAD reference:** master (per BACKLOG.md HEAD `76bc8547`, plus V-22 through V-34 sweep)
**Reviewer scope:** Contrarian direction review. Files cited: aim.md, plan.md, BACKLOG.md, kernel/mod.rs, ffi/mod.rs, actor/dispatch.rs, display.rs, nmp-wasm/lib.rs, apps/notes/.

---

## The single load-bearing call

**`apps/notes/` is still in tree, PD-033-A is still re-opened, and the project has not answered Opus #13.** That is the most important fact in this codebase. Notes is the *only* extant attempt by anyone — even a friendly first-party agent — to build a non-Chirp app on the framework. Review #13 verified empirically that it bypasses every defining property: `NotesBridge.swift:74` registers a raw kind filter `"[1]"` rather than a `LogicalInterest`, so D3 outbox routing is bypassed; `NoteModel.swift:14` parses event JSON in Swift via `JSONSerialization`, the architectural bible's *first* anti-pattern (aim.md §2); `NotesBridge.swift:84` inserts notes at the head of the array — the kernel owns no timeline view for the app, ordering is Swift-side; `NotesBridge.swift:34,37` flip `isSignedIn = true` synchronously, no handshake-success gate. The 299 LOC count was real. The proof was not.

This is not a bug in Notes. It is a failure of the framework to be hard to misuse — aim.md §1's central design claim: *"make it nearly impossible to build a broken Nostr application."* A friendly agent given the substrate produced a broken one in the easiest, most obvious way. The framework's response, twenty days later, is V-22 through V-34: fourteen PRs moving 50–100 LOC of formatting per PR out of Swift and into Rust. **The team is cleaning the house the framework's only inhabitant moved out of.** Plan.md TL;DR still calls Notes a thesis confirmation. It is the thesis *refutation*.

Until `apps/notes/` is either rewritten to use a kernel-owned timeline projection, a `LogicalInterest` for kind:1 from the active user's follow set, and a real handshake gate — *or deleted with a written acknowledgement that the substrate is not yet expressive enough to support it* — every other v1 claim is unverifiable.

## What NMP should support that it doesn't

**1. A documented public API.** The C-ABI surface (48 symbols in `crates/nmp-core/src/ffi/`) is *wire transport*, not the API. The real API is the `dispatch_action` namespace catalog: `nmp.publish`, `nmp.nip17.*`, `nmp.nip57.*`, `nmp.nip65.*`, `nmp.follow`, `nmp.unfollow`, `nmp.nip25.react`, `nmp.wallet.pay_invoice`. There is no catalog file. A third developer cannot find what to call, what JSON shape each namespace expects, or which projections they should subscribe to. PD-039 inventories the *symbols*; nothing inventories the *contracts*. This is the highest-leverage missing artifact in the entire workspace — one markdown file would change every developer's first hour.

**2. A `LogicalInterest` builder + a documented timeline projection contract.** Notes had to use `nmp_app_register_raw_event_observer` because no documented path exists to say "give me kind:1 from these authors, outbox-routed, kernel-ordered." `InterestRegistry` exists (V-04), `TimelineProjection` exists (`crates/nmp-nip01/src/timeline_projection.rs`), but together they are not a discoverable surface. The third developer would write the same bypass Notes wrote.

**3. An IndexedDB-backed nostr-database impl (F-01).** Without it, "cross-platform" remains false per plan.md v1 exit criterion #6. Stages 2–3c are merged; the kernel still resets on page reload. No persistent chirp-web feature can be added. This is the only WASM v1-blocker, and the calendar has slipped.

## What NMP shouldn't do that it does

**1. The thin-shell formatting sweep is now a maintenance subculture.** V-20, V-22, V-23, V-24, V-25, V-26, V-27, V-28, V-33 — every one moves `relativeTime`/`shortPubkey`/`avatarColor` from Swift into a Rust projection field, then deletes the Swift helper. The work is correct per doctrine, but at fourteen PRs in two weeks it is no longer a violation backlog — it is the project's tempo. **Cap it.** After a fixed date, Swift formatting drift is product polish, not a framework concern. Otherwise every native shell rewrite forever spawns a V-XX sweep, and contributor attention compounds into a domain (display strings) the framework barely advertises.

**2. NIP-46 reimplementation lacks an ADR (Opus #11 raised this, still open).** aim.md §3 names `nostr-connect` (the rust-nostr NIP-46 crate) as the dependency. NMP shipped `nmp-signer-broker` instead. V-13 (polling), V-14 (no reconnect), V-06 (NIP-42 incompatibility), V-08 (DM gift-wrap) are all *fix* tickets on a *should-this-exist* question. The framework's own doctrine corollary "Use rust-nostr" was violated without writing down why; the post-hoc fixes do not retroactively justify the divergence.

**3. The 48-symbol Theme A reclassification is convenient.** PD-039 reclassifies 26 of 48 bespoke symbols as "structural permanent." Lifecycle, callbacks, capability sockets, observer registration, NWC connection, publish control plane, liveness probe. Some of those are genuinely permanent (`nmp_app_new`, `nmp_app_free`, the update callback). Others — observer registration as eight separate symbols when `dispatch_action` could carry a `subscribe` verb; the NWC connection lifecycle as three symbols when wallet is supposed to be ActionModule-backed — look like the calendar accepting the current shape to declare itself done. Batch 1 deletion count is zero. Re-audit Theme A with the question: "would I add this symbol *de novo* in a UniFFI world (M14)?" If no, it is debt, not doctrine.

## What could be dramatically better

**1. Adopt one external developer publicly — even a contrived one — and gate v1 on what they hit.** Notes (rewritten honestly) plus a stateful read-write app neither the maintainers nor a friendly agent wrote — say, a hackday participant with the spec + scaffolding. The thesis is unfalsifiable until somebody outside the system tries to use it. The cost of every other improvement is unverifiable without this oracle.

**2. Delete `nmp-wasm` "no longer a stub" copy until F-01 lands.** Plan.md TL;DR currently sells wasm as substantially-shipped. It is a kernel-in-memory-without-persistence. The honest version of v1 exit #6 reads "iOS + macOS + Android, web preview only" — that is fine, and it removes the IndexedDB urgency from the v1 path entirely.

**3. Promote dispatch_action namespaces to first-class.** Each namespace gets a registered schema (the codegen-schema seam already exists, gated on `codegen-schema`). The Swift bridge generates a typed `dispatch.nmp.publish.PublishNote(...)` API from the schema. The 16 migration-debt FFI symbols disappear by construction — not by manual conversion. F-05 codegen would extend trivially. This is the step-change that turns Notes' Swift-`Any`-JSON bridge into a typed call.

---

## Three-sentence summary

The single most important fact about NMP today is that `apps/notes/` — the only outside-of-Chirp inhabitant of the framework — bypasses every defining property of the architecture, PD-033-A has been re-open for twenty days, and the project has answered with fourteen Swift-formatting-cleanup PRs that the framework's only user does not benefit from. The thin-shell sweep, the 48-symbol Theme A "structural" reclassification, and the half-shipped WASM claim are all symptoms of a codebase getting very good at cleaning a house with no inhabitants; the load-bearing call is to rewrite Notes honestly (or delete it) and only then trust any v1-exit-criterion check. Until the framework is provably hard to misuse by someone other than its maintainers, every other direction-review finding — including this one — is theoretical.
