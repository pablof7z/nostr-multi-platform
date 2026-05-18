import Foundation

// MARK: - OnboardingStep

/// Typed step list for the onboarding flow. Values intentionally do *not* use
/// raw integer ordering — `next` / `previous` walk an explicit ordered array
/// so reordering or inserting steps later requires only an array edit, not
/// renumbering raw values.
///
/// Source of truth lives here so `OnboardingView` stays under the 300-line
/// soft limit and so per-step logic in handler extensions can read the same
/// constants.
enum OnboardingStep: Hashable {
    case welcome
    case aiSetup
    case identity
    case subscribe
    case ready

    /// Canonical order. Source of truth for `next`, `previous`, and `isLast`.
    /// Order is semantic, not alphabetical.
    static let order: [OnboardingStep] = [
        .welcome, .aiSetup, .identity, .subscribe, .ready
    ]

    var next: OnboardingStep? {
        guard let i = Self.order.firstIndex(of: self), i + 1 < Self.order.count else { return nil }
        return Self.order[i + 1]
    }

    var previous: OnboardingStep? {
        guard let i = Self.order.firstIndex(of: self), i > 0 else { return nil }
        return Self.order[i - 1]
    }

    var isLast: Bool { next == nil }

    /// Steps that show a "Skip" affordance in the top bar. The welcome page
    /// has nothing to skip; the ready page is the destination, not a step.
    var allowsSkip: Bool {
        switch self {
        case .welcome, .ready: return false
        case .aiSetup, .identity, .subscribe: return true
        }
    }
}
