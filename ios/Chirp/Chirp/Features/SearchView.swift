import SwiftUI

// OWNER: Phase-2 Agent D — Search (real "open entity" box + CX4 stub).

struct SearchView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var query = ""
    @FocusState private var fieldFocused: Bool

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xl) {
                openEntityCard
                searchComingCard
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.top, ChirpSpace.m)
            .padding(.bottom, ChirpSpace.xxl)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Search")
        .navigationBarTitleDisplayMode(.large)
        .onTapGesture { fieldFocused = false }
    }

    // ── Open entity card ──────────────────────────────────────────────────

    private var openEntityCard: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Open by ID")

            GlassCard {
                VStack(alignment: .leading, spacing: ChirpSpace.l) {
                    // Input field
                    VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                        HStack(spacing: ChirpSpace.s) {
                            Image(systemName: "number")
                                .font(.system(size: 14, weight: .medium))
                                .foregroundStyle(hexValid ? ChirpColor.accent : ChirpColor.textTertiary)
                                .animation(.smooth(duration: 0.2), value: hexValid)

                            TextField("64-character hex pubkey or event ID", text: $query)
                                .font(ChirpFont.mono)
                                .foregroundStyle(ChirpColor.textPrimary)
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
                                        .foregroundStyle(ChirpColor.textTertiary)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(ChirpSpace.m)
                        .background(
                            RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
                                .fill(ChirpColor.surface)
                                .overlay(
                                    RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
                                        .strokeBorder(
                                            hexValid ? ChirpColor.accent.opacity(0.5) : ChirpColor.hairline,
                                            lineWidth: hexValid ? 1.5 : 1
                                        )
                                )
                        )
                        .animation(.smooth(duration: 0.25), value: hexValid)

                        // Validation hint
                        if !query.isEmpty && !hexValid {
                            HStack(spacing: ChirpSpace.xs) {
                                Image(systemName: "exclamationmark.circle")
                                    .font(.system(size: 11, weight: .medium))
                                Text("Must be a 64-character hex string (\(query.count)/64 chars)")
                                    .font(ChirpFont.caption)
                            }
                            .foregroundStyle(ChirpColor.like)
                            .transition(.opacity.combined(with: .move(edge: .top)))
                        }
                    }
                    .animation(.smooth(duration: 0.2), value: query.isEmpty)

                    // Action buttons
                    VStack(spacing: ChirpSpace.s) {
                        Button {
                            guard hexValid else { return }
                            model.openAuthor(pubkey: query)
                            router.push(.profile(pubkey: query))
                            fieldFocused = false
                        } label: {
                            HStack(spacing: ChirpSpace.s) {
                                Image(systemName: "person.circle")
                                    .font(.system(size: 16, weight: .semibold))
                                Text("Open Profile")
                                    .font(ChirpFont.headline)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                            .background(
                                hexValid ? ChirpColor.accent : ChirpColor.accent.opacity(0.3),
                                in: Capsule()
                            )
                            .foregroundStyle(.white)
                        }
                        .buttonStyle(.plain)
                        .disabled(!hexValid)
                        .animation(.smooth(duration: 0.2), value: hexValid)

                        Button {
                            guard hexValid else { return }
                            model.openThread(eventID: query)
                            router.push(.thread(eventID: query))
                            fieldFocused = false
                        } label: {
                            HStack(spacing: ChirpSpace.s) {
                                Image(systemName: "bubble.left.and.bubble.right")
                                    .font(.system(size: 16, weight: .semibold))
                                Text("Open Thread")
                                    .font(ChirpFont.headline)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                            .background(
                                .ultraThinMaterial,
                                in: Capsule()
                            )
                            .overlay(
                                Capsule()
                                    .strokeBorder(
                                        hexValid ? ChirpColor.accent.opacity(0.4) : ChirpColor.hairline,
                                        lineWidth: 1
                                    )
                            )
                            .foregroundStyle(hexValid ? ChirpColor.accent : ChirpColor.textTertiary)
                        }
                        .buttonStyle(.plain)
                        .disabled(!hexValid)
                        .animation(.smooth(duration: 0.2), value: hexValid)
                    }
                }
            }
        }
    }

    // ── NIP-50 full search teaser ─────────────────────────────────────────

    private var searchComingCard: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Full-text Search")
            GlassCard {
                VStack(spacing: ChirpSpace.l) {
                    HStack(spacing: ChirpSpace.m) {
                        ZStack {
                            Circle()
                                .fill(ChirpColor.accentSoft)
                                .frame(width: 56, height: 56)
                            Image(systemName: "magnifyingglass.circle")
                                .font(.system(size: 28, weight: .light))
                                .foregroundStyle(ChirpColor.accent)
                        }

                        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                            HStack(spacing: ChirpSpace.xs) {
                                Text("Full Search")
                                    .font(ChirpFont.headline)
                                    .foregroundStyle(ChirpColor.textPrimary)
                                versionTag("CX4")
                            }
                            Text("Keyword and hashtag search across Nostr")
                                .font(ChirpFont.callout)
                                .foregroundStyle(ChirpColor.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    Divider()
                        .background(ChirpColor.hairline)

                    VStack(alignment: .leading, spacing: ChirpSpace.s) {
                        featureLine(icon: "doc.text.magnifyingglass", label: "NIP-50 relay-backed full-text search")
                        featureLine(icon: "number", label: "Hashtag discovery and trending topics")
                        featureLine(icon: "person.2.wave.2", label: "People search by name or NIP-05")
                    }
                }
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private var hexValid: Bool {
        query.count == 64 && query.allSatisfy(\.isHexDigit)
    }

    @ViewBuilder
    private func versionTag(_ label: String) -> some View {
        Text(label)
            .font(.system(.caption2, design: .rounded).weight(.bold))
            .foregroundStyle(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(ChirpColor.accent, in: Capsule())
    }

    @ViewBuilder
    private func featureLine(icon: String, label: String) -> some View {
        HStack(spacing: ChirpSpace.s) {
            Image(systemName: icon)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(ChirpColor.accent)
                .frame(width: 20)
            Text(label)
                .font(ChirpFont.callout)
                .foregroundStyle(ChirpColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
