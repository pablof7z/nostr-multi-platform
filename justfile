set shell := ["zsh", "-cu"]

rust-test:
    cargo test --workspace

rust-ios-sim:
    # Keep the standalone core archive fresh for shells that link nmp-core
    # directly.
    cargo build -p nmp-core --features lmdb-backend --target aarch64-apple-ios-sim
    # NmpGallery links one aggregate archive so nmp-core static state is not
    # duplicated across framework, projection, and bridge crates.
    cargo build -p nmp-app-gallery --target aarch64-apple-ios-sim

rust-ios-device:
    # Release build required — pbxproj LIBRARY_SEARCH_PATHS points at the
    # release archive. IPHONEOS_DEPLOYMENT_TARGET=17.0 avoids the
    # ___chkstk_darwin linker error introduced by Xcode 26.
    IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p nmp-core --features lmdb-backend --target aarch64-apple-ios --release
    IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p nmp-app-gallery --target aarch64-apple-ios --release

gen-ios:
    xcodegen generate --spec apps/nmp-gallery/ios/project.yml

build-ios: rust-ios-sim gen-ios
    xcodebuild -project apps/nmp-gallery/ios/NmpGallery.xcodeproj -scheme NmpGallery -destination 'platform=iOS Simulator,name=iPhone 17,OS=26.5' -derivedDataPath apps/nmp-gallery/ios/DerivedData build

run-ios: build-ios
    xcrun simctl install booted apps/nmp-gallery/ios/DerivedData/Build/Products/Debug-iphonesimulator/NmpGallery.app
    xcrun simctl launch booted org.nmp.gallery
