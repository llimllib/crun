.PHONY: check fmt clippy doc test build integration-test all

# Run all checks
all: check test

# All static checks (fast, no tests)
check: fmt clippy doc build

# Check formatting
fmt:
	cargo fmt --check

# Clippy with warnings as errors
clippy:
	cargo clippy -- -D warnings

# Check documentation builds cleanly
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --quiet

# Build with no warnings
build:
	RUSTFLAGS="-D warnings" cargo build

# Rust unit tests
test:
	cargo test

# Build release and run integration tests
integration-test:
	cargo build --release
	cd tests/integration && npm test

# Format code (fix, not just check)
fix-fmt:
	cargo fmt

# Clippy with auto-fix
fix-clippy:
	cargo clippy --fix --allow-dirty -- -D warnings
