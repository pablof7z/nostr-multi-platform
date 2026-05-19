import UIKit

// MARK: - Haptics

/// Centralized haptic-feedback API.
///
/// ## Primitive one-shots
/// `selection`, `light`, `medium`, `soft`, `success`, `warning`, `error` —
/// thin wrappers around UIKit generators; use these when none of the
/// semantic patterns below fit.
///
/// ## Semantic patterns
/// Named patterns encode *meaning*, not just intensity.  Each pattern uses
/// graded intensity or a short two-beat sequence so the user can distinguish
/// "item completed" from "priority toggled" from "undo triggered" by feel
/// alone, without looking at the screen.
///
/// | Pattern            | Feel                                    | Use when…                        |
/// |--------------------|-----------------------------------------|----------------------------------|
/// | `itemComplete`     | medium + 120 ms + light (celebration)  | marking an item done             |
/// | `itemReopen`       | light + 120 ms + medium (reversal)     | reverting a done item to pending |
/// | `priorityOn`       | rigid, full-intensity single beat       | enabling priority on an item     |
/// | `priorityOff`      | soft, half-intensity single beat        | removing priority from an item   |
/// | `undo`             | light + 100 ms + medium (backwards)    | tapping Undo                     |
/// | `delete`           | rigid, high-intensity single beat       | deleting an item                 |
/// | `bulkAction`       | medium + 80 ms + medium (double-tap)   | completing/deleting in batch     |
@MainActor
enum Haptics {

    // MARK: - Primitive one-shots

    static func selection() {
        UISelectionFeedbackGenerator().selectionChanged()
    }

    static func light() {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    static func medium() {
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    static func heavy() {
        UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
    }

    static func soft() {
        UIImpactFeedbackGenerator(style: .soft).impactOccurred()
    }

    static func success() {
        UINotificationFeedbackGenerator().notificationOccurred(.success)
    }

    static func warning() {
        UINotificationFeedbackGenerator().notificationOccurred(.warning)
    }

    static func error() {
        UINotificationFeedbackGenerator().notificationOccurred(.error)
    }

    // MARK: - Semantic patterns

    /// Two-beat celebration pattern for marking an item done.
    ///
    /// Medium impact fires immediately (the decisive completion), followed by a
    /// lighter echo 120 ms later (the satisfying "settle").  The asymmetry
    /// (strong then soft) signals forward progress and finality.
    static func itemComplete() {
        let first = UIImpactFeedbackGenerator(style: .medium)
        first.prepare()
        first.impactOccurred(intensity: 0.85)
        Task {
            try? await Task.sleep(for: AppTheme.Timing.hapticTwoBeat)
            UIImpactFeedbackGenerator(style: .light).impactOccurred(intensity: 0.55)
        }
    }

    /// Two-beat reversal pattern for re-opening a done item.
    ///
    /// Soft then medium mirrors `itemComplete` in reverse, giving a distinct
    /// "unwinding" sensation — the user can tell they're moving backwards.
    static func itemReopen() {
        let first = UIImpactFeedbackGenerator(style: .soft)
        first.prepare()
        first.impactOccurred(intensity: 0.50)
        Task {
            try? await Task.sleep(for: AppTheme.Timing.hapticTwoBeat)
            UIImpactFeedbackGenerator(style: .medium).impactOccurred(intensity: 0.70)
        }
    }

    /// Crisp, rigid single beat for enabling priority.
    ///
    /// Full-intensity rigid impact conveys weight and urgency — the item now
    /// "matters more" and the feedback reflects that added importance.
    static func priorityOn() {
        let gen = UIImpactFeedbackGenerator(style: .rigid)
        gen.prepare()
        gen.impactOccurred(intensity: 1.0)
    }

    /// Soft, subdued single beat for removing priority.
    ///
    /// Half-intensity soft impact conveys release — the item has been deprioritized
    /// and the lighter touch reinforces that reduced weight.
    static func priorityOff() {
        let gen = UIImpactFeedbackGenerator(style: .soft)
        gen.prepare()
        gen.impactOccurred(intensity: 0.45)
    }

    /// Two-beat reversal pattern for triggering Undo.
    ///
    /// Light then medium is the mirror image of `itemComplete`; users learn that
    /// "light first" means going backwards, making Undo feel kinesthetically
    /// distinct from completion.
    static func undo() {
        let first = UIImpactFeedbackGenerator(style: .light)
        first.prepare()
        first.impactOccurred(intensity: 0.60)
        Task {
            try? await Task.sleep(for: AppTheme.Timing.hapticUndo)
            UIImpactFeedbackGenerator(style: .medium).impactOccurred(intensity: 0.80)
        }
    }

    /// Firm, definitive single beat for deleting an item.
    ///
    /// High-intensity rigid impact emphasises permanence.  Stronger than
    /// `priorityOn` so deletion is never confused with a priority change.
    static func delete() {
        let gen = UIImpactFeedbackGenerator(style: .rigid)
        gen.prepare()
        gen.impactOccurred(intensity: 0.90)
    }

    /// Double medium beat for batch operations affecting multiple items.
    ///
    /// The evenly-spaced double-tap communicates "this affected more than one
    /// thing" without overwhelming the user with a long pattern.
    static func bulkAction() {
        let gen = UIImpactFeedbackGenerator(style: .medium)
        gen.prepare()
        gen.impactOccurred(intensity: 0.80)
        Task {
            try? await Task.sleep(for: AppTheme.Timing.hapticBulk)
            UIImpactFeedbackGenerator(style: .medium).impactOccurred(intensity: 0.80)
        }
    }
}
