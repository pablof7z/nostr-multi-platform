import SwiftUI

// MARK: - LiquidGlassSegmentedPicker

/// Custom Liquid Glass segmented control matching iOS 26's design language.
///
/// Drop-in replacement for `Picker(...).pickerStyle(.segmented)`. Each
/// segment is a tappable `Button` rendered inside a `GlassEffectContainer`
/// so the active pill morphs between segments with a subtle spring rather
/// than the abrupt fill swap of the system control.
///
/// ### Visual treatment
/// - **Active segment** — `.regular.tint(accent.opacity(0.30)).interactive()`
///   in a capsule shape, plus a `glassEffectID` tied to the namespace so the
///   morph reads as a single sliding pill.
/// - **Inactive segments** — no glass; muted secondary label so the active
///   pill stays the only emphasised element.
/// - **Container** — `GlassEffectContainer(spacing: 6)` + a thin capsule
///   background so the control reads as a single surface against editorial
///   brass-amber sheets and translucent toolbars alike.
///
/// ### Accessibility
/// Each segment exposes `.isButton` plus `.isSelected` (active) so VoiceOver
/// announces "<label>, selected" / "<label>, button". Selection emits
/// `Haptics.selection()` only on actual change.
///
/// ### Generic constraint
/// The spec calls for `Hashable & Identifiable`. We relax to plain
/// `Hashable` so callers like `WikiGenerateSheet.ScopeChoice` and
/// `AgentAccessControlView.AccessTab` (which conform to `Hashable` only)
/// don't need a one-line conformance shim. Identity is derived from the
/// value itself via `\.self`, which is exactly what `Identifiable` would
/// produce for a value-type enum anyway.
struct LiquidGlassSegmentedPicker<Value: Hashable, Label: View>: View {

    // MARK: - Inputs

    /// Accessibility name for the whole control (e.g. "Add show source").
    let accessibilityTitle: String

    /// Bound selection.
    @Binding var selection: Value

    /// Ordered segments rendered left to right.
    let values: [Value]

    /// Builder that turns a value + selected flag into a label view. The
    /// flag is passed so callers can vary weight or colour for the active
    /// segment if they need to; most callers ignore it.
    let label: (Value, Bool) -> Label

    /// String label used for VoiceOver — required because SwiftUI can't
    /// reliably extract text from an arbitrary `@ViewBuilder` label, and
    /// segment controls *must* announce their option name.
    let accessibilityLabel: (Value) -> String

    // MARK: - Internal state

    @Namespace private var namespace

    // MARK: - Body

    var body: some View {
        GlassEffectContainer(spacing: 6) {
            HStack(spacing: 4) {
                ForEach(values, id: \.self) { value in
                    segmentButton(for: value)
                }
            }
            .padding(.horizontal, 4)
            .padding(.vertical, 4)
        }
        .background(
            Capsule(style: .continuous)
                .fill(AppTheme.Tint.surfaceMuted)
        )
        .clipShape(Capsule(style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(accessibilityTitle)
    }

    // MARK: - Segment

    @ViewBuilder
    private func segmentButton(for value: Value) -> some View {
        let isSelected = selection == value

        Button {
            guard selection != value else { return }
            withAnimation(AppTheme.Animation.spring) {
                selection = value
            }
            Haptics.selection()
        } label: {
            label(value, isSelected)
                .font(AppTheme.Typography.subheadline.weight(isSelected ? .semibold : .regular))
                .foregroundStyle(isSelected ? Color.primary : Color.secondary)
                .frame(maxWidth: .infinity)
                .padding(.vertical, AppTheme.Spacing.sm)
                .padding(.horizontal, AppTheme.Spacing.sm)
                .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .modifier(SegmentGlassModifier(isSelected: isSelected, namespace: namespace))
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
        .accessibilityLabel(accessibilityLabel(value))
    }
}

// MARK: - Active-state glass modifier

/// Applies the active glass pill only to the selected segment. Inactive
/// segments get no glass so the active pill is the visual focus.
private struct SegmentGlassModifier: ViewModifier {
    let isSelected: Bool
    let namespace: Namespace.ID

    func body(content: Content) -> some View {
        if isSelected {
            content
                .glassEffect(
                    .regular.tint(Color.accentColor.opacity(0.30)).interactive(),
                    in: .capsule
                )
                .glassEffectID("liquid-glass-segment-active", in: namespace)
        } else {
            content
        }
    }
}

// MARK: - Convenience initialisers

extension LiquidGlassSegmentedPicker where Label == Text {

    /// Convenience initialiser for the common `[(value, "Label")]` case.
    init(
        _ accessibilityTitle: String,
        selection: Binding<Value>,
        segments: [(value: Value, label: String)]
    ) {
        self.accessibilityTitle = accessibilityTitle
        self._selection = selection
        self.values = segments.map(\.value)
        let lookup = Dictionary(uniqueKeysWithValues: segments.map { ($0.value, $0.label) })
        self.label = { value, _ in Text(lookup[value] ?? "") }
        self.accessibilityLabel = { value in lookup[value] ?? "" }
    }
}

extension LiquidGlassSegmentedPicker {

    /// Designated `@ViewBuilder` initialiser for cases where segment
    /// content is conditional (e.g. `EpisodeDetailView` only shows the
    /// "Along" segment when a transcript exists).
    init(
        _ accessibilityTitle: String,
        selection: Binding<Value>,
        values: [Value],
        accessibilityLabel: @escaping (Value) -> String,
        @ViewBuilder label: @escaping (Value, Bool) -> Label
    ) {
        self.accessibilityTitle = accessibilityTitle
        self._selection = selection
        self.values = values
        self.label = label
        self.accessibilityLabel = accessibilityLabel
    }
}

// MARK: - Preview

#Preview("Light") {
    PreviewHarness()
        .padding()
        .preferredColorScheme(.light)
}

#Preview("Dark") {
    PreviewHarness()
        .padding()
        .preferredColorScheme(.dark)
}

private struct PreviewHarness: View {
    enum Mode: String, CaseIterable, Hashable {
        case search = "Search"
        case url = "From URL"
        case opml = "OPML"
    }

    @State private var mode: Mode = .search

    var body: some View {
        LiquidGlassSegmentedPicker(
            "Add show source",
            selection: $mode,
            segments: Mode.allCases.map { ($0, $0.rawValue) }
        )
    }
}
