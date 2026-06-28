# shellcheck shell=bash
#
# #1723 (epic #1719) — the SINGLE SOURCE for the in-repo flatc version pins.
# Sourced by the FlatBuffers drift gates and the regenerate driver
# (ci/regenerate-flatbuffers.sh) so a version bump is ONE edit here.
#
# These are NOT a free choice: generated FlatBuffers bindings bake a runtime
# guard call (e.g. `FLATBUFFERS_25_2_10()`) that must match the runtime library
# pin on each generated-binding platform. The skew is therefore deliberate and
# per-platform:
#
#   FLATC_PIN_RUST_SWIFT  Rust bindings — matches `flatbuffers = "…"`
#                         in Cargo.toml. External Swift apps own their own SPM
#                         pin checks.
#   FLATC_PIN_KOTLIN      Android/Kotlin runtime — matches
#                         `com.google.flatbuffers:flatbuffers-java:…` in
#                         in-repo Android apps. External Kotlin apps own their
#                         own generated-binding drift checks.
#   FLATC_PIN_TS          Web/TypeScript bindings — matches the `flatbuffers`
#                         dep in web/nmp-gallery/package.json.
#
# ci/check-flatbuffers-version-pins.sh is the authority that asserts each of
# those in-repo runtime-library pins (Cargo.toml / gradle / package.json) —
# and every in-repo flatc install in .github/workflows/codegen-drift.yml —
# equal the values declared HERE, so a bump can never be applied to only some
# owned surfaces.

# shellcheck disable=SC2034  # sourced by other scripts; not all are used here.
FLATC_PIN_RUST_SWIFT="25.12.19"
# shellcheck disable=SC2034
FLATC_PIN_KOTLIN="25.2.10"
# shellcheck disable=SC2034
FLATC_PIN_TS="25.9.23"
