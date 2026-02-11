# cancurrently

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

## Style

- Follow standard `rustfmt` formatting (no custom config)
- Prefer explicit error handling with `anyhow`/`thiserror` over `.unwrap()`
- Keep modules focused: one concern per file
- Add doc comments to all public items
