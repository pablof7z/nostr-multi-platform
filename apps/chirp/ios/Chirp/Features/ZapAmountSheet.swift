import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// ZapAmountSheet — NIP-57 amount picker (V-106).
//
// Replaces the hardcoded 21,000-msat zap default that used to ship on every
// tap of the zap button. The host (HomeFeedView) presents this sheet when the
// user taps zap; the user picks a preset (21 / 100 / 500 / 1k / 5k / 21k sats)
// or types a custom sats amount, optionally adds a comment, and confirms. The
// chosen amount (in msats) and comment flow back through `onConfirm` to
// `KernelModel.zap(amountMsats:comment:)`.
//
// Chirp thin-shell rule: this sheet is PURE PRESENTATION. The preset sats
// values are static UI constants; the only "logic" is the trivial sats→msats
// arithmetic (`sats * 1_000`) and parsing the custom field, both of which are
// presentation concerns (formatting user input for the kernel's `amount_msats`
// field). No protocol decisions live here — the kernel owns relay selection,
// LNURL resolution, receipt publication, and the zap request itself.
// ─────────────────────────────────────────────────────────────────────────

/// Convert a whole-sats amount to msats (the unit the kernel's
/// `nmp.nip57.zap` action body expects). 1 sat = 1,000 msats.
func zapMsats(fromSats sats: UInt64) -> UInt64 {
    sats * 1_000
}

/// Parse a free-text custom amount (whole sats) into msats. Returns `nil`
/// when the input is empty, non-numeric, or zero — the confirm button stays
/// disabled in those cases so the host can never dispatch a zero-amount zap.
/// Strips grouping separators and whitespace the user may have typed.
func parseCustomZapMsats(_ raw: String) -> UInt64? {
    let cleaned = raw.filter { $0.isNumber }
    guard let sats = UInt64(cleaned), sats > 0 else { return nil }
    return zapMsats(fromSats: sats)
}

/// The static preset ladder offered in the sheet, in sats. Pure UI constants
/// (no kernel coupling). 21k sats remains available as the top preset — the
/// difference from the old behaviour is that the user now CHOOSES it.
let zapPresetSats: [UInt64] = [21, 100, 500, 1_000, 5_000, 21_000]

struct ZapAmountSheet: View {
    @Environment(\.dismiss) private var dismiss

    /// (amountMsats, comment?) → dispatch the zap. The host supplies a closure
    /// that calls `KernelModel.zap`. Only invoked with a non-zero amount.
    let onConfirm: (UInt64, String?) -> Void

    /// Currently selected preset (in sats). `nil` when the custom field is the
    /// active source of the amount.
    @State private var selectedPresetSats: UInt64? = 21
    @State private var customText: String = ""
    @State private var comment: String = ""
    @FocusState private var customFocused: Bool

    /// The msats amount that will be dispatched, or `nil` when nothing valid
    /// is selected/typed (confirm disabled).
    private var resolvedMsats: UInt64? {
        if let preset = selectedPresetSats {
            return zapMsats(fromSats: preset)
        }
        return parseCustomZapMsats(customText)
    }

    private var canConfirm: Bool { resolvedMsats != nil }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: ChirpSpace.l) {
                presetGrid
                customField
                commentField
                Spacer(minLength: 0)
            }
            .padding(ChirpSpace.l)
            .background(ChirpColor.bg.ignoresSafeArea())
            .navigationTitle("Send a Zap")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Zap", action: confirm)
                        .fontWeight(.semibold)
                        .disabled(!canConfirm)
                        .accessibilityIdentifier("zap-confirm-button")
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    // ── Preset ladder ────────────────────────────────────────────────────
    private var presetGrid: some View {
        let columns = Array(repeating: GridItem(.flexible(), spacing: ChirpSpace.s), count: 3)
        return LazyVGrid(columns: columns, spacing: ChirpSpace.s) {
            ForEach(zapPresetSats, id: \.self) { sats in
                presetButton(sats)
            }
        }
    }

    private func presetButton(_ sats: UInt64) -> some View {
        let isSelected = selectedPresetSats == sats
        return Button {
            selectedPresetSats = sats
            customText = ""
            customFocused = false
        } label: {
            VStack(spacing: ChirpSpace.xs) {
                Image(systemName: "bolt.fill")
                    .font(.system(size: 14, weight: .semibold))
                Text(presetLabel(sats))
                    .font(.callout.weight(.semibold))
                    .minimumScaleFactor(0.7)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, ChirpSpace.m)
            .foregroundStyle(isSelected ? ChirpColor.emphasisForeground : ChirpColor.zap)
            .background(
                RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall)
                    .fill(isSelected ? ChirpColor.zap : ChirpColor.surface)
            )
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("zap-preset-\(sats)")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    /// Compact preset label: "21", "1k", "5k", "21k". Pure formatting.
    private func presetLabel(_ sats: UInt64) -> String {
        if sats >= 1_000 && sats % 1_000 == 0 {
            return "\(sats / 1_000)k"
        }
        return "\(sats)"
    }

    // ── Custom amount ────────────────────────────────────────────────────
    private var customField: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            ChirpSectionHeader(title: "Custom (sats)")
            TextField("Amount in sats", text: $customText)
                .keyboardType(.numberPad)
                .focused($customFocused)
                .padding(ChirpSpace.m)
                .background(
                    RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall)
                        .fill(ChirpColor.surface)
                )
                .accessibilityIdentifier("zap-custom-field")
                .onChange(of: customText) { _, newValue in
                    // Typing into the custom field deselects any preset so the
                    // resolved amount comes from the text (single source).
                    if !newValue.isEmpty {
                        selectedPresetSats = nil
                    }
                }
        }
    }

    // ── Optional comment ─────────────────────────────────────────────────
    private var commentField: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            ChirpSectionHeader(title: "Comment (optional)")
            TextField("Say something nice", text: $comment)
                .padding(ChirpSpace.m)
                .background(
                    RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall)
                        .fill(ChirpColor.surface)
                )
                .accessibilityIdentifier("zap-comment-field")
        }
    }

    private func confirm() {
        guard let msats = resolvedMsats else { return }
        let trimmed = comment.trimmingCharacters(in: .whitespacesAndNewlines)
        onConfirm(msats, trimmed.isEmpty ? nil : trimmed)
        UINotificationFeedbackGenerator().notificationOccurred(.success)
        dismiss()
    }
}
