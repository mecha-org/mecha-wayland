setup:
    ln -sf ../../scripts/pre-commit.sh .git/hooks/pre-commit
    @echo "pre-commit hook installed"

fmt:
    cargo fmt --all
    cargo fmt --all --manifest-path compositor/Cargo.toml
    cargo fmt --all --manifest-path launcher/Cargo.toml

fmt-check:
    cargo fmt --all --check
    cargo fmt --all --check --manifest-path compositor/Cargo.toml
    cargo fmt --all --check --manifest-path launcher/Cargo.toml

clippy:
    cargo clippy --workspace --all-targets --all-features
    cargo clippy --all-targets --all-features --manifest-path compositor/Cargo.toml
    cargo clippy --all-targets --all-features --manifest-path launcher/Cargo.toml

build:
    cargo build --workspace
    cargo build --manifest-path compositor/Cargo.toml
    cargo build --manifest-path launcher/Cargo.toml

test:
    cargo test --workspace
    cargo test --manifest-path compositor/Cargo.toml
    cargo test --manifest-path launcher/Cargo.toml

ci: fmt-check clippy build test
