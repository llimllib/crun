# crun

A Rust port of [concurrently](https://github.com/open-cli-tools/concurrently). The original JS source is checked out in `concurrently/` for reference.

## Before Every Commit

All checks must pass:

```bash
make check        # fmt, clippy, doc, build — all with warnings as errors
make test         # Rust unit tests
```

Do not commit with warnings, lint failures, or failing unit tests.

## Integration Tests

```bash
make integration-test   # builds release binary, runs JS test suite
```

Integration tests are expected to fail until features are implemented. Run them to check progress, but they are not required to pass before committing — only the checks in `make check` and `make test` are.

## Project Layout

- `src/` — Rust source
- `tests/integration/` — JS integration tests (vitest), adapted from concurrently's `bin/index.spec.ts`
- `tests/integration/fixtures/` — helper scripts used by integration tests
- `docs/plans/` — implementation plans
- `concurrently/` — reference checkout of the original JS project (gitignored)

## Implementation Plan

See `docs/plans/2026-02-11-implementation-plan.md` for the phased implementation plan.

## Ask Before Changing

Always ask for explicit approval before:

- Modifying integration tests (`tests/integration/`)
- Adding, removing, or changing dependencies in `Cargo.toml`

## Style

- Follow standard `rustfmt` formatting (no custom config)
- Prefer explicit error handling with `anyhow`/`thiserror` over `.unwrap()`
- Keep modules focused: one concern per file
- Add doc comments to all public items

## Cross-Platform Support

Code must work on both Unix (Linux/macOS) and Windows. Key differences:

- **Shell invocation**: Unix uses `/bin/sh -c`, Windows uses `cmd /C` (see `command.rs`)
- **Process groups**: Unix uses `setsid()` + `kill(-pid, signal)`, Windows uses `taskkill /T /F`
- **Signals**: Unix has SIGINT/SIGTERM/SIGHUP, Windows only has Ctrl+C (`tokio::signal::ctrl_c()`)
- **`libc` crate**: Unix-only dependency; use `#[cfg(unix)]` guards

When adding process management code:
1. Use `#[cfg(unix)]` and `#[cfg(windows)]` blocks
2. Test compiles for Windows: `cargo check --target x86_64-pc-windows-msvc`
3. Refer to `concurrently/` source to see how the JS version handles cross-platform issues (they use the `tree-kill` npm package which calls `taskkill` on Windows)
