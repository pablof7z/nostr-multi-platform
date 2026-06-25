import SwiftUI

private enum NewGroupKind: Int, CaseIterable, Identifiable {
    case privateGroup
    case publicGroup

    var id: Int { rawValue }
    var label: String {
        switch self {
        case .privateGroup: return "Private"
        case .publicGroup: return "Public"
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// NewGroupSheet — create a new MLS or NIP-29 group.
//
// For private (MLS/Marmot) groups the sheet stays OPEN after dispatch
// until the kernel reports a terminal verdict:
//
//   1. Dispatch fires → `pendingCid` stashed.
//   2. While the op is parked in `snapshot.pendingOps`, render the
//      Rust-supplied `displayLabel` verbatim. No polling — snapshot is push.
//   3. `recentTerminal(correlationId:)` returning `.accepted` → dismiss.
//   4. `.failed(reason)` → show reason verbatim, re-enable (retry/cancel).
//   5. `snapshot.lastOpError` shown as a banner when no pending cid.
//
// Thin-shell rule: ALL copy comes from Rust. Swift only gates dismissal.
// ─────────────────────────────────────────────────────────────────────────

struct NewGroupSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var kind: NewGroupKind = .privateGroup
    @State private var name = ""
    @State private var groupDescription = ""
    @State private var inviteeText = ""
    // #626/#1924: the suggested public-group relay URL is app/operator policy
    // supplied by Rust composition (`nmp-chirp-config`) and surfaced on the
    // kernel snapshot under `"nmp.nip29.group_defaults"`. This field starts
    // EMPTY and is seeded from `model.groupDefaults.suggestedRelayUrl` once
    // that projection lands (see `seedRelayUrlIfNeeded`) — never a hardcoded
    // Swift literal. Swift keeps only the editable `TextField` binding; the
    // user may overwrite the pre-filled value.
    @State private var publicRelayUrl = ""
    /// `true` once the kernel-suggested relay URL has been seeded into
    /// `publicRelayUrl`, so a later snapshot tick (or a user edit) never
    /// re-seeds over an existing value.
    @State private var didSeedRelayUrl = false
    @State private var publicLocalId = ""

    // ── Async feedback state ──────────────────────────────────────────────
    /// Correlation id stashed after a successful dispatch.
    @State private var pendingCid: String?
    /// Synchronous dispatch failure (bridge unavailable / bad JSON).
    @State private var syncError: String?

    private var trimmedName: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var trimmedDescription: String { groupDescription.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var trimmedRelayUrl: String { publicRelayUrl.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var trimmedLocalId: String { publicLocalId.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// Op currently parked in Rust's deferred-completion store for our cid.
    private var pendingOpRow: MarmotPendingOp? {
        guard let cid = pendingCid else { return nil }
        return model.marmot.snapshot.pendingOps.first { $0.correlationId == cid }
    }

    private var isWaiting: Bool { pendingCid != nil }

    var body: some View {
        NavigationStack {
            Form {
                typeSection
                detailsSection
                feedbackSection
                actionSection
            }
            .scrollContentBackground(.hidden)
            .chirpScreenBackground()
            .navigationTitle("New Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
            }
            .onChange(of: model.actionLifecycle) { resolveTerminal() }
            // #626: seed the relay field from the kernel-projected default.
            // `onAppear` covers the case where the snapshot already carries it;
            // `onChange` covers the late-arriving first tick (empty until then).
            .onAppear { seedRelayUrlIfNeeded() }
            .onChange(of: model.groupDefaults.suggestedRelayUrl) { seedRelayUrlIfNeeded() }
        }
    }

    /// Seed `publicRelayUrl` from the kernel-projected NIP-29 group default
    /// (`model.groupDefaults.suggestedRelayUrl`, #626) exactly once, and only
    /// while the field is still untouched. The field is empty until the
    /// projection's first snapshot tick lands; once a non-empty suggestion
    /// arrives it pre-fills, after which a user edit (or a later tick) never
    /// overwrites the value.
    private func seedRelayUrlIfNeeded() {
        guard !didSeedRelayUrl, publicRelayUrl.isEmpty else { return }
        let suggested = model.groupDefaults.suggestedRelayUrl
        guard !suggested.isEmpty else { return }
        publicRelayUrl = suggested
        didSeedRelayUrl = true
    }

    // ── Sections ──────────────────────────────────────────────────────────

    private var typeSection: some View {
        Section {
            Picker("Type", selection: $kind) {
                ForEach(NewGroupKind.allCases) { k in Text(k.label).tag(k) }
            }
            .pickerStyle(.segmented)
            .disabled(isWaiting)
        }
    }

    @ViewBuilder
    private var detailsSection: some View {
        switch kind {
        case .privateGroup:
            Section {
                field("Group name", text: $name, placeholder: "Trusted circle").disabled(isWaiting)
                field("Description", text: $groupDescription, placeholder: "Optional").disabled(isWaiting)
                membersEditor.disabled(isWaiting)
            }
        case .publicGroup:
            Section {
                field("Group name", text: $name, placeholder: "Rust Nostr").disabled(isWaiting)
                field("Description", text: $groupDescription, placeholder: "Optional").disabled(isWaiting)
                field("Relay URL", text: $publicRelayUrl, placeholder: "wss://groups.example.com")
                    .keyboardType(.URL).disabled(isWaiting)
                field("Group ID", text: $publicLocalId, placeholder: "rust-nostr").disabled(isWaiting)
            }
        }
    }

    @ViewBuilder
    private var feedbackSection: some View {
        // Pending row: Rust-owned display_label rendered verbatim.
        if let row = pendingOpRow {
            Section {
                HStack(spacing: ChirpSpace.s) {
                    ProgressView().controlSize(.small)
                    Text(row.displayLabel)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        // Synchronous dispatch failure or terminal failure.
        if let err = syncError {
            Section {
                Text(err)
                    .font(.caption)
                    .foregroundStyle(ChirpColor.danger)
            }
        }
        // Last-op-error from snapshot (survives correlation window expiry).
        // Rust-owned machine code mapped to a banner via `bannerText`.
        if pendingCid == nil, let lastErr = model.marmot.snapshot.lastOpError {
            Section {
                Text(lastErr.bannerText)
                    .font(.caption)
                    .foregroundStyle(ChirpColor.danger)
            }
        }
    }

    @ViewBuilder
    private var actionSection: some View {
        Section {
            // Show spinner while dispatched + no pending_ops row yet.
            if isWaiting, pendingOpRow == nil {
                HStack(spacing: ChirpSpace.s) {
                    ProgressView().controlSize(.small)
                    Text("Creating…")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            } else if !isWaiting {
                Button { create() } label: { Text("Create group") }
                    .disabled(createDisabled)
                // When Marmot is not registered (bunker / no local nsec), surface
                // the reason so the user knows how to unblock themselves. The shell
                // owns this copy now (aim.md §2 — presentation lives in the shell).
                if kind == .privateGroup, !model.marmot.isRegistered {
                    Text("Sign in with an nsec to enable")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private var membersEditor: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Members")
                .font(.caption)
                .foregroundStyle(.secondary)
            TextEditor(text: $inviteeText)
                .frame(minHeight: 90)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .overlay(alignment: .topLeading) {
                    if inviteeText.isEmpty {
                        Text("npub1... or hex pubkey, one per line")
                            .font(.body)
                            .foregroundStyle(.secondary)
                            .allowsHitTesting(false)
                            .padding(.top, 8)
                    }
                }
        }
    }

    private var createDisabled: Bool {
        if trimmedName.isEmpty { return true }
        switch kind {
        case .privateGroup: return !model.marmot.isRegistered
        case .publicGroup: return trimmedRelayUrl.isEmpty || trimmedLocalId.isEmpty
        }
    }

    private func create() {
        syncError = nil
        pendingCid = nil
        switch kind {
        case .privateGroup: createPrivateGroup()
        case .publicGroup: createPublicGroup()
        }
    }

    private func createPrivateGroup() {
        Task {
            let result = await model.marmot.createGroup(
                name: trimmedName,
                description: trimmedDescription,
                inviteeText: inviteeText)
            if result.ok, let cid = result.correlationId {
                pendingCid = cid
                // Check immediately in case terminal already landed.
                resolveTerminal()
            } else {
                syncError = result.error ?? "Could not create group"
            }
        }
    }

    private func createPublicGroup() {
        let group = GroupId(hostRelayUrl: trimmedRelayUrl, localId: trimmedLocalId)
        let result = model.createPublicGroup(
            group: group,
            name: trimmedName,
            about: trimmedDescription.isEmpty ? nil : trimmedDescription)
        switch result {
        case .accepted: dismiss()
        case .failure(let message): syncError = message
        }
    }

    /// Matches `pendingCid` against `recentTerminal` — same pattern as
    /// RelaySettingsView and HomeFeedView. Called on every `actionLifecycle` change.
    private func resolveTerminal() {
        guard let cid = pendingCid,
              let entry = model.recentTerminal(correlationId: cid) else { return }
        pendingCid = nil
        switch entry.stage {
        case .accepted:
            dismiss()
        case .failed:
            // #1735: prefer the localized reason_code, falling back to prose.
            let reason = entry.stage.localizedReason ?? ""
            syncError = reason.isEmpty ? "Group creation failed" : reason
        default:
            break
        }
    }

    @ViewBuilder
    private func field(_ label: String, text: Binding<String>, placeholder: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            TextField(placeholder, text: text)
                .font(.body)
                .foregroundStyle(.primary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
        }
    }
}
