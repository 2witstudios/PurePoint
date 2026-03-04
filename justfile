project := "apps/purepoint-macos/purepoint-macos.xcodeproj"
scheme := "purepoint-macos"
destination := "platform=macOS"

# One-time post-clone setup
setup:
    git config core.hooksPath .githooks
    rustup show

# Format Rust code
fmt:
    cargo fmt --all

# Check Rust formatting (CI)
fmt-check:
    cargo fmt --all -- --check

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

# Run everything
ci: ci-rust build-app test-app
