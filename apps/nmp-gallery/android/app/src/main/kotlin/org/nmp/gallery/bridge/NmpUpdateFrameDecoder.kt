package org.nmp.gallery.bridge

import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.JsonUnquotedLiteral
import nmp.transport.FrameKind
import nmp.transport.UpdateFrame
import nmp.transport.Value
import nmp.transport.ValueKind

internal const val SCHEMA_VERSION_EXPECTED: UInt = 1u

internal sealed class UpdateFrameDecodeErrorKind {
    object InvalidFlatbuffer : UpdateFrameDecodeErrorKind()
    object InvalidValue : UpdateFrameDecodeErrorKind()
    object MissingSnapshotPayload : UpdateFrameDecodeErrorKind()
    object MissingPanicPayload : UpdateFrameDecodeErrorKind()
    object UnexpectedPanicFrame : UpdateFrameDecodeErrorKind()
    object SchemaVersionMismatch : UpdateFrameDecodeErrorKind()
}

internal class UpdateFrameDecodeException(
    val kind: UpdateFrameDecodeErrorKind,
    message: String,
) : RuntimeException("${kind::class.simpleName}: $message")

@OptIn(ExperimentalSerializationApi::class)
internal object NmpUpdateFrameDecoder {
    fun decodeSnapshot(bytes: ByteArray): JsonObject {
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        if (!UpdateFrame.UpdateFrameBufferHasIdentifier(buffer)) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
                "missing NMPU file identifier",
            )
        }
        val frame = try {
            UpdateFrame.getRootAsUpdateFrame(buffer)
        } catch (e: Throwable) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
                e.message ?: e.javaClass.simpleName,
            )
        }
        when (frame.kind) {
            FrameKind.Snapshot -> {
                // proceed below
            }
            FrameKind.Panic -> {
                val panic = frame.panic ?: throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.MissingPanicPayload,
                    "panic frame missing payload",
                )
                // generated PanicFrame.msg throws AssertionError when the
                // required field is absent; route both branches through the
                // typed kind so call sites can distinguish them.
                val msg = try {
                    panic.msg
                } catch (e: AssertionError) {
                    throw UpdateFrameDecodeException(
                        UpdateFrameDecodeErrorKind.MissingPanicPayload,
                        "panic frame missing msg",
                    )
                }
                throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.UnexpectedPanicFrame,
                    msg,
                )
            }
            else -> throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
                "unknown frame kind ${frame.kind}",
            )
        }
        val snapshot = frame.snapshot ?: throw UpdateFrameDecodeException(
            UpdateFrameDecodeErrorKind.MissingSnapshotPayload,
            "snapshot frame missing snapshot",
        )
        if (snapshot.schemaVersion != SCHEMA_VERSION_EXPECTED) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.SchemaVersionMismatch,
                "frame schema_version=${snapshot.schemaVersion} host=$SCHEMA_VERSION_EXPECTED",
            )
        }
        val payload = snapshot.payload ?: throw UpdateFrameDecodeException(
            UpdateFrameDecodeErrorKind.MissingSnapshotPayload,
            "snapshot frame missing payload",
        )
        val decoded = valueToJson(payload) as? JsonObject ?: throw UpdateFrameDecodeException(
            UpdateFrameDecodeErrorKind.InvalidValue,
            "snapshot payload root is not a map",
        )
        // Mirrors iOS KernelBridge.swift: payload's inner `schema_version` is a
        // typed kernel field and must match the FlatBuffers envelope's field.
        val innerVersion = (decoded["schema_version"] as? JsonPrimitive)?.longOrNullSafe()
        if (innerVersion != null && innerVersion != SCHEMA_VERSION_EXPECTED.toLong()) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.SchemaVersionMismatch,
                "payload schema_version=$innerVersion host=$SCHEMA_VERSION_EXPECTED",
            )
        }
        return decoded
    }

    private fun valueToJson(value: Value): JsonElement =
        when (value.kind) {
            ValueKind.Null -> JsonNull
            ValueKind.Bool -> JsonPrimitive(value.boolValue)
            ValueKind.Int -> JsonPrimitive(value.intValue)
            ValueKind.UInt -> unsignedJsonPrimitive(value.uintValue)
            ValueKind.Float -> {
                val raw = value.floatValue
                if (!raw.isFinite()) {
                    throw UpdateFrameDecodeException(
                        UpdateFrameDecodeErrorKind.InvalidValue,
                        "non-finite float value",
                    )
                }
                JsonPrimitive(raw)
            }
            ValueKind.String -> {
                val s = value.stringValue ?: throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.InvalidValue,
                    "string value missing string_value",
                )
                JsonPrimitive(s)
            }
            ValueKind.List -> JsonArray((0 until value.listLength).map { index ->
                val child = value.list(index) ?: throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.InvalidValue,
                    "list value missing element at index $index",
                )
                valueToJson(child)
            })
            ValueKind.Map -> JsonObject((0 until value.mapLength).associate { index ->
                val pair = value.map(index) ?: throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.InvalidValue,
                    "map value missing pair at index $index",
                )
                // generated Pair.key throws AssertionError when the required
                // field is absent; convert to a typed error so callers can react.
                val key = try {
                    pair.key
                } catch (e: AssertionError) {
                    throw UpdateFrameDecodeException(
                        UpdateFrameDecodeErrorKind.InvalidValue,
                        "map pair at index $index missing key",
                    )
                }
                val child = pair.value ?: throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.InvalidValue,
                    "map pair at index $index missing value",
                )
                key to valueToJson(child)
            })
            else -> throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidValue,
                "unknown value kind ${value.kind}",
            )
        }

    // Preserve full u64 precision: values that don't fit in Long are emitted as
    // unquoted JSON integer literals via JsonUnquotedLiteral instead of being
    // clamped to Long.MAX_VALUE.
    private fun unsignedJsonPrimitive(value: ULong): JsonElement =
        if (value <= Long.MAX_VALUE.toULong()) {
            JsonPrimitive(value.toLong())
        } else {
            JsonUnquotedLiteral(value.toString())
        }

    private fun JsonPrimitive.longOrNullSafe(): Long? = content.toLongOrNull()
}
