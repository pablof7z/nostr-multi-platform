import Foundation

// MARK: - UUID + Identifiable

/// Makes `UUID` conform to `Identifiable` (using itself as its own id) so
/// that UUID-valued `@State` variables can drive `.sheet(item:)` directly —
/// without the boilerplate `IdentifiedXxx` wrapper struct that each call site
/// used to define privately.
extension UUID: @retroactive Identifiable {
    public var id: UUID { self }
}
