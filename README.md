# crun

A Rust port of [concurrently](https://github.com/open-cli-tools/concurrently) - run commands concurrently.

## Installation

```bash
cargo install crun
```

## Usage

```bash
crun "echo foo" "echo bar"
```

## Development

### Prerequisites

- Rust (latest stable)
- Node.js 20+ (for integration tests)

### Building

```bash
cargo build --release
```

### Running Tests

Integration tests use the same test harness as the original concurrently project:

```bash
# Install test dependencies
cd tests/integration && npm install

# Run tests
npm test

# Or from root
npm test
```

### Running Unit Tests

```bash
cargo test
```

### Make a release

```bash
make release
```

## License

MIT
