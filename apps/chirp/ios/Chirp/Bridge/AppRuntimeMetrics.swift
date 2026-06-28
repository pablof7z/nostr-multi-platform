import Foundation

// ─── Swift-side timing accumulator ───────────────────────────────────────

struct AppRuntimeMetrics {
    private(set) var updatesApplied = 0
    private(set) var lastDecodeMicros = 0
    private(set) var lastCallbackToApplyMicros = 0
    private(set) var lastApplyMicros = 0
    private(set) var lastCallbackToAppliedMicros = 0
    private(set) var maxDecodeMicros = 0
    private(set) var maxCallbackToApplyMicros = 0
    private(set) var maxApplyMicros = 0
    private(set) var maxCallbackToAppliedMicros = 0
    private(set) var lastPayloadBytes = 0

    #if DEBUG
    // ── Reliability instrumentation (debug-only) ─────────────────────────
    // These counters exist purely to quantify the profile-name flicker
    // defect and the typed-decode reliability of the snapshot pipeline.
    // They are NOT shipped to users (`#if DEBUG`) and feed no production
    // view — they are read by tests and `os_signpost` diagnostics only.

    /// A2: Name-regression counter — how many times a pubkey that should
    /// resolve to a real name had no claimed/resolved profile on the next
    /// accessor read. First-load misses and repeated reads during the same
    /// outage are excluded. See `KernelModel.profile(forPubkey:)`.
    private(set) var nameRegressionCount: Int = 0

    /// B1: Typed-decode tick counters. `typedHomeFeed` is the ADR-0038
    /// typed NOFS+NFCT decode; a nil result on a tick means the generic
    /// `projections.homeFeed` fallback was used instead.
    private(set) var typedDecodeSuccessCount: UInt64 = 0
    private(set) var typedDecodeFailCount: UInt64 = 0

    var typedDecodeSuccessRate: Double {
        let total = typedDecodeSuccessCount + typedDecodeFailCount
        guard total > 0 else { return 1.0 }
        return Double(typedDecodeSuccessCount) / Double(total)
    }

    /// B2: Empty-after-nonempty counter — the timeline went from a
    /// populated set of items to empty across a single tick, a strong
    /// signal of a projection churn / wipe defect.
    private(set) var emptyAfterNonEmptyCount: Int = 0

    mutating func recordNameRegression() {
        nameRegressionCount += 1
    }

    mutating func recordTypedDecode(success: Bool) {
        if success {
            typedDecodeSuccessCount &+= 1
        } else {
            typedDecodeFailCount &+= 1
        }
    }

    mutating func recordEmptyAfterNonEmpty() {
        emptyAfterNonEmptyCount += 1
    }
    #endif

    mutating func record(
        decodeMicros: Int,
        callbackToApplyMicros: Int,
        applyMicros: Int,
        callbackToAppliedMicros: Int,
        payloadBytes: Int
    ) {
        updatesApplied += 1
        lastDecodeMicros = decodeMicros
        lastCallbackToApplyMicros = callbackToApplyMicros
        lastApplyMicros = applyMicros
        lastCallbackToAppliedMicros = callbackToAppliedMicros
        maxDecodeMicros = max(maxDecodeMicros, decodeMicros)
        maxCallbackToApplyMicros = max(maxCallbackToApplyMicros, callbackToApplyMicros)
        maxApplyMicros = max(maxApplyMicros, applyMicros)
        maxCallbackToAppliedMicros = max(maxCallbackToAppliedMicros, callbackToAppliedMicros)
        lastPayloadBytes = payloadBytes
    }
}
