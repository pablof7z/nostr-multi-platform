package org.nmp.gallery.bridge

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

internal const val SCHEMA_VERSION_EXPECTED: UInt = 1u

internal sealed class UpdateFrameDecodeErrorKind {
    object InvalidFlatbuffer : UpdateFrameDecodeErrorKind()
    object InvalidValue : UpdateFrameDecodeErrorKind()
    object SchemaVersionMismatch : UpdateFrameDecodeErrorKind()
}

internal class UpdateFrameDecodeException(
    val kind: UpdateFrameDecodeErrorKind,
    message: String,
) : RuntimeException("${kind::class.simpleName}: $message")

internal object NmpUpdateFrameDecoder {
    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    fun decodeSnapshot(
        bytes: ByteArray,
        // ADR-0070 (#1671): the JSON provider is now session-scoped (it merges
        // refs.profile into the native session store), so callers MUST pass
        // `bridge::decodeSnapshotJson`. No static default — the decode cannot be
        // sessionless. Tests pass an in-memory provider.
        snapshotJsonProvider: (ByteArray) -> String?,
    ): JsonObject {
        val raw = snapshotJsonProvider(bytes) ?: throw UpdateFrameDecodeException(
            UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
            "snapshot frame did not decode",
        )
        val decoded = try {
            json.parseToJsonElement(raw) as? JsonObject
        } catch (e: Throwable) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidValue,
                e.message ?: e.javaClass.simpleName,
            )
        } ?: throw UpdateFrameDecodeException(
            UpdateFrameDecodeErrorKind.InvalidValue,
            "snapshot JSON root is not an object",
        )
        val innerVersion = (decoded["schema_version"] as? JsonPrimitive)?.longOrNullSafe()
        if (innerVersion != null && innerVersion != SCHEMA_VERSION_EXPECTED.toLong()) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.SchemaVersionMismatch,
                "payload schema_version=$innerVersion host=$SCHEMA_VERSION_EXPECTED",
            )
        }
        return decoded
    }

    private fun JsonPrimitive.longOrNullSafe(): Long? = content.toLongOrNull()
}
