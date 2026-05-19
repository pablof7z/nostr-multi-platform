import SwiftUI

// MARK: - AsyncButton

/// A button that runs an `async throws` action, showing a `ProgressView` while
/// in-flight and disabling itself until the task completes.
///
/// Usage:
/// ```swift
/// AsyncButton {
///     try await someAsyncWork()
/// } label: {
///     Text("Save")
/// }
///
/// AsyncButton(action: { try await publish() }, onError: { error in … }) {
///     Label("Send", systemImage: "paperplane.fill")
/// }
/// ```
struct AsyncButton<Label: View>: View {

    let action: () async throws -> Void
    let onError: ((Error) -> Void)?
    @ViewBuilder let label: () -> Label

    @State private var isRunning = false

    init(
        action: @escaping () async throws -> Void,
        onError: ((Error) -> Void)? = nil,
        @ViewBuilder label: @escaping () -> Label
    ) {
        self.action = action
        self.onError = onError
        self.label = label
    }

    var body: some View {
        Button {
            guard !isRunning else { return }
            isRunning = true
            Task {
                do {
                    try await action()
                } catch {
                    onError?(error)
                }
                isRunning = false
            }
        } label: {
            if isRunning {
                ProgressView()
            } else {
                label()
            }
        }
        .disabled(isRunning)
    }
}
