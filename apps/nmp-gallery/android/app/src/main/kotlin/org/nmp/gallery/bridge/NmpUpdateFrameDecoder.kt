package org.nmp.gallery.bridge

import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import nmp.transport.FrameKind
import nmp.transport.UpdateFrame
import nmp.transport.Value
import nmp.transport.ValueKind

internal object NmpUpdateFrameDecoder {
    fun decodeSnapshot(bytes: ByteArray): JsonObject? {
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        if (!UpdateFrame.UpdateFrameBufferHasIdentifier(buffer)) return null
        val frame = UpdateFrame.getRootAsUpdateFrame(buffer)
        if (frame.kind != FrameKind.Snapshot) return null
        val payload = frame.snapshot?.payload ?: return null
        return valueToJson(payload) as? JsonObject
    }

    private fun valueToJson(value: Value): JsonElement =
        when (value.kind) {
            ValueKind.Null -> JsonNull
            ValueKind.Bool -> JsonPrimitive(value.boolValue)
            ValueKind.Int -> JsonPrimitive(value.intValue)
            ValueKind.UInt -> JsonPrimitive(unsignedJsonNumber(value.uintValue))
            ValueKind.Float -> JsonPrimitive(value.floatValue)
            ValueKind.String -> JsonPrimitive(value.stringValue ?: "")
            ValueKind.List -> JsonArray((0 until value.listLength).mapNotNull { index ->
                value.list(index)?.let(::valueToJson)
            })
            ValueKind.Map -> JsonObject((0 until value.mapLength).mapNotNull { index ->
                val pair = value.map(index) ?: return@mapNotNull null
                val child = pair.value ?: return@mapNotNull null
                pair.key to valueToJson(child)
            }.toMap())
            else -> JsonNull
        }

    private fun unsignedJsonNumber(value: ULong): Long =
        if (value <= Long.MAX_VALUE.toULong()) value.toLong() else Long.MAX_VALUE
}
