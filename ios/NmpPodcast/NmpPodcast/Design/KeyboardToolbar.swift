import SwiftUI

// MARK: - View extension

extension View {
    /// Adds a keyboard toolbar with a "Done" button that dismisses the first responder.
    func dismissKeyboardToolbar() -> some View {
        modifier(DismissKeyboardToolbar())
    }
}

// MARK: - ViewModifier

/// A `ViewModifier` that injects a keyboard toolbar containing a semibold "Done" button
/// that resigns the first responder when tapped.
struct DismissKeyboardToolbar: ViewModifier {
    func body(content: Content) -> some View {
        content.toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("Done") {
                    UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
                }
                .fontWeight(.semibold)
            }
        }
    }
}
