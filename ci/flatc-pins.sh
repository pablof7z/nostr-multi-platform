# shellcheck shell=bash
#
# #1723 (epic #1719) — the SINGLE SOURCE for the three intentionally-skewed
# flatc version pins. Sourced by every flatc drift gate
# (ci/check-{rust,swift,kotlin,ts,marmot}-flatc-drift.sh) and the regenerate
# driver (ci/regenerate-flatbuffers.sh) so a version bump is ONE edit here.
#
# These are NOT a free choice: generated FlatBuffers bindings bake a runtime
# guard call (e.g. `FLATBUFFERS_25_2_10()`) that must match the runtime library
# pin on each platform. The skew is therefore deliberate and per-platform:
#
#   FLATC_PIN_RUST_SWIFT  Rust + Swift bindings  — matches `flatbuffers = "…"`
#                         in Cargo.toml and the SPM pin in apps/chirp/ios/project.yml.
#   FLATC_PIN_KOTLIN      Android/Kotlin bindings — matches
#                         `com.google.flatbuffers:flatbuffers-java:…` in
#                         apps/chirp/android/app/build.gradle.kts.
#   FLATC_PIN_TS          Web/TypeScript bindings — matches the `flatbuffers`
#                         dep in web/nmp-gallery/package.json.
#
# ci/check-flatbuffers-version-pins.sh is the authority that asserts each of
# those runtime-library pins (Cargo.toml / gradle / package.json) — and the
# per-job flatc installs in .github/workflows/codegen-drift.yml — equal the
# values declared HERE, so a bump can never be applied to only some surfaces.
#
# Marmot uses the same per-platform pins (Rust+Swift = RUST_SWIFT, Kotlin =
# KOTLIN); it has no TS binding.

# shellcheck disable=SC2034  # sourced by other scripts; not all are used here.
FLATC_PIN_RUST_SWIFT="25.12.19"
# shellcheck disable=SC2034
FLATC_PIN_KOTLIN="25.2.10"
# shellcheck disable=SC2034
FLATC_PIN_TS="25.9.23"
