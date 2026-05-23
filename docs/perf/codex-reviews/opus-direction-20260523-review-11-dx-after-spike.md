# Opus direction review #11 — DX after the Notes spike (2026-05-23)

Brief: "What does 'the framework works' mean for a developer who wants to build app #3 TODAY?" Stateful spike landed in `d5372e09` (25 LOC Rust + 299 LOC SwiftUI, zero new C-ABI symbols, NIP-01 read + publish + nsec/NIP-46 signin). Thesis is empirically confirmed for stateful apps — the next question is whether a developer who has *not* read the spike can repeat the trick.

> **Note on F-08.** The brief instructs me to read "F-08 NmpAppBuilder" in `docs/BACKLOG.md`. Grep returns F-01..F-07 only; F-08 is not in the file as of this commit. I am treating it as a *proposal* the brief is introducing, not a documented backlog item. §4 below sketches what it would need to deliver.

---

## 1. The step-by-step DX of building app #3

Reconstructed from `apps/notes/nmp-app-notes/{Cargo.toml,src/lib.rs}` + `apps/notes/ios/Notes/Bridge/NotesBridge.swift`:

1. Create `apps/foo/nmp-app-foo/Cargo.toml`. Add **two** deps and remember the feature flag: `nmp-core = { path = "...", features = ["android-ffi"] }` and `nmp-signer-broker`. If you forget `features = ["android-ffi"]`, your iOS link fails on `nmp_app_signin_bunker`, `nmp_app_switch_active`, `nmp_app_remove_account`, `nmp_app_stop` — undefined symbols, no compiler hint (`crates/nmp-core/src/lib.rs:146-149`). That is the **first CI error a new developer will see**, and the message is from `ld`, not from rustc.
2. Write `src/lib.rs` with the two `#[allow(unused_imports)] pub use` blocks (`apps/notes/nmp-app-notes/src/lib.rs:51-57`). Without the glob re-export, `#[no_mangle]` symbol bodies stay `U` in `libnmp_app_foo.a`. There is no compile-time check; the failure shows up at the iOS link stage. The `#[allow]` is required because rustc otherwise warns on every glob symbol.
3. Add an `nmp_app_foo_init` marker (`apps/notes/nmp-app-notes/src/lib.rs:73-76`). The Notes spike's README admits this is empty and would still link if deleted; it survives only as a forward-compatibility seam.
4. Copy `ios/Chirp/Chirp/Bridge/NmpCore.h` verbatim into `apps/foo/ios/Foo/Bridge/NmpCore.h`. The two existing copies (`apps/notes/...NmpCore.h` vs. `ios/Chirp/...NmpCore.h`) are **byte-identical, 448 lines each** (`diff -q` returns clean). The Notes README documents this as intentional.
5. Mirror the init order from `apps/notes/ios/Notes/Bridge/NotesBridge.swift:16-30`: `nmp_app_new` → `set_storage_path` → `nmp_signer_broker_init` → `nmp_app_foo_init` → later `nmp_app_start(_, 0, 0, 0)`. The 3 zero args are unexplained at the call site; their meaning lives in `NmpCore.h`.
6. To publish, hand-build a JSON dict: `["PublishNote": ["content": …, "reply_to_id": NSNull(), "target": "Auto"]]` (`NotesBridge.swift:50-52`). The shape is discoverable only by reading `crates/nmp-core/src/publish/action.rs:94-98` in Rust source — there is no Swift type, no schema, no doc string on the Swift side.

## 2. The three DX gaps

**Gap A — `NmpCore.h` is copy-pasted, not generated.** `apps/notes/ios/Notes/Bridge/NmpCore.h` and `ios/Chirp/Chirp/Bridge/NmpCore.h` are byte-identical 448-line files. Every new iOS app forks this header by hand, and there is no CI gate that the fork stays in sync with the Rust surface (`ci/check-ffi-surface-freeze.sh` freezes the Rust side, not the per-app header copies).

**Gap B — the action JSON shape is undiscoverable from the host language.** `NotesBridge.swift:50-52` hard-codes a `PublishNote` dict with three keys, two strings and a `NSNull`. The schema lives in `crates/nmp-core/src/publish/action.rs:94-98` as a Rust enum variant. Misnaming `reply_to_id` as `replyToId`, or passing `"target": "auto"` (lowercase), silently rejects the action — the dispatch returns `{"error":"…"}` JSON (`crates/nmp-core/src/ffi/action.rs:87-88`) which `publishNote` discards (`NotesBridge.swift:57-58`).

**Gap C — symbol bodies vs. Rust dead-code analysis.** `apps/notes/nmp-app-notes/src/lib.rs:51-57` requires `#[allow(unused_imports)]` on the glob re-exports because without it `cargo check` warns on each. The crate "looks unused" to rustc but is load-bearing to `ld`. A developer who follows the warning and deletes the glob produces a clean `cargo build` and a broken `.a` archive — caught only at iOS link time.

## 3. The `android-ffi` feature verdict

It is a **structural** problem dressed as a naming problem. `crates/nmp-core/src/lib.rs:146-149` gates four symbols on `android-ffi`: `nmp_app_remove_account`, `nmp_app_signin_bunker`, `nmp_app_stop`, `nmp_app_switch_active`. Three of those are lifecycle essentials any iOS or web app needs (NIP-46 signin, actor shutdown, multi-account switch); only the wallet gate at `lib.rs:155-156` is a legitimate domain opt-in.

The correct fix is **not** renaming `android-ffi` → `ffi`. The four lifecycle symbols belong unconditionally in the `native` re-export block (`lib.rs:108-120`). The `android-ffi` feature should either be deleted (if no symbol is genuinely Android-only) or shrunk to true Android JNI deltas (none exist today, per the Notes README and `lib.rs:140-145`). Renaming preserves the defect; moving the symbols removes it.

## 4. What an `NmpAppBuilder` (proposed F-08) would minimally deliver

To support a 15-minute "publish your first kind:1" experience: a typed Rust + Swift surface that (a) abstracts the new→set_storage_path→signer_broker_init→app_init→start dance (`NotesBridge.swift:16-30`) into a single `NmpAppBuilder::new().storage(…).with_signer_broker().build_and_start()`, (b) ships a generated `NmpCore.h` (one source, cbindgen or equivalent) so the per-app copy disappears, and (c) ships a Swift `PublishAction` enum codegen so `publishNote(content)` compiles instead of `["PublishNote": …]` succeeding-or-silently-failing at runtime. Anything beyond these three is scope creep for the minimum-viable surface.

## 5. What NOT to change

- **The `#[allow(unused_imports)]` glob re-exports at `apps/notes/nmp-app-notes/src/lib.rs:51-57`.** The comment block at lines 43-50 already explains the linker-vs-rustc asymmetry; the workaround is correct. (The right fix is at the build-system / `cargo metadata` layer, not in app crates.)
- **The empty `nmp_app_notes_init` marker at `lib.rs:73-76`.** Looks like dead code; it is the documented extension point for future per-app projection registration without changing the FFI shape. Deleting it removes a forward-compatibility seam for $0 reward.

## 6. 30-day call

**Move `nmp_app_remove_account`, `nmp_app_signin_bunker`, `nmp_app_stop`, `nmp_app_switch_active` out of `#[cfg(feature = "android-ffi")]` in `crates/nmp-core/src/lib.rs:146-149` and into the unconditional `#[cfg(feature = "native")]` block at `lib.rs:108-120`. Then drop `features = ["android-ffi"]` from `apps/notes/nmp-app-notes/Cargo.toml:28` and `apps/longform/nmp-app-longform/Cargo.toml`, run iOS link, confirm green.** Independently verifiable: `grep -rn 'features = \["android-ffi"\]' apps/` returns zero matches, `cargo build -p nmp-core --no-default-features --features native` exports all four symbols, Notes and Longform iOS apps still link. If the `android-ffi` feature has no remaining consumers after this, delete it in the same PR.
