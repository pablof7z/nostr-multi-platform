import SwiftUI

// Go-to / search box. The user pastes or types ONE string; Rust classifies it
// (thin-shell rule — no parsing in Swift) and this sheet routes to the matching
// destination. Paste-to-navigate (npub/nprofile/note/nevent) and #hashtag feeds
// are wired today; NIP-05 lookup and NIP-50 full-text search are recognized but
// not yet wired, so they surface a "coming soon" notice instead of navigating.
//
// Navigation is deferred to the presenter: a routable classification calls
// `onNavigate` then dismisses, and `HomeFeedView` pushes the route in the
// sheet's `onDismiss` to avoid a push-during-dismiss race.
struct SearchSheet: View {
    let onNavigate: (ChirpRoute) -> Void

    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var text = ""
    @State private var notice: String?
    @FocusState private var fieldFocused: Bool

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 16) {
                TextField(
                    "npub, nevent, #hashtag, name@domain, search…",
                    text: $text
                )
                .textFieldStyle(.roundedBorder)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.go)
                .focused($fieldFocused)
                .onSubmit(submit)
                .onChange(of: text) { notice = nil }
                .accessibilityIdentifier("search-field")

                if let notice {
                    Label(notice, systemImage: "info.circle")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("search-notice")
                }

                Text("Paste a profile or note link to jump to it, or enter a #hashtag.")
                    .font(.footnote)
                    .foregroundStyle(.tertiary)

                Spacer()
            }
            .padding()
            .navigationTitle("Go to")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Go", action: submit)
                        .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .task { fieldFocused = true }
        }
        .presentationDetents([.medium, .large])
    }

    private func submit() {
        let query = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return }

        switch model.classify(query: query) {
        case .profile(let pubkey, _):
            navigate(.profile(pubkey: pubkey))
        case .event(let eventID, _, _, _):
            navigate(.thread(eventID: eventID))
        case .hashtag(let tag):
            navigate(.hashtag(tag: tag))
        case .nip05(let identifier):
            notice = "Opening \(identifier) by NIP-05 is coming soon."
        case .search(let query):
            notice = "Full-text search for “\(query)” is coming soon."
        case .unsupported(let reason):
            notice = message(forUnsupported: reason)
        }
    }

    private func navigate(_ route: ChirpRoute) {
        onNavigate(route)
        dismiss()
    }

    private func message(forUnsupported reason: String) -> String {
        switch reason {
        case "nsec-forbidden":
            return "That looks like a secret key (nsec). Never paste your nsec anywhere."
        case "addressable-unsupported":
            return "Long-form / addressable (naddr) links aren't supported yet."
        default:
            return "Sorry, that doesn't look like anything I can open."
        }
    }
}
