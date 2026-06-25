package org.nmp.android

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Android peer of Swift `DispatchResult`.
 *
 * The kernel returns exactly one JSON envelope from `nmp_app_dispatch_action`:
 * accepted actions carry `correlation_id`; synchronous rejects carry `error`.
 * Terminal outcomes still arrive later through the snapshot projections.
 */
sealed class DispatchResult {
    abstract val correlationId: String?

    data class Accepted(override val correlationId: String) : DispatchResult()
    data class Failure(val message: String) : DispatchResult() {
        override val correlationId: String? = null
    }

    companion object {
        fun parse(envelope: String): DispatchResult {
            val obj: JsonObject = try {
                Json.parseToJsonElement(envelope).jsonObject
            } catch (_: Exception) {
                return Failure("dispatch envelope was not a JSON object (bytes=${envelope.length})")
            }
            val correlationId = obj["correlation_id"]?.jsonPrimitive?.contentOrNull
            if (!correlationId.isNullOrEmpty()) {
                return Accepted(correlationId)
            }
            val error = obj["error"]?.jsonPrimitive?.contentOrNull
            if (error != null) {
                return Failure(error)
            }
            return Failure("dispatch envelope missing both correlation_id and error (bytes=${envelope.length})")
        }
    }
}
