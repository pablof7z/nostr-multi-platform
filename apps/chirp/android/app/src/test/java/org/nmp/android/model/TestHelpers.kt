package org.nmp.android.model

import kotlinx.serialization.json.Json

/**
 * Test-only JSON instance with snake_case and lenient defaults.
 *
 * Used by [OpFeedDecoderTest.genericModelDeserializesOpCentricShape] to
 * verify the `@Serializable` + `@SerialName` annotations on [ChirpOpFeedSnapshot]
 * and its transitive types decode correctly from Rust serde JSON output.
 */
fun testJson(): Json = Json {
    ignoreUnknownKeys = true
    isLenient = true
}
