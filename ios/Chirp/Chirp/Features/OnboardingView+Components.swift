import SwiftUI

extension OnboardingView {

    // MARK: — Welcome screen

    var welcomeScreen: some View {
        VStack(spacing: ChirpSpace.xl) {
            Spacer()

            logoBrand

            Spacer()

            VStack(spacing: ChirpSpace.l) {
                Button {
                    withAnimation(.smooth(duration: 0.35)) { mode = .create }
                } label: {
                    Label("Create account", systemImage: "person.badge.plus")
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 16)
                }
                .buttonStyle(ChirpGlassButtonStyle(prominent: true))
                .accessibilityIdentifier("onboarding-create-account-button")

                Button {
                    withAnimation(.smooth(duration: 0.35)) { mode = .signIn }
                } label: {
                    Label("I have an account", systemImage: "key.fill")
                        .font(.subheadline.weight(.medium))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                }
                .buttonStyle(ChirpGlassButtonStyle())
            }
            .padding(.horizontal, ChirpSpace.l)

            Spacer().frame(height: 48)
        }
    }

    // MARK: — Create account screen

    var createScreen: some View {
        VStack(spacing: 0) {
            HStack {
                Button("Back") {
                    withAnimation(.smooth(duration: 0.25)) { mode = .welcome }
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
                    .foregroundStyle(.tint)

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
                    let name = displayName.trimmingCharacters(in: .whitespaces)
                    let profile: [String: String] = name.isEmpty ? ["name": "Anonymous"] : ["name": name]
                    model.createAccount(profile: profile)
                } label: {
                    Label("Create account", systemImage: "arrow.right.circle.fill")
                        .font(.headline)
                        .frame(maxWidth: 280)
                        .padding(.vertical, 16)
                }
                .buttonStyle(ChirpGlassButtonStyle(prominent: true))
                .disabled(false) // always enabled, empty name → "Anonymous"
                .accessibilityIdentifier("onboarding-submit-create-account-button")
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.xl)
            .chirpGlass(cornerRadius: ChirpSpace.radius)
            .padding(.horizontal, ChirpSpace.l)

            Spacer()
        }
    }

    // MARK: — Sign-in screen

    var signInScreen: some View {
        VStack(spacing: 0) {
            HStack {
                Button("Back") {
                    withAnimation(.smooth(duration: 0.25)) { mode = .welcome }
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
                            model.signInNsec(nsec.trimmingCharacters(in: .whitespacesAndNewlines))
                        } label: {
                            Label("Sign in", systemImage: "key.fill")
                                .font(.headline)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(ChirpGlassButtonStyle(prominent: true))
                        .disabled(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    .padding(ChirpSpace.l)
                    .chirpGlass(cornerRadius: ChirpSpace.radius)
                    .padding(.horizontal, ChirpSpace.l)

                    Text("Or use a remote signer")
                        .font(.headline)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, ChirpSpace.l)

                    nip46SignerCard
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
                .foregroundStyle(.tint)

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
