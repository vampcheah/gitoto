run:
    cargo run

test:
    cargo test

fmt:
    cargo fmt --all

lint:
    cargo clippy --all-targets --all-features

screenshots:
    node scripts/screenshots.js

ci: fmt lint test
