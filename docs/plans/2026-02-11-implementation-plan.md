# Implementation Plan: crun

## Overview

This document outlines the implementation plan for porting `concurrently` from JavaScript to Rust. The goal is to create a drop-in replacement that passes the same integration tests as the original.

## Architecture

### Core Components

```
src/
├── main.rs           # Entry point, CLI parsing
├── cli.rs            # Argument definitions (done)
├── runner.rs         # Orchestrates command execution
├── command.rs        # Individual command state and lifecycle
├── output.rs         # Output formatting, prefixing, colors
├── input.rs          # Stdin forwarding logic
└── signal.rs         # Signal handling (SIGTERM, SIGINT, etc.)
```

### Key Data Structures

```rust
// A command to be executed
struct Command {
    index: usize,
    name: String,
    command_line: String,
    process: Option<Child>,
    state: CommandState,
    started_at: Option<Instant>,
    ended_at: Option<Instant>,
}

enum CommandState {
    Pending,
    Running,
    Exited { code: i32 },
    Killed { signal: String },
    Errored { message: String },
}

// Result of running all commands
struct RunResult {
    commands: Vec<CommandResult>,
    success: bool,
}
```

---

## Implementation Phases

### Phase 1: Basic Command Execution (Tests: 5-10 passing)

**Goal:** Run commands concurrently, capture output, return correct exit codes.

**Files to modify/create:**
- `src/runner.rs` - Main execution loop
- `src/command.rs` - Command struct and spawning

**Implementation:**
1. Parse command strings and spawn child processes using `tokio::process::Command`
2. Capture stdout/stderr from each process
3. Wait for all processes to complete
4. Return exit code based on `--success` flag:
   - `all`: exit 0 only if all commands exit 0
   - `first`: exit code of first command to finish
   - `last`: exit code of last command to finish

**Tests that should pass:**
- `is of success by default when running successful commands`
- `is of failure by default when one of the command fails`
- `--success=first` tests
- `--success=last` tests

---

### Phase 2: Output Prefixing (Tests: 15-20 passing)

**Goal:** Add prefixes to command output lines.

**Files to modify/create:**
- `src/output.rs` - Prefix formatting logic

**Implementation:**
1. Create `OutputWriter` that wraps stdout
2. Implement prefix types:
   - `index` (default): `[0]`, `[1]`, etc.
   - `name`: `[foo]`, `[bar]` (from `--names`)
   - `command`: `[echo foo]` (truncated per `--prefix-length`)
   - `pid`: `[12345]`
   - `time`: `[2024-01-01 12:00:00.000]`
   - `none`: no prefix
   - Template: `[{time} {pid}]`
3. Implement `--prefix-length` truncation with `..` in middle
4. Implement `--pad-prefix` to align all prefixes

**Tests that should pass:**
- `--names` / `-n` tests
- `--prefix` / `-p` tests
- `--prefix-length` / `-l` tests
- `--pad-prefix` tests
- `--name-separator` test

---

### Phase 3: Raw Mode and Hide (Tests: 20-25 passing)

**Goal:** Support `--raw` mode and `--hide` flag.

**Implementation:**
1. `--raw`: Bypass prefix formatting, output directly
2. `--hide`: Filter output from specified commands (by index or name)
3. Both can be combined

**Tests that should pass:**
- `--raw` / `-r` tests
- `--hide` tests (by index and by name)

---

### Phase 4: Kill Others (Tests: 25-30 passing)

**Goal:** Implement `--kill-others` and `--kill-others-on-fail`.

**Files to modify/create:**
- `src/signal.rs` - Signal sending utilities

**Implementation:**
1. When a command exits:
   - `--kill-others`: Kill all other running commands
   - `--kill-others-on-fail`: Kill others only if exit code != 0
2. Send configurable signal (`--kill-signal`, default SIGTERM)
3. Log "Sending SIGTERM to other processes.."
4. Implement `--kill-timeout` for forced termination

**Platform considerations:**
- Unix: Use `kill()` with signal
- Windows: Use `TerminateProcess()` (no signal support)

**Tests that should pass:**
- `--kill-others` / `-k` tests
- `--kill-others-on-fail` tests

---

### Phase 5: Restart Logic (Tests: 30-32 passing)

**Goal:** Implement `--restart-tries` and `--restart-after`.

**Implementation:**
1. Track restart count per command
2. On non-zero exit, if restarts remaining:
   - Log "[0] command restarted"
   - Wait `--restart-after` milliseconds (or exponential backoff)
   - Spawn command again
3. Negative `--restart-tries` means infinite restarts

**Tests that should pass:**
- `--restart-tries` test

---

### Phase 6: Input Handling (Tests: 32-36 passing)

**Goal:** Forward stdin to child processes.

**Files to modify/create:**
- `src/input.rs` - Stdin routing logic

**Implementation:**
1. When `--handle-input` / `-i` is set:
   - Read lines from stdin
   - Route to `--default-input-target` (default: 0)
2. Support target prefixing: `1:message` sends to command 1
3. Combine with `--kill-others` for full functionality

**Tests that should pass:**
- `--handle-input` / `-i` tests
- `--default-input-target` test
- Target prefix routing test

---

### Phase 7: Grouped Output (Tests: 36-37 passing)

**Goal:** Implement `--group` for sequential output.

**Implementation:**
1. Buffer output per command
2. Only flush a command's buffer when it completes
3. Flush in order of completion

**Tests that should pass:**
- `--group` test

---

### Phase 8: Timings (Tests: 37-39 passing)

**Goal:** Implement `--timings` for performance information.

**Implementation:**
1. Track start/end time per command
2. On start: `[0] command started at 2024-01-01 12:00:00.000`
3. On end: `[0] command stopped at 2024-01-01 12:00:01.000 after 1,000ms`
4. After all complete, print summary table:
   ```
   ┌──────┬──────────┬───────────┬────────┬─────────┐
   │ name │ duration │ exit code │ killed │ command │
   ├──────┼──────────┼───────────┼────────┼─────────┤
   │ 0    │ 1.00s    │ 0         │ false  │ echo hi │
   └──────┴──────────┴───────────┴────────┴─────────┘
   ```

**Tests that should pass:**
- `--timings` tests (success and failure)

---

### Phase 9: Teardown (Tests: 39-40 passing)

**Goal:** Implement `--teardown` for cleanup commands.

**Implementation:**
1. After all main commands complete (regardless of success):
   - Log `--> Running teardown command "..."`
   - Run each teardown command sequentially
   - Log `--> Teardown command "..." exited with code X`
2. Teardown exit codes don't affect main exit code

**Tests that should pass:**
- `--teardown` tests (single and multiple)

---

### Phase 10: Passthrough Arguments (Tests: 40 passing)

**Goal:** Implement `--passthrough-arguments` / `-P`.

**Implementation:**
1. When enabled, arguments after `--` are not treated as commands
2. Replace placeholders in commands:
   - `{1}`, `{2}`, etc.: Individual arguments
   - `{@}`: All arguments space-separated
   - `{*}`: All arguments as single quoted string
   - `\{@}`: Escaped, becomes literal `{@}`

**Tests that should pass:**
- `--passthrough-arguments` tests

---

## Dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive", "env"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
thiserror = "1"
chrono = "0.4"              # Timestamp formatting
shell-words = "1"           # Command line parsing (or shell-quote equivalent)

# Optional, for colors
termcolor = "1"             # Or use `owo-colors` / `colored`
```

---

## Testing Strategy

### Running Tests

```bash
# Run all integration tests
cd tests/integration && npm test

# Run specific test
cd tests/integration && npx vitest run -t "has help command"

# Watch mode during development
cd tests/integration && npm run test:watch
```

### Test-Driven Development

For each phase:
1. Run tests, note which ones fail
2. Implement feature
3. Run tests, verify expected tests now pass
4. Refactor if needed
5. Move to next phase

### Additional Rust Unit Tests

Add unit tests in Rust for:
- Command line parsing edge cases
- Prefix truncation logic
- Placeholder replacement
- Signal handling (mocked)

---

## Platform Considerations

### Unix vs Windows

| Feature | Unix | Windows |
|---------|------|---------|
| Signals | SIGTERM, SIGINT, SIGKILL | TerminateProcess only |
| Process groups | `setpgid`, `killpg` | Job objects |
| Shell | `/bin/sh -c` | `cmd /C` |
| Exit codes | 0-255, signals add 128 | 32-bit values |

### Shell Execution

Commands should be executed through the shell to support:
- Pipes: `echo foo | grep foo`
- Redirects: `echo foo > file.txt`
- Chaining: `echo foo && echo bar`
- Environment variable expansion: `echo $HOME`

```rust
#[cfg(unix)]
fn shell_command(cmd: &str) -> Command {
    let mut c = Command::new("/bin/sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(windows)]
fn shell_command(cmd: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(cmd);
    c
}
```

---

## Milestones

| Milestone | Tests Passing | Key Features |
|-----------|---------------|--------------|
| M1 | 10 | Basic execution, exit codes |
| M2 | 20 | Output prefixing, names |
| M3 | 25 | Raw mode, hide |
| M4 | 30 | Kill others |
| M5 | 35 | Restart, input handling |
| M6 | 40 | Group, timings, teardown, passthrough |

---

## Future Enhancements (Post-MVP)

1. **Color support**: Implement `--prefix-colors` with full chalk compatibility
2. **npm: prefix**: Support `npm:watch-*` style patterns
3. **Config file**: Support `.concurrentlyrc` or similar
4. **IPC**: Inter-process communication between commands
5. **Max processes**: Implement `--max-processes` with queue
6. **Better shell parsing**: Handle complex quoting and escaping

---

## References

- Original concurrently: https://github.com/open-cli-tools/concurrently
- Tokio process: https://docs.rs/tokio/latest/tokio/process/index.html
- Signal handling in Rust: https://docs.rs/signal-hook/latest/signal_hook/
