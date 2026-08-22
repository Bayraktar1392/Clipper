# Contributing

## Local verification

```bash
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

GTK4 and libadwaita development packages are required for local builds.
