import SwiftUI

/// Minimal note composer. Publishes through the single generic
/// `nmp_app_dispatch_action("nmp.publish", …)` door — identical to Chirp's
/// path, proving the action surface is generic, not Chirp-specific.
struct ComposeView: View {
    let bridge: NotesBridge
    @State private var text = ""
    @State private var didSubmit = false
    private let maxLen = 280

    var body: some View {
        NavigationStack {
            VStack(spacing: 12) {
                TextEditor(text: $text)
                    .padding(8).background(Color(.secondarySystemBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .overlay(alignment: .bottomTrailing) {
                        Text("\(text.count)/\(maxLen)").font(.caption2)
                            .foregroundStyle(text.count > maxLen ? .red : .secondary).padding(8)
                    }
                Button("Publish") { bridge.publishNote(text); text = ""; didSubmit = true }
                    .buttonStyle(.borderedProminent)
                    .disabled(text.isEmpty || text.count > maxLen)
            }
            .padding().navigationTitle("Compose")
            .overlay(alignment: .bottom) {
                if didSubmit {
                    Text("Submitted").font(.callout)
                        .padding(.horizontal, 16).padding(.vertical, 8)
                        .background(.thinMaterial, in: Capsule()).padding(.bottom, 24)
                        .task {
                            try? await Task.sleep(for: .seconds(1.5))
                            didSubmit = false
                        }
                }
            }
        }
    }
}
