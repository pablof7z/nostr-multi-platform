# Chirp — Agent Guidance

## Chirp is the full NMP showcase client

Chirp is the production Nostr client and the canonical proof that NMP can
ship a complete app. Its product goal is deliberately broad: every reusable
feature NMP ships should eventually have a real Chirp surface, diagnostics
entry, smoke path, or an explicit platform exception.

The old Twitter-like timeline is only Chirp's first social baseline. It is not
the ceiling.

**Chirp's product surface can be large. Chirp's local logic must stay small.**

Every line of business logic in Chirp defeats the proof. If Chirp has state
management, data transformation, protocol handling, or decision-making code, it
shows that those things are not reusable and that future apps would need to
duplicate them. That is the opposite of what NMP is for.

## What belongs in Chirp

The only code that legitimately belongs in `apps/chirp/` is:

- **`#[no_mangle] extern "C"` FFI symbols** — the C-ABI bridge Swift links against.
- **Platform bootstrap** — initializing the NMP kernel, wiring observers, managing handle lifetimes.
- **String marshalling** — converting between C strings and Rust types at the FFI boundary.
- **D6 null/error degradation** — returning null or `{"ok":false}` when a handle is null or a mutex is poisoned. This is FFI hygiene, not business logic.

## What does NOT belong in Chirp

If you are about to write any of the following inside `apps/chirp/`, stop and
put it in the appropriate NMP crate instead:

- State structs or projections (e.g., `MarmotProjection`, `ChirpTimeline`)
- Business operations or dispatch logic (e.g., `ops::dispatch`, `ops::group_messages`)
- Publish routing or relay-selection logic
- Event ingestion or processing
- Data-transfer objects / DTOs that represent domain concepts
- Any `impl` that contains an `if` deciding what the app should *do*
- Showcase policy, feature eligibility, or protocol-specific UX decisions

## How to decide where code goes

Ask: *"Would a different Nostr app — say, a podcast app or a marketplace — need this same logic?"*

- **Yes** → it belongs in an NMP crate under `crates/`.
- **No, it's genuinely Chirp-specific** → ask again. If it is truly unique to Chirp's domain (which has almost no unique domain — Chirp is a generic Nostr client), only then can it live here, and only in Chirp's own Rust crates, not in the FFI shell itself.

The showcase target raises the bar for what Chirp exposes; it does not relax
the ownership rule. New product capability usually means new reusable Rust
module behavior plus a thin Chirp render/capability surface.

## The canonical bad example

`apps/chirp/crates/nmp-app-chirp/src/marmot/ops.rs`, `state.rs`, `publish.rs`, `tap.rs`, `payload.rs` — ~1400 lines of Marmot projection logic that lived in Chirp and was later migrated to `crates/nmp-marmot/src/projection/`. Do not repeat this mistake.
