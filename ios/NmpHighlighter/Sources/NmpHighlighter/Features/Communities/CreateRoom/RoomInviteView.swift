import CoreImage.CIFilterBuiltins
import Kingfisher
import SwiftUI

/// Add-people screen used both right after creating a room (welcome mode)
/// and from the room's overflow menu (manage mode).
///
/// Mental model: there is no segmented "manual vs link" picker — both
/// exist on one canvas. The share card at the top is "whoever shows up"
/// and the search field below is "specifically these people". A unified
/// search field auto-detects npub / nprofile / hex pubkey on paste, and
/// otherwise filters the user's follow list. Selected invitees collect as
/// paper chips above the field; a sticky "Add (N)" button only appears
/// when chips exist.
struct RoomInviteView: View {
    enum Mode {
        case welcome
        case manage
    }

    let groupId: String
    let mode: Mode
    let onClose: (() -> Void)?

    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    @State private var query: String = ""
    @State private var follows: [String] = []
    @State private var followsLoaded = false
    @State private var pasteResolution: ResolvedCandidate?
    @State private var selected: [Candidate] = []
    @State private var sending = false
    @State private var error: String?
    @State private var sentToast: String?

    var body: some View {
        ZStack(alignment: .bottom) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 24) {
                    if mode == .welcome {
                        welcomeHeader
                            .padding(.horizontal, 22)
                            .padding(.top, 8)
                    }

                    RoomShareCard(groupId: groupId, room: cachedRoom)
                        .padding(.horizontal, 22)

                    sectionHeader("Add specific people")
                        .padding(.horizontal, 22)

                    chipsZone
                        .padding(.horizontal, 22)

                    searchField
                        .padding(.horizontal, 22)

                    suggestionsList

                    Spacer(minLength: 140)
                }
                .padding(.top, 8)
            }
            .scrollDismissesKeyboard(.interactively)

            stickyAddBar
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationTitle(mode == .welcome ? "" : "Add people")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if mode == .welcome {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        if let onClose { onClose() } else { dismiss() }
                    }
                    .foregroundStyle(Color.highlighterInkStrong)
                }
            }
        }
        .task {
            await loadFollows()
        }
        .onChange(of: query) { _, newValue in
            Task { await resolvePaste(input: newValue) }
        }
        .alert("Couldn't add", isPresented: errorBinding, actions: {
            Button("OK") { error = nil }
        }, message: { if let error { Text(error) } })
        .overlay(alignment: .top) {
            if let toast = sentToast {
                Text(toast)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .background(Color.highlighterInkStrong, in: Capsule())
                    .padding(.top, 8)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
    }

    // MARK: - Sections

    private var welcomeHeader: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Your room is open.")
                .font(.system(.title2, design: .default).italic())
                .foregroundStyle(Color.highlighterInkStrong)
            Text("Invite the first guests below — or share the link.")
                .font(.subheadline)
                .foregroundStyle(Color.highlighterInkMuted)
        }
    }

    private func sectionHeader(_ title: String) -> some View {
        Text(title.uppercased())
            .font(.footnote.weight(.semibold))
            .tracking(1.2)
            .foregroundStyle(Color.highlighterInkMuted)
    }

    @ViewBuilder
    private var chipsZone: some View {
        if !selected.isEmpty {
            FlowChips(items: selected) { candidate in
                Chip(candidate: candidate, profile: profile(for: candidate.pubkeyHex)) {
                    remove(candidate)
                }
            }
        }
    }

    private var searchField: some View {
        HStack(spacing: 10) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(Color.highlighterInkMuted)
            TextField(
                "",
                text: $query,
                prompt: Text("Search follows or paste an npub")
                    .foregroundColor(Color.highlighterInkMuted.opacity(0.7))
            )
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .submitLabel(.done)
            .onSubmit { acceptPasteIfAny() }
            if !query.isEmpty {
                Button {
                    query = ""
                    pasteResolution = nil
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Color.highlighterInkMuted)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .stroke(Color.highlighterRule, lineWidth: 1)
        )
    }

    @ViewBuilder
    private var suggestionsList: some View {
        if let resolved = pasteResolution {
            VStack(spacing: 0) {
                personRow(
                    pubkeyHex: resolved.pubkeyHex,
                    profile: profile(for: resolved.pubkeyHex),
                    secondary: resolved.kind.label,
                    isSelected: isSelected(resolved.pubkeyHex)
                ) {
                    add(Candidate(pubkeyHex: resolved.pubkeyHex, source: resolved.kind.candidateSource))
                    query = ""
                    pasteResolution = nil
                }
            }
            .padding(.horizontal, 22)
        } else {
            let visible = visibleFollows()
            if visible.isEmpty && !query.isEmpty && followsLoaded {
                Text("No matching follow — paste an npub to invite anyone.")
                    .font(.subheadline)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .padding(.horizontal, 22)
                    .padding(.top, 8)
            } else {
                LazyVStack(spacing: 0) {
                    ForEach(visible, id: \.self) { pubkey in
                        personRow(
                            pubkeyHex: pubkey,
                            profile: profile(for: pubkey),
                            secondary: "Following",
                            isSelected: isSelected(pubkey)
                        ) {
                            toggle(Candidate(pubkeyHex: pubkey, source: .follow))
                        }
                        if pubkey != visible.last {
                            Divider().overlay(Color.highlighterRule)
                                .padding(.leading, 70)
                        }
                    }
                }
                .padding(.horizontal, 22)
            }
        }
    }

    @ViewBuilder
    private var stickyAddBar: some View {
        if !selected.isEmpty {
            VStack(spacing: 0) {
                LinearGradient(
                    colors: [Color.highlighterPaper.opacity(0), Color.highlighterPaper],
                    startPoint: .top,
                    endPoint: .bottom
                )
                .frame(height: 24)

                Button(action: send) {
                    ZStack {
                        if sending {
                            ProgressView().tint(.white)
                        } else {
                            Text(selected.count == 1 ? "Add 1 person" : "Add \(selected.count) people")
                                .font(.headline)
                                .foregroundStyle(.white)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 16)
                    .background(
                        RoundedRectangle(cornerRadius: 16)
                            .fill(Color.highlighterAccent)
                    )
                }
                .buttonStyle(.plain)
                .disabled(sending)
                .padding(.horizontal, 22)
                .padding(.bottom, 24)
                .background(Color.highlighterPaper)
            }
        }
    }

    // MARK: - Person row

    @ViewBuilder
    private func personRow(
        pubkeyHex: String,
        profile: ProfileMetadata?,
        secondary: String,
        isSelected: Bool,
        onTap: @escaping () -> Void
    ) -> some View {
        Button(action: onTap) {
            HStack(spacing: 14) {
                AvatarView(profile: profile, pubkeyHex: pubkeyHex, size: 44)

                VStack(alignment: .leading, spacing: 2) {
                    Text(displayName(profile: profile, fallback: pubkeyHex))
                        .font(.body.weight(.medium))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .lineLimit(1)
                    Text(secondary)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                Image(systemName: isSelected ? "checkmark.circle.fill" : "plus.circle")
                    .font(.title3)
                    .foregroundStyle(isSelected ? Color.highlighterAccent : Color.highlighterInkMuted)
            }
            .padding(.vertical, 12)
        }
        .buttonStyle(.plain)
        .task {
            if profile == nil {
                await appStore.requestProfile(pubkeyHex: pubkeyHex)
            }
        }
    }

    // MARK: - State helpers

    private var cachedRoom: CommunitySummary? {
        appStore.joinedCommunities.first(where: { $0.id == groupId })
    }

    private func profile(for pubkey: String) -> ProfileMetadata? {
        appStore.profileCache[pubkey]
    }

    private func isSelected(_ pubkey: String) -> Bool {
        selected.contains(where: { $0.pubkeyHex == pubkey })
    }

    private func toggle(_ candidate: Candidate) {
        if isSelected(candidate.pubkeyHex) {
            remove(candidate)
        } else {
            add(candidate)
        }
    }

    private func add(_ candidate: Candidate) {
        guard !isSelected(candidate.pubkeyHex) else { return }
        let me = appStore.currentUser?.pubkey ?? ""
        if candidate.pubkeyHex.lowercased() == me.lowercased() {
            error = "You're already in this room."
            return
        }
        selected.append(candidate)
        UISelectionFeedbackGenerator().selectionChanged()
    }

    private func remove(_ candidate: Candidate) {
        selected.removeAll(where: { $0.pubkeyHex == candidate.pubkeyHex })
    }

    private func acceptPasteIfAny() {
        guard let resolved = pasteResolution else { return }
        add(Candidate(pubkeyHex: resolved.pubkeyHex, source: resolved.kind.candidateSource))
        query = ""
        pasteResolution = nil
    }

    private var errorBinding: Binding<Bool> {
        Binding(get: { error != nil }, set: { if !$0 { error = nil } })
    }

    private func visibleFollows() -> [String] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return Array(follows.prefix(50)) }
        let needle = trimmed.lowercased()
        return follows.filter { pubkey in
            let prof = profile(for: pubkey)
            let name = (prof?.name ?? "").lowercased()
            let nip05 = (prof?.nip05 ?? "").lowercased()
            let displayName = (prof?.displayName ?? "").lowercased()
            return name.contains(needle) || nip05.contains(needle) || displayName.contains(needle)
        }.prefix(50).map { $0 }
    }

    private func displayName(profile: ProfileMetadata?, fallback hex: String) -> String {
        if let displayName = profile?.displayName, !displayName.isEmpty { return displayName }
        if let name = profile?.name, !name.isEmpty { return name }
        return shortPubkey(hex)
    }

    // MARK: - Loading + actions

    private func loadFollows() async {
        do {
            let result = try await appStore.safeCore.getFollows()
            await MainActor.run {
                follows = result
                followsLoaded = true
            }
            // Warm the profile cache for the first chunk so suggestions
            // render with names rather than truncated hex.
            for pubkey in result.prefix(40) {
                await appStore.requestProfile(pubkeyHex: pubkey)
            }
        } catch {
            await MainActor.run { followsLoaded = true }
        }
    }

    private func resolvePaste(input: String) async {
        let trimmed = input
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "nostr:", with: "")
        guard looksLikeReference(trimmed) else {
            await MainActor.run { pasteResolution = nil }
            return
        }
        do {
            let hex = try await appStore.safeCore.decodeNpub(trimmed)
            let kind: ResolvedCandidate.Kind
            if trimmed.lowercased().hasPrefix("npub") { kind = .npub }
            else if trimmed.lowercased().hasPrefix("nprofile") { kind = .nprofile }
            else { kind = .hex }
            await appStore.requestProfile(pubkeyHex: hex)
            await MainActor.run {
                pasteResolution = ResolvedCandidate(pubkeyHex: hex, kind: kind)
            }
        } catch {
            await MainActor.run { pasteResolution = nil }
        }
    }

    private func looksLikeReference(_ s: String) -> Bool {
        let lower = s.lowercased()
        if lower.hasPrefix("npub1") && lower.count >= 60 { return true }
        if lower.hasPrefix("nprofile1") && lower.count >= 60 { return true }
        if s.count == 64 && s.allSatisfy({ $0.isHexDigit }) { return true }
        return false
    }

    private func send() {
        guard !sending, !selected.isEmpty else { return }
        sending = true
        let toAdd = selected
        Task {
            defer { Task { @MainActor in sending = false } }
            var failures: [String] = []
            for candidate in toAdd {
                do {
                    _ = try await appStore.safeCore.addRoomMember(
                        groupId: groupId,
                        pubkeyHex: candidate.pubkeyHex
                    )
                } catch {
                    failures.append(shortPubkey(candidate.pubkeyHex))
                }
            }
            await MainActor.run {
                if failures.isEmpty {
                    let added = toAdd.count
                    selected.removeAll()
                    sentToast = added == 1 ? "Added 1 person" : "Added \(added) people"
                    UINotificationFeedbackGenerator().notificationOccurred(.success)
                    Task {
                        try? await Task.sleep(for: .seconds(2))
                        sentToast = nil
                    }
                } else if failures.count == toAdd.count {
                    error = "Couldn't add anyone. Are you a moderator of this room?"
                } else {
                    selected.removeAll(where: { c in !failures.contains(shortPubkey(c.pubkeyHex)) })
                    error = "Some failed: \(failures.joined(separator: ", "))"
                }
            }
        }
    }

    private func shortPubkey(_ hex: String) -> String {
        guard hex.count > 12 else { return hex }
        let prefix = hex.prefix(6)
        let suffix = hex.suffix(4)
        return "\(prefix)…\(suffix)"
    }
}
