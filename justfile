# Run all tasks
all:
    just fmt clippy

# Format the code
fmt:
    cargo fmt --all

# Run Clippy for linting
clippy:
    cargo clippy --all -- -D warnings
