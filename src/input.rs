use std::collections::HashMap;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::mpsc;

/// Message to send to a child process's stdin.
#[derive(Debug)]
pub struct InputMessage {
    /// Target command index.
    pub target: usize,
    /// The line to send (without newline).
    pub line: String,
}

/// Parse a line of input, extracting the target prefix if present.
/// Format: `N:message` routes to command N, otherwise uses default target.
pub fn parse_input_line(line: &str, default_target: usize) -> InputMessage {
    // Check for `N:` prefix where N is a number
    if let Some(colon_pos) = line.find(':') {
        let prefix = &line[..colon_pos];
        if let Ok(target) = prefix.parse::<usize>() {
            return InputMessage {
                target,
                line: line[colon_pos + 1..].to_string(),
            };
        }
    }
    InputMessage {
        target: default_target,
        line: line.to_string(),
    }
}

/// Spawn a task that reads from parent stdin and routes lines to child processes.
pub fn spawn_input_router(
    default_target: usize,
    stdin_senders: HashMap<usize, mpsc::UnboundedSender<String>>,
) {
    tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let msg = parse_input_line(&line, default_target);
            if let Some(sender) = stdin_senders.get(&msg.target) {
                let _ = sender.send(msg.line);
            }
        }
    });
}

/// Spawn a task that forwards lines from a channel to a child's stdin.
pub fn spawn_stdin_writer(
    mut child_stdin: ChildStdin,
    mut receiver: mpsc::UnboundedReceiver<String>,
) {
    tokio::spawn(async move {
        while let Some(line) = receiver.recv().await {
            let data = format!("{}\n", line);
            if child_stdin.write_all(data.as_bytes()).await.is_err() {
                break;
            }
            if child_stdin.flush().await.is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_input_line_with_prefix() {
        let msg = parse_input_line("1:stop", 0);
        assert_eq!(msg.target, 1);
        assert_eq!(msg.line, "stop");
    }

    #[test]
    fn test_parse_input_line_without_prefix() {
        let msg = parse_input_line("hello", 0);
        assert_eq!(msg.target, 0);
        assert_eq!(msg.line, "hello");
    }

    #[test]
    fn test_parse_input_line_default_target() {
        let msg = parse_input_line("hello", 2);
        assert_eq!(msg.target, 2);
        assert_eq!(msg.line, "hello");
    }

    #[test]
    fn test_parse_input_line_non_numeric_prefix() {
        let msg = parse_input_line("foo:bar", 0);
        assert_eq!(msg.target, 0);
        assert_eq!(msg.line, "foo:bar");
    }
}
