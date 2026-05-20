import SwiftUI

struct SearchView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var query = ""
    @FocusState private var fieldFocused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: ChirpSpace.l) {
                openEntityCard
                searchStatusCard
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.top, ChirpSpace.m)
        }
        .chirpScreenBackground()
        .navigationTitle("Search")
        .navigationBarTitleDisplayMode(.large)
        .onTapGesture { fieldFocused = false }
    }

    private var openEntityCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Open exact Nostr ID")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)

            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 8) {
                    Image(systemName: "number")
                        .foregroundStyle(hexValid ? Color.accentColor : .secondary)

                    TextField("64-character pubkey or note ID", text: $query)
                        .font(.footnote.monospaced())
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .focused($fieldFocused)
                        .submitLabel(.done)
                        .onSubmit { fieldFocused = false }

                    if !query.isEmpty {
                        Button {
                            query = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                        }
                        .accessibilityLabel("Clear ID")
                    }
                }
                .padding(ChirpSpace.m)
                .chirpSurface(cornerRadius: ChirpSpace.radiusSmall)

                if !query.isEmpty && !hexValid {
                    HStack(spacing: 4) {
                        Image(systemName: "exclamationmark.circle")
                        Text("Must be a 64-character hex string (\(query.count)/64 chars)")
                    }
                    .font(.caption)
                    .foregroundStyle(.red)
                }

                ViewThatFits {
                    HStack(spacing: ChirpSpace.m) {
                        openProfileButton
                        openThreadButton
                    }

                    VStack(spacing: ChirpSpace.s) {
                        openProfileButton
                        openThreadButton
                    }
                }
            }
        }
        .padding(ChirpSpace.l)
        .chirpGlass(cornerRadius: ChirpSpace.radius)
    }

    private var searchStatusCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Discovery")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)

            HStack(spacing: 12) {
                Image(systemName: "magnifyingglass.circle")
                    .font(.system(size: 28))
                    .foregroundStyle(Color.accentColor)

                VStack(alignment: .leading, spacing: 4) {
                    Text("Search is being wired to Rust")
                        .font(.headline)
                    Text("Exact IDs work today. Keyword, hashtag, and profile discovery will appear here once the kernel projects search results.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            VStack(alignment: .leading, spacing: 8) {
                featureLine(icon: "person.crop.circle.badge.questionmark", label: "Names and NIP-05 profiles")
                featureLine(icon: "number", label: "Hashtags and topics")
                featureLine(icon: "doc.text.magnifyingglass", label: "Relay-backed note search")
            }
        }
        .padding(ChirpSpace.l)
        .chirpSurface(cornerRadius: ChirpSpace.radius)
    }

    private var hexValid: Bool {
        query.count == 64 && query.allSatisfy(\.isHexDigit)
    }

    private var openProfileButton: some View {
        Button {
            guard hexValid else { return }
            model.openAuthor(pubkey: query)
            router.push(.profile(pubkey: query))
            fieldFocused = false
        } label: {
            Label("Open Profile", systemImage: "person.circle")
                .frame(maxWidth: .infinity)
        }
        .buttonStyle(ChirpGlassButtonStyle())
        .disabled(!hexValid)
    }

    private var openThreadButton: some View {
        Button {
            guard hexValid else { return }
            model.openThread(eventID: query)
            router.push(.thread(eventID: query))
            fieldFocused = false
        } label: {
            Label("Open Thread", systemImage: "bubble.left.and.bubble.right")
                .frame(maxWidth: .infinity)
        }
        .buttonStyle(ChirpGlassButtonStyle())
        .disabled(!hexValid)
    }

    private func featureLine(icon: String, label: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .foregroundStyle(Color.accentColor)
            Text(label)
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}
