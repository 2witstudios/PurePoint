project := "apps/purepoint-macos/purepoint-macos.xcodeproj"
scheme := "purepoint-macos"
destination := "platform=macOS"

# One-time post-clone setup
setup:
    git config core.hooksPath .githooks
    chmod +x .githooks/*
    rustup show
    @if command -v swift-format &>/dev/null || xcrun --find swift-format &>/dev/null 2>&1; then \
        echo "swift-format: found"; \
    else \
        echo "swift-format: not found (optional — install with: brew install swift-format)"; \
    fi

# Format Rust code
fmt:
    cargo fmt --all

# Check Rust formatting (CI)
fmt-check:
    cargo fmt --all -- --check

# Format Swift code
fmt-swift:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v swift-format &>/dev/null; then
        SF="swift-format"
    elif xcrun --find swift-format &>/dev/null 2>&1; then
        SF="$(xcrun --find swift-format)"
    else
        echo "swift-format not found. Install: brew install swift-format" && exit 1
    fi
    find apps/ -name '*.swift' -print0 | xargs -0 "$SF" format -i

# Check Swift formatting (CI)
fmt-swift-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v swift-format &>/dev/null; then
        SF="swift-format"
    elif xcrun --find swift-format &>/dev/null 2>&1; then
        SF="$(xcrun --find swift-format)"
    else
        echo "swift-format not found. Install: brew install swift-format" && exit 1
    fi
    find apps/ -name '*.swift' -print0 | xargs -0 "$SF" lint --strict

# Run clippy lints
lint:
    RUSTFLAGS="-D warnings" cargo clippy --all-targets

# Run all Rust tests
test:
    cargo test --all-targets

# Run cargo-deny checks
deny:
    cargo deny check advisories licenses bans sources

# Full Rust CI locally
ci-rust: fmt-check lint test deny

# Build macOS app
build-app:
    xcodebuild build \
        -project "{{project}}" \
        -scheme "{{scheme}}" \
        -destination "{{destination}}" \
        CODE_SIGN_IDENTITY="-" \
        CODE_SIGNING_REQUIRED=NO \
        DEVELOPMENT_TEAM=""

# Run macOS tests
test-app:
    xcodebuild test \
        -project "{{project}}" \
        -scheme "{{scheme}}" \
        -destination "{{destination}}" \
        CODE_SIGN_IDENTITY="-" \
        CODE_SIGNING_REQUIRED=NO \
        DEVELOPMENT_TEAM=""

# Archive for release testing
archive:
    xcodebuild archive \
        -project "{{project}}" \
        -scheme "{{scheme}}" \
        -configuration Release \
        -archivePath build/PurePoint.xcarchive \
        2>&1 | xcbeautify

# Export archive for distribution (run after archive)
export:
    xcodebuild -exportArchive \
        -archivePath build/PurePoint.xcarchive \
        -exportPath build/export \
        -exportOptionsPlist apps/purepoint-macos/ExportOptions.plist \
        2>&1 | xcbeautify

# Run everything
ci: ci-rust build-app test-app
