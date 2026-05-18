# NmpHighlighter (M11.5 Step 0 scaffold)

This directory holds the **verbatim copy** of `Highlighter` Swift sources from
`/Users/pablofernandez/Work/hl/app/ios/Highlighter/Sources/`. The copy lock is
the UI-fidelity invariant described in `docs/plan/m11.5-highlighter.md` §Step 0:

> Copy every file in `…/hl/app/ios/Highlighter/Sources/Highlighter/` into
> `ios/NmpHighlighter/Sources/NmpHighlighter/` verbatim. Commit immediately.
> No edits except the minimum needed to compile against placeholder data
> sources (`// MARK: NMP-WIRE` markers).

## What's here

```
ios/NmpHighlighter/
├── Sources/
│   ├── NmpHighlighter/         ← 138 .swift files copied verbatim from
│   │                              hl/app/ios/Highlighter/Sources/Highlighter/
│   ├── Shared/                 ← Shared between app + share extension
│   └── ShareExtension/         ← Share-extension target sources
└── README.md (this file)
```

## What's NOT here yet (Steps 1-5)

- `project.yml` / Xcodeproj — to be generated against this tree once the Rust
  binding crate name is finalized (`highlighter_core` → `nmp_highlighter_core`
  in Step 3).
- `Vendor/` — vendored Rust static lib; lands when codegen for
  `nmp-highlighter-core` ships (Step 3).
- `Core/Generated/highlighter_core.swift` — currently the verbatim file from
  Highlighter; Step 5 regenerates it against the NMP per-app crate.
- Resources/Assets.xcassets / Info.plist sanitisation — Step 5 polish.

## Doctrine

The verbatim copy preserves UI-as-source-of-truth. Editing here is reserved
for Step 5 ("wire each copied Swift view to its Rust view module"), where the
*only* allowed change per file is replacing the data source — the view
shape itself stays identical to the reference app.

The reference for diffs going forward:

```
/Users/pablofernandez/Work/hl/app/ios/Highlighter/Sources/Highlighter/
```

vs

```
/Users/pablofernandez/Work/nostr-multi-platform/ios/NmpHighlighter/Sources/NmpHighlighter/
```

Step 5 acceptance: every Swift view here has at most one `// MARK: NMP-WIRE`
block + a single import of the generated NMP wrapper.

## Companion

- `crates/nmp-highlighter-core/` — Rust extension crate (currently a Step 0
  scaffold; the M11.5 Step 3 surface lands here).
- `crates/nmp-nip29/` — NIP-29 protocol crate the app consumes through
  codegen.
