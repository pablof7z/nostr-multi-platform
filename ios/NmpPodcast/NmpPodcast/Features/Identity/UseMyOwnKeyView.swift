import SwiftUI
import UIKit

// MARK: - UseMyOwnKeyView
//
// Per identity-05-synthesis §4.6. T0 reading body. SecureField with mono
// typography; inline Show / Paste actions. Confirm checkbox + non-empty
// field gates the Use button. Errors render inline below the field, never
// in an alert. On success: pop two views back to Identity root.

struct UseMyOwnKeyView: View {

    /// Called on a successful nsec import. The caller (AdvancedView) dismisses
    /// itself, which pops both it and this view — landing the user on the
    /// Identity root rather than stranding them on AdvancedView.
    var onImportComplete: () -> Void = {}

    @Environment(UserIdentityStore.self) private var identity
    @State private var nsec = ""
    @State private var revealed = false
    @State private var hasBackup = false
    @State private var inlineError: String?
    @State private var importing = false
    /// Cached so we don't read the clipboard on every body re-render —
    /// each read triggers iOS's "Podcastr accessed your pasteboard"
    /// privacy banner, which would fire per-keystroke if we recomputed
    /// inside the body.
    @State private var clipboardLooksLikeNsec = false
    @FocusState private var keyFocused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppTheme.Spacing.md) {
                preface

                Text("Your key is stored only in this device's iOS Keychain — the same place that holds Wi-Fi passwords. We never see it. We never send it anywhere.")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.secondary)

                sectionLabel("Your key")
                keyField
                if let inlineError {
                    HStack(alignment: .top, spacing: AppTheme.Spacing.xs) {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(AppTheme.Tint.error)
                        Text(inlineError)
                            .font(AppTheme.Typography.caption)
                            .foregroundStyle(AppTheme.Tint.error)
                    }
                }

                confirmCheckbox

                useButton
                    .padding(.top, AppTheme.Spacing.sm)

                Text("Don't have one? You don't need one — your existing account works fine.")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.tertiary)
                    .padding(.top, AppTheme.Spacing.sm)
            }
            .padding(AppTheme.Spacing.lg)
        }
        .navigationTitle("Use my own key")
        .navigationBarTitleDisplayMode(.inline)
        .background(Color(.systemBackground))
        .onAppear {
            keyFocused = true
            refreshClipboardCheck()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIPasteboard.changedNotification)) { _ in
            refreshClipboardCheck()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.willEnterForegroundNotification)) { _ in
            // Clipboard may have changed in another app while we were
            // backgrounded — re-detect when we come back.
            refreshClipboardCheck()
        }
    }

    // MARK: - Preface

    private var preface: some View {
        Text("If you already use an app like Damus, Amethyst, or Primal, you have a private key — it usually starts with `nsec1`. Paste it here and Podcastr will use the same account everywhere.")
            .font(AppTheme.Typography.body)
            .foregroundStyle(.primary)
    }

    // MARK: - Field

    @ViewBuilder
    private var keyField: some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            Group {
                if revealed {
                    TextField("nsec1…", text: $nsec, axis: .vertical)
                        .lineLimit(1...4)
                } else {
                    SecureField("nsec1…", text: $nsec)
                }
            }
            .font(AppTheme.Typography.monoCaption)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .focused($keyFocused)
            .dismissKeyboardToolbar()
            .frame(maxWidth: .infinity)

            Button {
                revealed.toggle()
            } label: {
                Image(systemName: revealed ? "eye.slash" : "eye")
            }
            .buttonStyle(.glass)
            .accessibilityLabel(revealed ? "Hide key" : "Show key")

            Button {
                paste()
            } label: {
                Image(systemName: "doc.on.clipboard")
            }
            .buttonStyle(.glass)
            .disabled(!clipboardLooksLikeNsec)
            .accessibilityLabel("Paste key from clipboard")
        }
        .padding(AppTheme.Spacing.sm)
        .overlay(
            RoundedRectangle(cornerRadius: AppTheme.Corner.sm)
                .stroke(AppTheme.Tint.hairline, lineWidth: 0.5)
        )
    }

    // MARK: - Checkbox

    private var confirmCheckbox: some View {
        Button {
            hasBackup.toggle()
            Haptics.selection()
        } label: {
            HStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: hasBackup ? "checkmark.square.fill" : "square")
                    .foregroundStyle(hasBackup ? Color.accentColor : .secondary)
                    .font(AppTheme.Typography.title3)
                Text("I have this key saved somewhere safe.")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.primary)
                Spacer()
            }
        }
        .buttonStyle(.plain)
        .padding(.top, AppTheme.Spacing.sm)
    }

    // MARK: - Button

    private var useButton: some View {
        Button {
            Task { await importNsec() }
        } label: {
            Text(importing ? "Importing…" : "Use this key")
                .font(AppTheme.Typography.headline)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 12)
        }
        .buttonStyle(.glassProminent)
        .opacity(canSubmit ? 1.0 : 0.4)
        .disabled(!canSubmit)
    }

    // MARK: - Logic

    private var canSubmit: Bool {
        !nsec.isBlank && hasBackup && !importing
    }

    private func refreshClipboardCheck() {
        // `hasStrings` is the only no-prompt check available — it tells
        // us whether *any* string is on the clipboard without exposing
        // the bytes. We still need to read `.string` to confirm the
        // `nsec1` prefix, but we do it once per change rather than per
        // body render. iOS surfaces a single "Pasted from..." banner the
        // first time we read after a clipboard change, which is the
        // expected behavior (the user just copied something; they're
        // about to paste).
        guard UIPasteboard.general.hasStrings else {
            clipboardLooksLikeNsec = false
            return
        }
        let candidate = UIPasteboard.general.string?.trimmed ?? ""
        clipboardLooksLikeNsec = candidate.hasPrefix("nsec1")
    }

    private func paste() {
        guard let s = UIPasteboard.general.string?.trimmed, s.hasPrefix("nsec1") else { return }
        nsec = s
        Haptics.light()
    }

    private func importNsec() async {
        guard canSubmit else { return }
        importing = true
        defer { importing = false }
        inlineError = nil
        do {
            try identity.importNsec(nsec.trimmed)
            Haptics.success()
            onImportComplete()
        } catch {
            // Translation of the existing `loginError` copy per §4.6.
            inlineError = "That key doesn't look right. Check the start (it should begin with nsec1) and try again."
            Haptics.error()
        }
    }

    // MARK: - Section helper

    @ViewBuilder
    private func sectionLabel(_ title: String) -> some View {
        Text(title)
            .font(AppTheme.Typography.caption2.weight(.semibold))
            .foregroundStyle(.tertiary)
            .textCase(.uppercase)
            .tracking(0.4)
            .padding(.top, AppTheme.Spacing.sm)
    }
}
