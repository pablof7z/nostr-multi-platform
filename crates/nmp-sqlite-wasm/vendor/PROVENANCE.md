# Vendored artifact provenance — SQLite-WASM (opfs-sahpool)

This directory vendors a pre-compiled, public-domain SQLite-WASM build from
sqlite.org. It backs `nmp-sqlite-wasm`'s OPFS-SQLite engine (issue #1007,
ADR-0054 §1). Because the artifact lives **outside the Cargo dependency graph**,
it is also outside the only automated supply-chain control (`cargo-deny` /
`cargo-audit`). This file plus the CI re-verify gate
(`ci/verify-sqlite-wasm-artifact.sh`) are the **substitute control**.

## Upstream source

| Field | Value |
| --- | --- |
| Product | SQLite-WASM (WASM/JS distribution) |
| Version | **3.53.2** (SQLite library version 3.53.2) |
| Archive | `sqlite-wasm-3530200.zip` |
| Source URL | <https://sqlite.org/2026/sqlite-wasm-3530200.zip> |
| Download index | <https://sqlite.org/download.html> |
| Docs | <https://sqlite.org/wasm> |
| License | **Public Domain** (SQLite is dedicated to the public domain; see <https://sqlite.org/copyright.html> — "The author disclaims copyright to this source code. In place of a legal notice, here is a blessing: May you do good and not evil…") |

### Archive integrity (as downloaded)

| Hash | Value |
| --- | --- |
| SHA3-256 (sqlite.org-published, on the download page) | `d52a475b2c39a3e0025380503914c5bb479a586a0c8c1d0874a54b8b68bbf9f2` |
| SHA2-256 (independent) | `f14eb7afc88efb7bc1c51e669ff23d08f813c2a996bcc2f76d3bd5086b13f1b6` |

> Authenticity note: sqlite.org publishes **SHA3-256** hashes on its download
> page (not SHA2). The downloaded `sqlite-wasm-3530200.zip` was confirmed to
> match the published SHA3-256 `d52a475b…` exactly (`openssl dgst -sha3-256`),
> establishing upstream authenticity. The SHA2-256 above is recorded for tooling
> that lacks SHA3, and is what the CI gate re-verifies on the extracted files.

## Vendored files

Only the **minimal set required by the opfs-sahpool VFS** is vendored — the
engine `.wasm` plus the canonical ES-module glue. The `installOpfsSAHPoolVfs`
VFS is self-contained inside `sqlite3.mjs`; it needs no worker, no promiser, and
**not** the `sqlite3-opfs-async-proxy.js` helper (that proxy belongs to the
older async `opfs` VFS, which requires COOP/COEP and is deliberately not used).

| File | SHA2-256 | Origin |
| --- | --- | --- |
| `sqlite-wasm/sqlite3.wasm` | `ae1cd941deaa3e6a4880e6f1287a5354cfab9e8dfbbb389158e94026f364da49` | upstream `jswasm/sqlite3.wasm`, verbatim |
| `sqlite-wasm/sqlite3.mjs` | `5e089491fe0ebf810f41be398d357cb9486ef4ac84485fa361d3dd1e77ef0f82` | upstream `jswasm/sqlite3.mjs`, verbatim |
| `sqlite-wasm/nmp-sqlite3-shim.mjs` | — (first-party) | **hand-authored NMP shim glue**, NOT upstream |
| `sqlite-wasm/SHA256SUMS` | — | pinned manifest consumed by the CI gate |

`sqlite3.mjs` (version 3.53.2) is the canonical ES module; it is itself
bundler-compatible and loads `sqlite3.wasm` as a sibling file via
`import.meta.url` (there is no separate `sqlite3-bundler-friendly.mjs` in this
release). The bundler resolution of both is wired in PR-6 (the conformance
vehicle); PR-2 only compile-checks the Rust shim against `wasm32`.

`nmp-sqlite3-shim.mjs` is **our code**, not part of the upstream artifact: it is
the JavaScript half of the wasm-bindgen shim, re-exporting a flat API over
`sqlite3.mjs` so the Rust extern block can bind it. Its integrity is git history
plus the file-size and doctrine gates — it is therefore intentionally **not**
listed in `SHA256SUMS` (the SHA-256 gate pins only the opaque upstream binaries).

## Integrity verification (CI)

`ci/verify-sqlite-wasm-artifact.sh` recomputes the SHA-256 of each vendored
upstream file and fails on any mismatch with `sqlite-wasm/SHA256SUMS`. It runs
on every PR and push via `.github/workflows/supply-chain.yml`. Run locally with:

```sh
bash ci/verify-sqlite-wasm-artifact.sh
```

## Reproducible re-download / re-vendor procedure

To bump the SQLite-WASM version (or re-verify from scratch):

1. Find the current archive on <https://sqlite.org/download.html> (the
   `sqlite-wasm-<n>.zip` row) and note its published SHA3-256.
2. Download and verify authenticity against the published hash:
   ```sh
   curl -fSL -o sqlite-wasm-<n>.zip "https://sqlite.org/<year>/sqlite-wasm-<n>.zip"
   openssl dgst -sha3-256 sqlite-wasm-<n>.zip   # must equal the page's hash
   ```
3. Extract and copy the minimal opfs-sahpool set verbatim:
   ```sh
   unzip -q sqlite-wasm-<n>.zip
   cp sqlite-wasm-<n>/jswasm/sqlite3.wasm crates/nmp-sqlite-wasm/vendor/sqlite-wasm/
   cp sqlite-wasm-<n>/jswasm/sqlite3.mjs  crates/nmp-sqlite-wasm/vendor/sqlite-wasm/
   ```
4. Confirm the artifact still bundles the opfs-sahpool VFS:
   ```sh
   grep -c installOpfsSAHPoolVfs crates/nmp-sqlite-wasm/vendor/sqlite-wasm/sqlite3.mjs
   ```
5. Regenerate the pinned manifest and update this file's hash/version tables:
   ```sh
   cd crates/nmp-sqlite-wasm/vendor/sqlite-wasm
   shasum -a 256 sqlite3.wasm sqlite3.mjs > SHA256SUMS
   ```
6. Re-run the gate and the wasm compile check:
   ```sh
   bash ci/verify-sqlite-wasm-artifact.sh
   cargo check -p nmp-sqlite-wasm --target wasm32-unknown-unknown
   ```
