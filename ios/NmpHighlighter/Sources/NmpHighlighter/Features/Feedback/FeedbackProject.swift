import Foundation

/// Constants for the Highlighter feedback project — the kind:31933 event the
/// shake-to-share surface scopes itself to. The project's first `p` tag (the
/// active agent) is fetched at compose time via `getProjectFirstAgentPubkey`.
enum FeedbackProject {
    static let coordinate =
        "31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:highlighter"
}
