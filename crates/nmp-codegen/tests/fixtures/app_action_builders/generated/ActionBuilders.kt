// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform kotlin \
//       --out apps/chirp/android/app/src/main/java/org/nmp/android/ActionBuilders.kt
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated Kotlin differs from a fresh run.
//
// ADR-0064 §3 — typed write builders. Each function below encodes the per-crate
// FlatBuffers payload for one open-registry `action_namespace` and stamps it,
// the namespace, and the envelope schema_version into a `DispatchEnvelope`,
// returning the finished bytes for the native byte doorway
// `nmp_app_dispatch_action_bytes` (#1752). App code NEVER spells a namespace
// string or hand-assembles FlatBuffers — that lives only here, in generated
// code. The host supplies the `correlationId` (the operation identity end to
// end, ADR-0064 §4) and owns the JNI call.
// ─────────────────────────────────────────────────────────────────────────────

package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder

object GeneratedActionBuilders {
    enum class PublishSignerProvenance(val token: String) {
        APP_MANAGED("app_managed"),
        USER_SELECTED("user_selected"),
        PROTOCOL_PINNED("protocol_pinned"),
        DIAGNOSTIC("diagnostic"),
    }

    sealed class PublishSignerSelection {
        object Active : PublishSignerSelection()
        data class Registered(
            val pubkey: String,
            val provenance: PublishSignerProvenance = PublishSignerProvenance.APP_MANAGED,
        ) : PublishSignerSelection()
    }

    enum class PublishRouteClass(val token: String) {
        MANUAL_OVERRIDE("manual_override"),
        GROUP_HOST_PIN("group_host_pin"),
        VERIFIED_PRIVATE_INBOX("verified_private_inbox"),
        IMPORTED_OR_PRESIGNED("imported_or_presigned"),
        DIAGNOSTIC("diagnostic"),
    }

    sealed class PublishTargetSelection {
        object Auto : PublishTargetSelection()
        data class Explicit(
            val relays: List<String>,
            val routeClass: PublishRouteClass,
        ) : PublishTargetSelection()
    }

    /// The single recognised envelope schema version — mirrors
    /// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`.
    const val DISPATCH_ENVELOPE_SCHEMA_VERSION: Int = 1

    /// Stamp `(correlationId, actionNamespace, schemaVersion, payload)` into a
    /// `DispatchEnvelope` and return the finished bytes (file identifier `NMPD`).
    /// The byte-for-byte twin of `encode_dispatch_envelope` in `nmp-core`.
    private fun encodeDispatchEnvelope(
        correlationId: String,
        actionNamespace: String,
        payload: ByteArray,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val correlationOffset = fbb.createString(correlationId)
        val namespaceOffset = fbb.createString(actionNamespace)
        val payloadOffset = fbb.createByteVector(payload)
        fbb.startTable(4)
        fbb.addOffset(0, correlationOffset, 0)   // slot 0: correlation_id
        fbb.addOffset(1, namespaceOffset, 0)     // slot 1: action_namespace
        fbb.addInt(2, DISPATCH_ENVELOPE_SCHEMA_VERSION, 0) // slot 2: schema_version
        fbb.addOffset(3, payloadOffset, 0)       // slot 3: payload
        val root = fbb.endTable()
        fbb.finish(root, "NMPD")
        return fbb.sizedByteArray()
    }

    /// Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),
    /// mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
    /// Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)
    /// so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.
    /// Role strings may be comma-separated (e.g. `"both,indexer"`); comparisons are case-insensitive.
    private fun relayMarkerByte(role: String): Byte {
        var hasBoth = false; var hasRead = false; var hasWrite = false; var hasIndexer = false
        var invalid = false
        for (part in role.split(",").map { it.trim().lowercase() }) {
            when (part) {
                "" -> {}
                "both" -> hasBoth = true
                "read" -> hasRead = true
                "write" -> hasWrite = true
                "indexer" -> hasIndexer = true
                else -> invalid = true
            }
        }
        if (invalid) return 255.toByte()
        return (when {
            hasBoth || (hasRead && hasWrite) -> 0
            hasRead -> 1
            hasWrite -> 2
            hasIndexer -> 3
            else -> 255
        }).toByte()
    }

    /// Publish an app-private note event.
    /// Builds the `app.notes.publish_note` `DispatchEnvelope` bytes for the byte doorway.
    fun publishNote(
        correlationId: String,
        title: String,
        retryCount: Int,
        topics: List<String>?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val titleOffset = fbb.createString(title)
        val topicsOffset = run {
            val values = topics
            if (values == null || values.isEmpty()) 0 else {
                val offsets = IntArray(values.size) { i -> fbb.createString(values[i]) }
                fbb.startVector(4, offsets.size, 4)
                for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
                fbb.endVector()
            }
        }
        fbb.startTable(4)
        fbb.addInt(0, 42, 0) // slot 0: schema_version
        fbb.addOffset(1, titleOffset, 0) // slot 1: title
        fbb.addInt(2, retryCount, 0) // slot 2: retryCount
        if (topicsOffset != 0) fbb.addOffset(3, topicsOffset, 0) // slot 3: topics
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "APPA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "app.notes.publish_note",
            payload = payload,
        )
    }
}
