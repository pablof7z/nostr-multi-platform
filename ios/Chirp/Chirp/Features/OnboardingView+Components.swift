import SwiftUI

extension OnboardingView {

    // MARK: — Welcome screen

    var welcomeScreen: some View {
        VStack(spacing: ChirpSpace.xl) {
            Spacer()

            logoBrand
                .opacity(appeared ? 1 : 0)
                .offset(y: appeared ? 0 : 24)
                .animation(.smooth(duration: 0.55), value: appeared)

            Spacer()

            VStack(spacing: ChirpSpace.l) {
                Button {
                    mode = .create
                } label: {
                    Label("Create account", systemImage: "person.badge.plus")
                        .font(.headline)
                        .foregroundStyle(ChirpColor.onAccent)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 16)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("onboarding-create-account-button")

                Button {
                    mode = .signIn
                } label: {
                    Label("I have an account", systemImage: "key.fill")
                        .font(.subheadline.weight(.medium))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                }
                .buttonStyle(.bordered)
            }
            .padding(.horizontal, ChirpSpace.l)
            .opacity(appeared ? 1 : 0)
            .offset(y: appeared ? 0 : 16)
            .animation(.smooth(duration: 0.5).delay(0.15), value: appeared)

            Spacer().frame(height: 48)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            Text("\(BuildInfo.branch) · \(BuildInfo.commit) · \(BuildInfo.buildTime)")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .padding(.bottom, 12)
        }
    }

    // MARK: — Create account screen

    var createScreen: some View {
        VStack(spacing: 0) {
            HStack {
                Button("Back") {
                    mode = .welcome
                }
                .font(.subheadline)
                Spacer()
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.m)

            Spacer()

            VStack(spacing: ChirpSpace.xl) {
                Image(systemName: "person.badge.plus")
                    .font(.system(size: 40, weight: .medium))
                    .foregroundStyle(ChirpColor.accent)

                VStack(spacing: ChirpSpace.s) {
                    Text("Choose your display name")
                        .font(.headline)

                    Text("This is how others will see you on Nostr")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }

                TextField("Satoshi", text: $displayName)
                    .textFieldStyle(.roundedBorder)
                    .font(.body)
                    .textInputAutocapitalization(.words)
                    .frame(maxWidth: 280)
                    .focused($nameFieldFocused)
                    .onAppear { nameFieldFocused = true }
                    .accessibilityIdentifier("onboarding-display-name-field")

                Button {
                    guard !isCreatingAccount else { return }
                    isCreatingAccount = true
                    let name = displayName.trimmingCharacters(in: .whitespaces)
                    let profile: [String: String] = name.isEmpty ? ["name": "Anonymous"] : ["name": name]
                    model.createAccount(profile: profile)
                } label: {
                    Label("Create account", systemImage: "arrow.right.circle.fill")
                        .font(.headline)
                        .foregroundStyle(ChirpColor.onAccent)
                        .frame(maxWidth: 280)
                        .padding(.vertical, 16)
                }
                .buttonStyle(.borderedProminent)
                .disabled(isCreatingAccount)
                .accessibilityIdentifier("onboarding-submit-create-account-button")
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.xl)

            Spacer()
        }
    }

    // MARK: — Sign-in screen

    var signInScreen: some View {
        VStack(spacing: 0) {
            HStack {
                Button("Back") {
                    mode = .welcome
                }
                .font(.subheadline)
                Spacer()
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.m)

            ScrollView {
                VStack(spacing: ChirpSpace.xl) {
                    VStack(alignment: .leading, spacing: ChirpSpace.m) {
                        Text("Paste your private key")
                            .font(.headline)

                        SecureField("nsec1…", text: $nsec)
                            .font(ChirpFont.mono)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .focused($nsecFieldFocused)

                        Button {
                            model.addSigner(localNsec: nsec.trimmingCharacters(in: .whitespacesAndNewlines), makeActive: true)
                        } label: {
                            Label("Sign in", systemImage: "key.fill")
                                .font(.headline)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    .padding(ChirpSpace.l)

                    Text("Or use a remote signer")
                        .font(.headline)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, ChirpSpace.l)

                    nip46SignerSection
                }
                .padding(.vertical, ChirpSpace.l)
            }
        }
    }

    // MARK: — Logo + brand

    var logoBrand: some View {
        VStack(spacing: ChirpSpace.m) {
            Image(systemName: "bird.fill")
                .font(.system(size: 48, weight: .medium))
                .foregroundStyle(ChirpColor.accent)

            VStack(spacing: ChirpSpace.xs) {
                Text("Chirp")
                    .font(.largeTitle.weight(.bold))

                Text("A Nostr client for iOS")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
