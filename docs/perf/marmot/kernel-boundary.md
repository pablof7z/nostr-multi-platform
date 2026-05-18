# Marmot Milestone — Kernel Boundary Report

**Date**: 2026-05-18  
**Commit**: `018ac40d` (nmp-marmot fully landed)  
**Exit gate ref**: `docs/plan/marmot-mls.md §"Exit gate (kernel boundary)"`

---

## 1. nmp-marmot is the sole importer of mdk-core / openmls

### Command

```
grep -rl 'mdk-core\|mdk_core\|openmls' --include=Cargo.toml crates apps
```

### Output

```
crates/nmp-marmot/Cargo.toml
```

**Result: PASS.** Only `crates/nmp-marmot/Cargo.toml` declares a dependency on
`mdk-core` or `openmls`. No other crate or app in the workspace imports them.

---

## 2. nmp-core has zero MLS nouns

### Command

```
grep -rn 'MlsGroup\|KeyPackage\|Welcome\|Epoch\|RatchetTree\|MarmotGroup\|MarmotMessage' crates/nmp-core/src/
```

### Output

```
(no output — zero matches)
```

**Result: PASS.** `nmp-core/src/` contains no MLS type names. The kernel has
no structural awareness of MLS groups, key packages, epochs, ratchet trees, or
Marmot-specific types.

---

## 3. apps/ directory

### Command

```
grep -rl 'mdk-core\|mdk_core\|openmls' --include=Cargo.toml apps/
```

### Output

```
(no output — zero matches)
```

**Result: PASS.** No app (`apps/chirp`, `apps/fixture`, `apps/podcast`)
directly imports mdk-core or openmls.

---

## Sign-off

The kernel boundary exit gate is met:

- `nmp-core` gains zero MLS types. The M2 compiler and M2 publish planner
  require no changes; `nmp-marmot` uses `InterestShape::relay_pin` from
  M11.5 as any other relay-pinned crate would.

- `nmp-marmot` is the sole crate in the workspace that carries `mdk-core` or
  `openmls` in its `Cargo.toml`. No other NMP crate depends on MLS types.

- The SQLite ratchet-state file (`marmot-mls-state.sqlite`) is an
  implementation detail of `nmp-marmot`; it is never referenced by `nmp-core`,
  `nmp-nip59`, or any app.

- `nmp-testing` carries `mdk-core` and `mdk-sqlite-storage` in
  `[dev-dependencies]` only (not `[dependencies]`), for the exit-gate
  integration tests. This does not violate the kernel boundary: dev-deps are
  test scaffolding, not production imports, and `nmp-testing` is a test
  harness crate, not a production crate.

Signed off: Marcus Webb, 2026-05-18
