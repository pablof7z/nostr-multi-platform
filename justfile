set shell := ["zsh", "-cu"]

rust-test:
    cargo test --workspace

gen-modules:
    cargo run -p nmp-codegen -- gen modules --manifest apps/fixture/nmp.toml --out apps/fixture/nmp-app-fixture

gen-modules-check:
    cargo run -p nmp-codegen -- gen modules --manifest apps/fixture/nmp.toml --out apps/fixture/nmp-app-fixture --check

rust-ios-sim:
    cargo build -p nmp-core --target aarch64-apple-ios-sim

gen-ios:
    xcodegen generate --spec ios/NmpStress/project.yml

build-ios: rust-ios-sim gen-ios
    xcodebuild -project ios/NmpStress/NmpStress.xcodeproj -scheme NmpStress -destination 'platform=iOS Simulator,name=iPhone 17,OS=26.5' -derivedDataPath ios/DerivedData build

run-ios: build-ios
    xcrun simctl install booted ios/DerivedData/Build/Products/Debug-iphonesimulator/NmpStress.app
    xcrun simctl launch booted com.example.NmpStress
