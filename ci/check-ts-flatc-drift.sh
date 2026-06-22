#!/usr/bin/env bash
#
# TypeScript flatc codegen-drift gate (issue #1209, extended by PR-F2, PR-F3).
#
# The checked-in TypeScript bindings at
#   web/chirp/src/nmp/generated/nmp/
# and, when present on a branch,
#   web/nmp-gallery/src/nmp/generated/nmp/
# cover schemas in five groups:
#   transport  — crates/nmp-core/schema/nmp_update.fbs
#   feed        — crates/nmp-nip01/schema/op_feed.fbs
#              + crates/nmp-nip01/schema/timeline_snapshot.fbs
#              + crates/nmp-content/schema/content_tree.fbs
#              + crates/nmp-feed/schema/feed_home.fbs
#   KPRF        — crates/nmp-core/schema/profile_card.fbs
#              + crates/nmp-core/schema/profile.fbs
#              + crates/nmp-core/schema/claimed_events.fbs
#   KRDG        — crates/nmp-core/schema/relay_diagnostics.fbs
#   NRRD        — crates/nmp-core/schema/ref_rowdelta.fbs (ADR-0063 / #1671)
# All generated with flatc 25.9.23 (the Web/TypeScript runtime pin — see
# ci/check-flatbuffers-version-pins.sh and web/chirp/package.json).
#
# This script regenerates ALL schemas with the PINNED flatc version into one
# temp dir and fails on any file difference so checked-in bindings can never
# drift from the schemas. The version is intentionally different from the
# Rust+Swift pin (25.12.19) and the Kotlin pin (25.2.10); see the comment at
# the top of crates/nmp-core/schema/nmp_update.fbs for the rationale.
#
# Usage:
#   ci/check-ts-flatc-drift.sh
#   ci/check-ts-flatc-drift.sh --write
# Requires: flatc 25.9.23 on PATH.
#
# To regenerate after an intentional schema change:
#   bash ci/regenerate-flatbuffers.sh

set -euo pipefail

MODE="${1:---check}"
case "${MODE}" in
--check|--write) ;;
*)
    echo "ts-flatc-drift: unknown mode '${MODE}' (--check|--write)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

EXPECTED_FLATC_VERSION="25.9.23"
TRANSPORT_SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/nmp_update.fbs"
FEED_INCLUDE_DIR="${REPO_ROOT}/crates/nmp-nip01/schema"
FEED_SCHEMAS=(
  "${REPO_ROOT}/crates/nmp-nip01/schema/timeline_snapshot.fbs"
  "${REPO_ROOT}/crates/nmp-nip01/schema/op_feed.fbs"
  "${REPO_ROOT}/crates/nmp-content/schema/content_tree.fbs"
  "${REPO_ROOT}/crates/nmp-feed/schema/feed_home.fbs"
)
KERNEL_SCHEMA_DIR="${REPO_ROOT}/crates/nmp-core/schema"
# KPRF kernel profile bindings (ADR-0063 #1671): `profile.fbs` (the
# `ProfileSnapshot` root carrying one `ProfileCard`, file id KPRF) is the
# per-ROW payload of the `refs.profile` row-delta projection. `profile_card.fbs`
# is its shared `include`d row table and MUST be a root argument so flatc emits
# `profile-card.ts` (it does not without `--gen-all`). `claimed_events.fbs`
# (KCEV) is the per-row event payload mirror for the gallery's `refs.event`
# consumer. The whole-map `resolved_profiles.fbs` (KRPR) decode is GONE — every
# web consumer now reads the per-key `refs.profile` row-delta cache instead.
KERNEL_SCHEMAS=(
  "${REPO_ROOT}/crates/nmp-core/schema/profile_card.fbs"
  "${REPO_ROOT}/crates/nmp-core/schema/profile.fbs"
  "${REPO_ROOT}/crates/nmp-core/schema/claimed_events.fbs"
)
KRDG_SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/relay_diagnostics.fbs"
# NRRD reference row-delta carrier (ADR-0063 / #1671) — `namespace nmp.refs`,
# generated independently into `nmp/refs/`. The keyed-projection row-delta
# payload every web `RefRowCache` decodes.
REFS_SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/ref_rowdelta.fbs"
CHECKED_IN_ROOTS=("${REPO_ROOT}/web/chirp/src/nmp/generated")
GALLERY_TS_ROOT="${REPO_ROOT}/web/nmp-gallery/src/nmp/generated"
if [[ -d "${GALLERY_TS_ROOT}" ]]; then
    CHECKED_IN_ROOTS+=("${GALLERY_TS_ROOT}")
fi
# components-web ships only the content subtree (content_tree.fbs bindings).
# It is checked separately below with a scoped diff rather than a full-tree diff.
COMPONENTS_WEB_CONTENT_ROOT="${REPO_ROOT}/web/packages/components-web/src/generated"

# ── flatc availability + version guard ──────────────────────────────────────

if ! command -v flatc >/dev/null 2>&1; then
    echo "ts-flatc-drift: flatc not found on PATH." >&2
    echo "  Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "  (Note: the Web/TS pin is ${EXPECTED_FLATC_VERSION}, distinct from the" >&2
    echo "   Rust+Swift pin 25.12.19 and the Kotlin pin 25.2.10.)" >&2
    exit 1
fi

ACTUAL_FLATC_VERSION="$(flatc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [[ "${ACTUAL_FLATC_VERSION}" != "${EXPECTED_FLATC_VERSION}" ]]; then
    echo "ts-flatc-drift: flatc ${ACTUAL_FLATC_VERSION} found, but the TypeScript" >&2
    echo "transport bindings are pinned to flatc ${EXPECTED_FLATC_VERSION}" >&2
    echo "(matching the 'flatbuffers: ^${EXPECTED_FLATC_VERSION}' runtime pin in" >&2
    echo " web/chirp/package.json)." >&2
    echo "" >&2
    echo "Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "" >&2
    echo "NOTE: the Web/TS pin (${EXPECTED_FLATC_VERSION}) is intentionally different from" >&2
    echo "the Rust+Swift pin (25.12.19) and the Kotlin pin (25.2.10)." >&2
    echo "Do not regenerate TypeScript bindings with the Rust/Swift or Kotlin flatc." >&2
    exit 1
fi

# ── Regenerate into a temp dir and diff ────────────────────────────────────

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# ── Transport schema (nmp_update.fbs → nmp/transport/) ──────────────────────
flatc --ts -o "${TMP_DIR}" "${TRANSPORT_SCHEMA}"

# ── Feed schemas (op_feed + deps → nmp/nip01/, nmp/content/, nmp/feed/) ──────
# Must be one invocation: generating op_feed.fbs alone emits a barrel that
# re-exports timeline-event-card.js etc. without generating those files —
# broken imports. timeline_snapshot.fbs must be passed explicitly alongside it.
flatc --ts -o "${TMP_DIR}" \
    -I "${FEED_INCLUDE_DIR}" \
    "${FEED_SCHEMAS[@]}"

# ── KPRF/KCEV kernel schemas (profile_card + profile + claimed_events →
#    nmp/kernel/) ───────────────────────────────────────────────────────────
# profile_card.fbs must be listed as a root argument: profile.fbs /
# claimed_events.fbs include it via `include "profile_card.fbs"`, but without
# --gen-all flatc only emits profile-card.ts when the file is an explicit root
# argument. profile.fbs adds the ProfileSnapshot (KPRF) root that wraps one
# ProfileCard — the per-row payload of the refs.profile projection.
flatc --ts -o "${TMP_DIR}" \
    -I "${KERNEL_SCHEMA_DIR}" \
    "${KERNEL_SCHEMAS[@]}"

# ── KRDG schema (relay_diagnostics → nmp/kernel/) ────────────────────────────
# Self-contained: no includes. Output lands in nmp/kernel/ alongside the KPRF
# bindings.
flatc --ts -o "${TMP_DIR}" \
    -I "${KERNEL_SCHEMA_DIR}" \
    "${KRDG_SCHEMA}"

# ── NRRD refs schema (ref_rowdelta → nmp/refs/) ──────────────────────────────
# Self-contained (no includes). `namespace nmp.refs` lands in nmp/refs/. This is
# the row-delta carrier the per-key RefRowCache merges (ADR-0063 / #1671).
flatc --ts -o "${TMP_DIR}" "${REFS_SCHEMA}"

GENERATED_DIR="${TMP_DIR}/nmp"

if [[ "${MODE}" == "--write" ]]; then
    for checked_in_root in "${CHECKED_IN_ROOTS[@]}"; do
        mkdir -p "${checked_in_root}"
        rm -rf "${checked_in_root}/nmp"
        cp -R "${GENERATED_DIR}" "${checked_in_root}/nmp"
        echo "ts-flatc-drift: wrote ${checked_in_root#${REPO_ROOT}/}/nmp (flatc ${EXPECTED_FLATC_VERSION})"
    done
    # components-web: only the content subtree (content_tree.fbs bindings).
    mkdir -p "${COMPONENTS_WEB_CONTENT_ROOT}/nmp"
    rm -rf "${COMPONENTS_WEB_CONTENT_ROOT}/nmp/content"
    cp -R "${GENERATED_DIR}/content" "${COMPONENTS_WEB_CONTENT_ROOT}/nmp/content"
    echo "ts-flatc-drift: wrote ${COMPONENTS_WEB_CONTENT_ROOT#${REPO_ROOT}/}/nmp/content (flatc ${EXPECTED_FLATC_VERSION})"
    exit 0
fi

drift=0
for checked_in_root in "${CHECKED_IN_ROOTS[@]}"; do
    checked_in_dir="${checked_in_root}/nmp"
    if [[ ! -d "${checked_in_dir}" ]]; then
        echo "ts-flatc-drift: checked-in TypeScript binding dir missing: ${checked_in_dir#${REPO_ROOT}/}" >&2
        drift=$((drift + 1))
        continue
    fi
    if ! diff -r "${checked_in_dir}" "${GENERATED_DIR}"; then
        echo "ts-flatc-drift: ${checked_in_dir#${REPO_ROOT}/} drifted from a fresh flatc run." >&2
        drift=$((drift + 1))
    fi
done

# components-web: content subtree only (content_tree.fbs bindings).
COMPONENTS_WEB_CHECKED_IN="${COMPONENTS_WEB_CONTENT_ROOT}/nmp/content"
if [[ ! -d "${COMPONENTS_WEB_CHECKED_IN}" ]]; then
    echo "ts-flatc-drift: checked-in TypeScript binding dir missing: ${COMPONENTS_WEB_CHECKED_IN#${REPO_ROOT}/}" >&2
    drift=$((drift + 1))
elif ! diff -r "${COMPONENTS_WEB_CHECKED_IN}" "${GENERATED_DIR}/content"; then
    echo "ts-flatc-drift: ${COMPONENTS_WEB_CHECKED_IN#${REPO_ROOT}/} drifted from a fresh flatc run." >&2
    drift=$((drift + 1))
fi

if [[ "${drift}" -ne 0 ]]; then
    echo "" >&2
    echo "ts-flatc-drift: checked-in TypeScript bindings differ from a fresh" >&2
    echo "'flatc --ts' run. Regenerate with:" >&2
    echo "  bash ci/regenerate-flatbuffers.sh" >&2
    exit 1
fi

echo "ts-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, transport + feed + KPRF + KRDG + NRRD bindings in sync across ${#CHECKED_IN_ROOTS[@]} TS tree(s))"
