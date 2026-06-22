#!/usr/bin/env bash
#
# Guard the intentionally-skewed FlatBuffers runtime versions used by the
# runtime update transport bindings. Wire format compatibility is stable across
# these patch lines, but generated bindings bake runtime guard calls such as
# `FLATBUFFERS_25_2_10()`. If a developer regenerates one platform with a
# different `flatc`, this check fails before the mismatch reaches CI builds.
#
# #1723 — this gate is the AUTHORITY that ties every flatc-version surface back
# to the single source `ci/flatc-pins.sh`. The drift scripts + the regenerate
# driver `source` that file directly (so they cannot drift); the surfaces that
# CANNOT source it — the runtime-library pins (Cargo.toml / gradle /
# package.json), the generated-binding runtime guard calls, and the per-job
# `flatc` installs in .github/workflows/codegen-drift.yml — are asserted HERE to
# equal the pins-file values. A version bump is therefore one edit in
# `ci/flatc-pins.sh`; any surface left behind fails this gate.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Single source of truth for the three pins.
# shellcheck source=ci/flatc-pins.sh
source "${SCRIPT_DIR}/flatc-pins.sh"

require_line() {
    local file="$1"
    local needle="$2"
    if ! grep -Fq "${needle}" "${REPO_ROOT}/${file}"; then
        echo "flatbuffers-version-pins: ${file} missing expected line:" >&2
        echo "  ${needle}" >&2
        echo "  (expected pin from ci/flatc-pins.sh — bump there, then propagate)" >&2
        exit 1
    fi
}

no_stale_kotlin_gradle_pin() {
    # Every `flatbuffers-java:<ver>` literal in a gradle file (both
    # `implementation` and `testImplementation`) MUST be the Kotlin pin. Catches
    # a duplicate dependency line left stale after a bump.
    local file="$1"
    local stale
    stale="$(grep -oE 'flatbuffers-java:[0-9]+\.[0-9]+\.[0-9]+' "${REPO_ROOT}/${file}" \
        | grep -vF "flatbuffers-java:${FLATC_PIN_KOTLIN}" || true)"
    if [[ -n "${stale}" ]]; then
        echo "flatbuffers-version-pins: ${file} has a flatbuffers-java pin that is not" >&2
        echo "the Kotlin pin (${FLATC_PIN_KOTLIN}):" >&2
        echo "${stale}" | sed 's/^/  /' >&2
        echo "  (pin from ci/flatc-pins.sh — bump there, then propagate everywhere)" >&2
        exit 1
    fi
}

# ── Runtime-library pins (cannot source the shell file) ─────────────────────
# Rust + Swift.
require_line "Cargo.toml" "flatbuffers = \"${FLATC_PIN_RUST_SWIFT}\""
require_line "ios/Chirp/project.yml" "from: ${FLATC_PIN_RUST_SWIFT}"
require_line "ios/Chirp/Chirp.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved" "\"version\" : \"${FLATC_PIN_RUST_SWIFT}\""
# Android/Kotlin — both gradle files, every dependency line (impl + testImpl).
require_line "android/app/build.gradle.kts" "flatbuffers-java:${FLATC_PIN_KOTLIN}"
require_line "apps/nmp-gallery/android/app/build.gradle.kts" "flatbuffers-java:${FLATC_PIN_KOTLIN}"
no_stale_kotlin_gradle_pin "android/app/build.gradle.kts"
no_stale_kotlin_gradle_pin "apps/nmp-gallery/android/app/build.gradle.kts"
# Web/TypeScript — every package.json that pins flatbuffers + the lockfile.
require_line "web/chirp/package.json" "\"flatbuffers\": \"^${FLATC_PIN_TS}\""
require_line "web/nmp-gallery/package.json" "\"flatbuffers\": \"^${FLATC_PIN_TS}\""
require_line "web/packages/runtime-web/package.json" "\"flatbuffers\": \"^${FLATC_PIN_TS}\""
# The lockfile is npm-derived from those manifests, but a hand-edit could leave a
# stale flatbuffers version behind: assert NO flatbuffers reference (the `^…`
# spec lines, the resolved `version`, or the registry tarball URL) names anything
# other than the TS pin.
lock_stale="$(grep -nE 'flatbuffers' web/package-lock.json \
    | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' \
    | grep -vF "${FLATC_PIN_TS}" || true)"
if [[ -n "${lock_stale}" ]]; then
    echo "flatbuffers-version-pins: web/package-lock.json has a flatbuffers version" >&2
    echo "that is not the TS pin (${FLATC_PIN_TS}):" >&2
    echo "${lock_stale}" | sed 's/^/  /' >&2
    echo "  (re-run npm install after bumping ci/flatc-pins.sh + the package.json files)" >&2
    exit 1
fi

# ── CI workflow per-job flatc installs (YAML cannot source the shell file) ──
# Each drift job downloads a pinned flatc release tarball. Rather than count
# per-version occurrences (which would pass if one job were bumped and another
# left stale), assert that EVERY `flatbuffers/releases/download/v…` URL in the
# workflow names one of the three current pins — so any stale install URL fails.
WORKFLOW=".github/workflows/codegen-drift.yml"
stale_urls="$(grep -oE 'flatbuffers/releases/download/v[0-9]+\.[0-9]+\.[0-9]+' "${REPO_ROOT}/${WORKFLOW}" \
    | sort -u \
    | grep -vE "/v(${FLATC_PIN_RUST_SWIFT}|${FLATC_PIN_KOTLIN}|${FLATC_PIN_TS})\$" || true)"
if [[ -n "${stale_urls}" ]]; then
    echo "flatbuffers-version-pins: ${WORKFLOW} installs a flatc version that is not" >&2
    echo "one of the current pins (rust+swift=${FLATC_PIN_RUST_SWIFT}, kotlin=${FLATC_PIN_KOTLIN}, ts=${FLATC_PIN_TS}):" >&2
    echo "${stale_urls}" | sed 's/^/  /' >&2
    echo "  (pin from ci/flatc-pins.sh — bump there, then update every install site)" >&2
    exit 1
fi
# And assert each pin is actually installed by at least one job (no pin silently
# dropped from CI).
for pin in "${FLATC_PIN_RUST_SWIFT}" "${FLATC_PIN_KOTLIN}" "${FLATC_PIN_TS}"; do
    if ! grep -qF "flatbuffers/releases/download/v${pin}/" "${REPO_ROOT}/${WORKFLOW}"; then
        echo "flatbuffers-version-pins: ${WORKFLOW} has no flatc install for pin v${pin}" >&2
        echo "  (every pin in ci/flatc-pins.sh must be installed by a drift job)" >&2
        exit 1
    fi
done

# ── Generated-binding runtime guard calls (baked into flatc output) ─────────
# flatc emits a `FLATBUFFERS_<MAJOR>_<MINOR>_<PATCH>()` guard call in each Kotlin
# binding; it MUST match the Kotlin runtime pin. Derive the needle from the pin.
KOTLIN_GUARD="FLATBUFFERS_${FLATC_PIN_KOTLIN//./_}()"

while IFS= read -r file; do
    require_line "${file#"${REPO_ROOT}/"}" "${KOTLIN_GUARD}"
done < <(grep -rl "fun validateVersion" \
    "${REPO_ROOT}/apps/nmp-gallery/android/app/src/main/kotlin/nmp/transport" | sort)

while IFS= read -r file; do
    require_line "${file#"${REPO_ROOT}/"}" "${KOTLIN_GUARD}"
done < <(grep -rl "fun validateVersion" \
    "${REPO_ROOT}/android/app/src/main/java/nmp" | sort)

echo "flatbuffers-version-pins: OK (rust+swift=${FLATC_PIN_RUST_SWIFT}, kotlin=${FLATC_PIN_KOTLIN}, ts=${FLATC_PIN_TS})"
