import SwiftUI

/// A `ButtonStyle` that provides tactile press feedback via scale and opacity.
/// Useful for tappable elements that aren't wrapped in a glass effect.
///
/// Usage:
/// ```swift
/// Button("Tap me") { action() }
///     .buttonStyle(.pressable)
///
/// // Custom scale:
/// Button(...) { ... }
///     .buttonStyle(.pressable(scale: 0.92))
/// ```
struct PressableStyle: ButtonStyle {
    var scale: CGFloat = 0.96
    var pressedOpacity: CGFloat = 0.80

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? scale : 1)
            .opacity(configuration.isPressed ? pressedOpacity : 1)
            .animation(AppTheme.Animation.springFast, value: configuration.isPressed)
    }
}

extension ButtonStyle where Self == PressableStyle {
    static var pressable: PressableStyle { PressableStyle() }
    static func pressable(scale: CGFloat = 0.96, opacity: CGFloat = 0.80) -> PressableStyle {
        PressableStyle(scale: scale, pressedOpacity: opacity)
    }
}

// MARK: - Inline error text modifier

extension View {
    /// Styles text as an inline error message on a standard (light-tinted) background.
    ///
    /// Applies `AppTheme.Typography.caption` font and `AppTheme.Tint.error` foreground colour.
    /// Add transitions, padding, or frame alignment modifiers at the call site as needed —
    /// those vary per context and are intentionally not bundled here.
    ///
    /// Example:
    /// ```swift
    /// if let errorMessage {
    ///     Text(errorMessage)
    ///         .inlineErrorText()
    ///         .transition(.opacity.combined(with: .move(edge: .top)))
    /// }
    /// ```
    func inlineErrorText() -> some View {
        self
            .font(AppTheme.Typography.caption)
            .foregroundStyle(AppTheme.Tint.error)
    }
}

// MARK: - Copy context-menu helper

extension View {
    /// Attaches a "Copy" context-menu action that writes `text` to the system
    /// pasteboard and fires a selection haptic.
    ///
    /// Use this on any view that displays text the user might want to copy.
    /// For menus that need additional actions (e.g. delete), build the
    /// `.contextMenu` directly so you can include all required buttons.
    func copyableTextMenu(_ text: String) -> some View {
        contextMenu {
            Button {
                UIPasteboard.general.string = text
                Haptics.selection()
            } label: {
                Label("Copy", systemImage: "doc.on.doc")
            }
        }
    }
}

// MARK: - Agent content row actions modifier

extension View {
    /// Attaches the standard Edit / Copy / Delete context-menu and swipe actions
    /// used by agent content rows (memories, notes).
    ///
    /// Mirrors `copyableTextMenu`, but adds Edit (leading swipe) and Delete
    /// (trailing swipe + context menu) alongside the copy action.
    ///
    /// - Parameters:
    ///   - onEdit: Called when the user selects Edit (context menu or leading swipe).
    ///   - copyText: The string written to the pasteboard when the user selects Copy.
    ///   - onDelete: Called when the user selects Delete (context menu or trailing swipe).
    func agentContentRowActions(
        onEdit: @escaping () -> Void,
        copyText: String,
        onDelete: @escaping () -> Void
    ) -> some View {
        self
            // Tap = open the edit sheet. Without this, the only path into
            // editing a note/memory was long-press → context menu → Edit
            // (or leading-edge swipe → Edit). Tapping a typed-content row
            // and getting nothing is a paper-cut — the iOS-default user
            // expectation for any list of writable items is tap-to-open.
            // SwiftUI's `.onTapGesture` cooperates with `.contextMenu`
            // (long-press) and `.swipeActions` (horizontal drags), so the
            // existing affordances stay intact.
            .contentShape(Rectangle())
            .onTapGesture {
                Haptics.selection()
                onEdit()
            }
            .contextMenu {
                Button { onEdit() } label: {
                    Label("Edit", systemImage: "pencil")
                }
                Button {
                    UIPasteboard.general.string = copyText
                    Haptics.selection()
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                Button(role: .destructive) { onDelete() } label: {
                    Label("Delete", systemImage: "trash")
                }
            }
            .swipeActions(edge: .leading, allowsFullSwipe: false) {
                Button { onEdit() } label: {
                    Label("Edit", systemImage: "pencil")
                }
                .tint(.blue)
            }
            // `allowsFullSwipe: false` so the user has to swipe AND tap
            // the Delete button — instead of triggering on a full-edge
            // swipe alone. Notes/Memories are user-typed content with no
            // recovery UI ("Recently Deleted" doesn't exist), and the
            // soft-delete (`deleted: True`) is invisible from the app —
            // an accidental full-swipe would silently lose the row with
            // no Undo. The extra tap-to-confirm is cheap and matches the
            // pattern Apple Notes uses on its main list.
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                Button(role: .destructive) {
                    onDelete()
                    Haptics.delete()
                } label: {
                    Label("Delete", systemImage: "trash")
                }
            }
    }
}

// MARK: - Copy-to-clipboard helper

/// Copies `text` to the system pasteboard, fires `haptic`, sets `isCopied` to
/// `true`, waits `AppTheme.Timing.copyFeedback`, then resets `isCopied` to `false`.
///
/// All sites that show a transient "Copied!" badge should call this instead of
/// duplicating the Task/sleep/MainActor dance inline.
///
/// Example:
/// ```swift
/// Button { copyToClipboard(key, isCopied: $showCopied) } label: { ... }
/// ```
@MainActor
func copyToClipboard(_ text: String, isCopied: Binding<Bool>, haptic: @MainActor () -> Void = { Haptics.selection() }) {
    UIPasteboard.general.string = text
    haptic()
    isCopied.wrappedValue = true
    Task {
        try? await Task.sleep(for: AppTheme.Timing.copyFeedback)
        isCopied.wrappedValue = false
    }
}
