// Generated — do not edit by hand.
// Source of truth: apps/podcast/nmp-app-podcast/src/ffi/snapshot.rs
// Regenerate when the Rust snapshot shape changes.
//
// At M0.A this struct mirrors the stub payload:
//   {"running":true,"rev":0,"schema_version":1}
// Real podcast-specific fields land here in later milestones.

import Foundation

struct PodcastUpdate: Decodable, Equatable {
    var running: Bool = false
    var rev: UInt64 = 0
    var schemaVersion: UInt32 = 0
    var lastErrorToast: String? = nil
}
