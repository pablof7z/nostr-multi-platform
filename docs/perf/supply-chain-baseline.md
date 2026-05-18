# Supply-Chain Security Baseline

- **Captured**: 2026-05-18
- **Commit**: 1af1d22c6f64400d8a41eed811c2dc5b2d04c24a
- **Tools**: cargo-audit 0.22.1, cargo-deny 0.19.6
- **Cargo.lock**: 178 crate dependencies

This document records the first-run output of `cargo audit` and `cargo deny check` so that future regressions are immediately visible by diff.

---

## cargo audit

```
Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
  Loaded 1090 security advisories
  Scanning Cargo.lock for vulnerabilities (178 crate dependencies)

Crate:     instant
Version:   0.1.13
Warning:   unmaintained
Title:     `instant` is unmaintained
Date:      2024-09-01
ID:        RUSTSEC-2024-0384
URL:       https://rustsec.org/advisories/RUSTSEC-2024-0384
Dependency tree:
instant 0.1.13
└── nostr 0.44.2

Crate:     paste
Version:   1.0.15
Warning:   unmaintained
Title:     paste - no longer maintained
Date:      2024-10-07
ID:        RUSTSEC-2024-0436
URL:       https://rustsec.org/advisories/RUSTSEC-2024-0436
Dependency tree:
paste 1.0.15
└── nmp-testing 0.1.0

warning: 2 allowed warnings found
```

**Result**: 0 vulnerabilities, 2 unmaintained warnings.

### Interpretation

- `instant` (RUSTSEC-2024-0384): No safe upgrade available. Pulled in transitively via `nostr@0.44.2`. Track the `nostr` crate for a release that migrates to `web-time`.
- `paste` (RUSTSEC-2024-0436): No safe upgrade available. Used by `nmp-testing` for macro test helpers. Low security risk (proc-macro, no network exposure). Track dtolnay's successor recommendation.

---

## cargo deny check

```
warning[license-not-encountered]: "Unicode-DFS-2016" unmatched (in allow list but not seen in deps)
warning[license-not-encountered]: "MPL-2.0" unmatched (in allow list but not seen in deps)
warning[duplicate]: getrandom 0.2.17 / 0.3.4
warning[duplicate]: rand 0.8.6 / 0.9.4
warning[duplicate]: rand_chacha 0.3.1 / 0.9.0
warning[duplicate]: rand_core 0.6.4 / 0.9.5
error[unmaintained]: instant (RUSTSEC-2024-0384)
error[unmaintained]: paste (RUSTSEC-2024-0436)

advisories FAILED, bans ok, licenses ok, sources ok
```

**Result**: licenses ok, bans ok (warnings only), sources ok; advisories FAILED on 2 unmaintained crates (same as cargo audit).

### License inventory (as of baseline)

| SPDX ID | Crate count |
|---|---|
| MIT | 146 |
| Apache-2.0 | 51 |
| Unicode-3.0 | 19 |
| BSD-3-Clause | (subset of Apache-2.0 set) |
| BSD-2-Clause | (subset) |
| CC0-1.0 | bitcoin-io, bitcoin_hashes |
| CDLA-Permissive-2.0 | 1 |
| LGPL-2.1-or-later | r-efi (Windows EFI types; build-time only) |
| ISC | (subset) |
| Zlib | (subset) |
| Unlicense | (subset) |

No GPL-2.0, GPL-3.0, or AGPL licensed crates detected.

### Duplicate version warnings

These are expected from the ecosystem convergence between `nostr@0.44.2` (pins rand 0.8.x) and `ulid@1.2.1` (pins rand 0.9.x). Treated as `warn`; not blocking. Resolve naturally when `nostr` upgrades to rand 0.9.

| Crate | Versions |
|---|---|
| getrandom | 0.2.17, 0.3.4 |
| rand | 0.8.6, 0.9.4 |
| rand_chacha | 0.3.1, 0.9.0 |
| rand_core | 0.6.4, 0.9.5 |

---

## CI workflow

`.github/workflows/supply-chain.yml` runs on every PR and every push to master.

- **audit job**: `cargo audit` via `taiki-e/install-action`. Warning-only advisories are non-blocking; actual vulnerabilities fail the job.
- **deny job**: `EmbarkStudios/cargo-deny-action@v2` using `deny.toml` at repo root. Currently fails on advisories (same 2 unmaintained crates). Licenses, bans, and sources pass.

### Next steps to clear this baseline

1. **instant**: File a tracking issue against the `nostr` crate; watch for `nostr@0.45+` which should migrate to `web-time`.
2. **paste**: Replace `paste!` macro usage in `nmp-testing` with `pastey` or `with_builtin_macros` if the crate continues to show security advisories.
3. **deny advisories**: Once the two `ignore` IDs are added to `deny.toml [advisories].ignore`, the CI job will be fully green. Only add them after confirming no upgrade path exists.
