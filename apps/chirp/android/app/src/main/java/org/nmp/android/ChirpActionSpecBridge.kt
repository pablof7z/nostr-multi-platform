package org.nmp.android

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json

// The `ChirpActionIntent` / `ChirpActionSpec` JSON intent DTOs were RETIRED in
// M14-1 / PR2 (#2145): every social write now goes through a generated
// `GeneratedActionBuilders.*` byte builder dispatched via `bridge.dispatchBytes`,
// with Rust owning all protocol-tag construction (thin-shell rule). What remains
// is the shared `kotlinx.serialization` Json instance still used by the Marmot
// group-action envelopes (`MarmotActions` / `MarmotActionEnvelopes`).

/**
 * Shared JSON codec for the Marmot group-action envelopes. `encodeDefaults =
 * false` + `explicitNulls = false` keep absent/optional fields off the wire so
 * the Rust side sees the exact `(namespace, body_json)` shape it expects.
 */
@OptIn(ExperimentalSerializationApi::class)
internal val chirpActionJson = Json {
    encodeDefaults = false
    explicitNulls = false
    ignoreUnknownKeys = true
}
