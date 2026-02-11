/// Represents the state of a command's execution.
#[derive(Debug, Clone)]
pub enum CommandState {
    /// Command has not yet started.
    Pending,
    /// Command is currently running.
    Running,
    /// Command exited with a code.
    Exited { code: i32 },
    /// Command was killed by a signal.
    Killed { signal: String },
    /// Command encountered an error before or during execution.
    Errored { message: String },
}

/// Metadata and state for a single command being run.
#[derive(Debug)]
pub struct CommandInfo {
    /// Zero-based index of this command.
    pub index: usize,
    /// Display name (from --names, or defaults to the command string).
    pub name: String,
    /// The raw command line string.
    pub command_line: String,
    /// Current execution state.
    pub state: CommandState,
}

impl CommandInfo {
    /// Create a new command info.
    pub fn new(index: usize, name: String, command_line: String) -> Self {
        Self {
            index,
            name,
            command_line,
            state: CommandState::Pending,
        }
    }

    /// The exit code if the command has exited, otherwise None.
    pub fn exit_code(&self) -> Option<i32> {
        match &self.state {
            CommandState::Exited { code } => Some(*code),
            CommandState::Killed { .. } => Some(1),
            CommandState::Errored { .. } => Some(1),
            _ => None,
        }
    }
}

/// Creates a `tokio::process::Command` that runs the given command line through the shell.
///
/// On Unix, each command is spawned in its own process group (via `setsid`) so that
/// signals can be sent to the entire group, ensuring child processes are also killed.
pub fn shell_command(cmd: &str) -> tokio::process::Command {
    #[cfg(unix)]
    {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(cmd);
        // SAFETY: setsid() is async-signal-safe and has no side effects that
        // could corrupt state between fork and exec.
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        c
    }
    #[cfg(windows)]
    {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_info_new() {
        let cmd = CommandInfo::new(0, "test".to_string(), "echo hello".to_string());
        assert_eq!(cmd.index, 0);
        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.command_line, "echo hello");
        assert!(matches!(cmd.state, CommandState::Pending));
    }

    #[test]
    fn test_exit_code() {
        let mut cmd = CommandInfo::new(0, "test".to_string(), "echo hello".to_string());
        assert_eq!(cmd.exit_code(), None);

        cmd.state = CommandState::Running;
        assert_eq!(cmd.exit_code(), None);

        cmd.state = CommandState::Exited { code: 0 };
        assert_eq!(cmd.exit_code(), Some(0));

        cmd.state = CommandState::Exited { code: 42 };
        assert_eq!(cmd.exit_code(), Some(42));

        cmd.state = CommandState::Killed {
            signal: "SIGTERM".to_string(),
        };
        assert_eq!(cmd.exit_code(), Some(1));
    }
}
