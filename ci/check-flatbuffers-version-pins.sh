#!/usr/bin/env bash
#
# Guard the intentionally-skewed FlatBuffers runtime versions used by the
# runtime update transport bindings. Wire format compatibility is stable across
# these patch lines, but generated bindings bake runtime guard calls such as
# `FLATBUFFERS_25_2_10()`. If a developer regenerates one platform with a
# different `flatc`, this check fails before the mismatch reaches CI builds.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

require_line() {
    local file="$1"
    local needle="$2"
    if ! grep -Fq "${needle}" "${REPO_ROOT}/${file}"; then
        echo "flatbuffers-version-pins: ${file} missing expected line:" >&2
        echo "  ${needle}" >&2
        exit 1
    fi
}

require_line "Cargo.toml" 'flatbuffers = "25.12.19"'
require_line "ios/Chirp/project.yml" "from: 25.12.19"
require_line "ios/Chirp/Chirp.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved" '"version" : "25.12.19"'
require_line "apps/nmp-gallery/android/app/build.gradle.kts" 'implementation("com.google.flatbuffers:flatbuffers-java:25.2.10")'
require_line "android/app/build.gradle.kts" 'implementation("com.google.flatbuffers:flatbuffers-java:25.2.10")'
require_line "web/chirp/package.json" '"flatbuffers": "^25.9.23"'
require_line "web/chirp/package-lock.json" '"version": "25.9.23"'

while IFS= read -r file; do
    require_line "${file#"${REPO_ROOT}/"}" "FLATBUFFERS_25_2_10()"
done < <(grep -rl "fun validateVersion" \
    "${REPO_ROOT}/apps/nmp-gallery/android/app/src/main/kotlin/nmp/transport" | sort)

echo "flatbuffers-version-pins: OK"
