use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "crun")]
#[command(author, version, about = "Run commands concurrently", long_about = None)]
#[command(after_help = "For documentation, visit: https://github.com/llimllib/crun")]
pub struct Args {
    /// Commands to run concurrently
    #[arg(required = false)]
    pub commands: Vec<String>,

    /// Additional arguments passed after `--`.
    /// With `-P`, these are used for placeholder substitution.
    /// Without `-P`, these are treated as additional commands.
    #[arg(last = true)]
    pub extra_args: Vec<String>,

    // ─── General ─────────────────────────────────────────────────────────────
    /// How many processes should run at once.
    /// Exact number or a percent of CPUs available (e.g. "50%")
    #[arg(short = 'm', long = "max-processes")]
    pub max_processes: Option<String>,

    /// List of custom names to be used in prefix template.
    /// Example: "main,browser,server"
    #[arg(short = 'n', long = "names")]
    pub names: Option<String>,

    /// The character to split names on.
    #[arg(long = "name-separator", default_value = ",")]
    pub name_separator: String,

    /// Which command(s) must exit with code 0 for concurrently to exit with code 0.
    /// Options: "first", "last", "all", "command-{name}", "command-{index}"
    #[arg(short = 's', long = "success", default_value = "all")]
    pub success: String,

    /// Output only raw output of processes, disables prettifying and coloring.
    #[arg(short = 'r', long = "raw")]
    pub raw: bool,

    /// Disables colors from logging
    #[arg(long = "no-color")]
    pub no_color: bool,

    /// Comma-separated list of processes to hide the output.
    /// The processes can be identified by their name or index.
    #[arg(long = "hide", default_value = "")]
    pub hide: String,

    /// Order the output as if the commands were run sequentially.
    #[arg(short = 'g', long = "group")]
    pub group: bool,

    /// Show timing information for all processes.
    #[arg(long = "timings")]
    pub timings: bool,

    /// Passthrough additional arguments to commands (accessible via placeholders)
    /// instead of treating them as commands.
    #[arg(short = 'P', long = "passthrough-arguments")]
    pub passthrough_arguments: bool,

    /// Clean up command(s) to execute before exiting concurrently.
    #[arg(long = "teardown")]
    pub teardown: Vec<String>,

    // ─── Kill others ─────────────────────────────────────────────────────────
    /// Kill other processes once the first exits.
    #[arg(short = 'k', long = "kill-others")]
    pub kill_others: bool,

    /// Kill other processes if one exits with non zero status code.
    #[arg(long = "kill-others-on-fail")]
    pub kill_others_on_fail: bool,

    /// Signal to send to other processes if one exits or dies.
    #[arg(long = "kill-signal", default_value = "SIGTERM")]
    pub kill_signal: String,

    /// How many milliseconds to wait before forcing process termination.
    #[arg(long = "kill-timeout")]
    pub kill_timeout: Option<u64>,

    // ─── Prefix styling ──────────────────────────────────────────────────────
    /// Prefix used in logging for each process.
    /// Possible values: index, pid, time, command, name, none, or a template.
    #[arg(short = 'p', long = "prefix")]
    pub prefix: Option<String>,

    /// Comma-separated list of colors to use on prefixes.
    #[arg(short = 'c', long = "prefix-colors", default_value = "")]
    pub prefix_colors: String,

    /// Limit how many characters of the command is displayed in prefix.
    #[arg(short = 'l', long = "prefix-length", default_value = "10")]
    pub prefix_length: usize,

    /// Pads short prefixes with spaces so that the length of all prefixes match.
    #[arg(long = "pad-prefix")]
    pub pad_prefix: bool,

    /// Specify the timestamp in Unicode format.
    #[arg(
        short = 't',
        long = "timestamp-format",
        default_value = "yyyy-MM-dd HH:mm:ss.SSS"
    )]
    pub timestamp_format: String,

    // ─── Restarting ──────────────────────────────────────────────────────────
    /// How many times a process that died should restart.
    /// Negative numbers will make the process restart forever.
    #[arg(long = "restart-tries", default_value = "0")]
    pub restart_tries: i32,

    /// Delay before restarting the process, in milliseconds, or "exponential".
    #[arg(long = "restart-after", default_value = "0")]
    pub restart_after: String,

    // ─── Input handling ──────────────────────────────────────────────────────
    /// Whether input should be forwarded to the child processes.
    #[arg(short = 'i', long = "handle-input")]
    pub handle_input: bool,

    /// Identifier for child process to which input on stdin should be sent
    /// if not specified at start of input.
    #[arg(long = "default-input-target", default_value = "0")]
    pub default_input_target: String,
}

#[allow(dead_code)]
impl Args {
    /// Parse the names string into a vector of names
    pub fn get_names(&self) -> Vec<String> {
        match &self.names {
            Some(names) => names
                .split(&self.name_separator)
                .map(|s| s.to_string())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Parse the hide string into a vector of identifiers
    pub fn get_hide(&self) -> Vec<String> {
        if self.hide.is_empty() {
            Vec::new()
        } else {
            self.hide.split(',').map(|s| s.to_string()).collect()
        }
    }

    /// Get the final list of commands to run.
    /// If `-P` is enabled, commands are the original commands with placeholders expanded.
    /// If `-P` is disabled, extra_args are appended as additional commands.
    pub fn get_commands(&self) -> Vec<String> {
        if self.passthrough_arguments {
            // Expand placeholders in commands using extra_args
            self.commands
                .iter()
                .map(|cmd| expand_arguments(cmd, &self.extra_args))
                .collect()
        } else {
            // Treat extra_args as additional commands
            let mut cmds = self.commands.clone();
            cmds.extend(self.extra_args.clone());
            cmds
        }
    }
}

/// Expand argument placeholders in a command string.
///
/// Supported placeholders:
/// - `{1}`, `{2}`, etc. - individual positional arguments (1-indexed)
/// - `{@}` - all arguments, space-separated
/// - `{*}` - all arguments joined as a single quoted string
///
/// Placeholders can be escaped with a backslash: `\{1}` becomes `{1}`.
pub fn expand_arguments(command: &str, args: &[String]) -> String {
    let mut result = String::with_capacity(command.len());
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Check for escaped placeholder: \{...}
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '{' {
            // Look for the closing brace to see if this is a valid placeholder
            if let Some(placeholder_end) = find_placeholder_end(&chars, i + 1) {
                let placeholder_content: String = chars[i + 2..placeholder_end].iter().collect();
                if is_valid_placeholder(&placeholder_content) {
                    // Valid escaped placeholder - output the placeholder literally (without backslash)
                    result.push('{');
                    result.push_str(&placeholder_content);
                    result.push('}');
                    i = placeholder_end + 1;
                    continue;
                }
            }
            // Not a valid placeholder pattern, output the backslash
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Check for placeholder: {...}
        if chars[i] == '{' {
            if let Some(placeholder_end) = find_placeholder_end(&chars, i) {
                let placeholder_content: String = chars[i + 1..placeholder_end].iter().collect();
                if is_valid_placeholder(&placeholder_content) {
                    // Replace the placeholder
                    let replacement = replace_placeholder(&placeholder_content, args);
                    result.push_str(&replacement);
                    i = placeholder_end + 1;
                    continue;
                }
            }
        }

        // Regular character
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Find the position of the closing brace for a placeholder starting at `start`.
/// Returns None if no closing brace is found.
fn find_placeholder_end(chars: &[char], start: usize) -> Option<usize> {
    if chars[start] != '{' {
        return None;
    }
    for (j, &ch) in chars.iter().enumerate().skip(start + 1) {
        if ch == '}' {
            return Some(j);
        }
        // If we hit another '{' or invalid char before '}', it's not a valid placeholder
        if ch == '{' {
            return None;
        }
    }
    None
}

/// Check if the placeholder content is valid.
/// Valid: "@", "*", or a positive integer starting with 1-9.
fn is_valid_placeholder(content: &str) -> bool {
    if content == "@" || content == "*" {
        return true;
    }
    // Must be a positive integer starting with 1-9
    if content.is_empty() {
        return false;
    }
    let first_char = content.chars().next().unwrap();
    if !('1'..='9').contains(&first_char) {
        return false;
    }
    content.chars().all(|c| c.is_ascii_digit())
}

/// Replace a placeholder with its value from args.
fn replace_placeholder(placeholder: &str, args: &[String]) -> String {
    if args.is_empty() {
        return String::new();
    }

    match placeholder {
        "@" => {
            // All arguments, each quoted if necessary, space-separated
            args.iter()
                .map(|a| shell_quote(a))
                .collect::<Vec<_>>()
                .join(" ")
        }
        "*" => {
            // All arguments joined with space, then quoted as a single string
            shell_quote(&args.join(" "))
        }
        n => {
            // Numeric placeholder (1-indexed)
            if let Ok(index) = n.parse::<usize>() {
                if index > 0 && index <= args.len() {
                    shell_quote(&args[index - 1])
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        }
    }
}

/// Quote a string for shell if it contains special characters.
/// Uses single quotes, escaping any embedded single quotes.
fn shell_quote(s: &str) -> String {
    // If the string contains no special characters, return as-is
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return s.to_string();
    }

    // Use single quotes, escaping any embedded single quotes
    // 'foo' -> 'foo'
    // "foo's bar" -> 'foo'"'"'s bar'
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_no_placeholders() {
        assert_eq!(expand_arguments("echo foo", &["bar".into()]), "echo foo");
    }

    #[test]
    fn test_expand_single_placeholder() {
        assert_eq!(expand_arguments("echo {1}", &["foo".into()]), "echo foo");
    }

    #[test]
    fn test_expand_placeholder_with_space() {
        assert_eq!(
            expand_arguments("echo {1}", &["foo bar".into()]),
            "echo 'foo bar'"
        );
    }

    #[test]
    fn test_expand_multiple_placeholders() {
        assert_eq!(
            expand_arguments("echo {2} {1}", &["foo".into(), "bar".into()]),
            "echo bar foo"
        );
    }

    #[test]
    fn test_expand_missing_placeholder() {
        assert_eq!(
            expand_arguments("echo {3}", &["foo".into(), "bar".into()]),
            "echo "
        );
    }

    #[test]
    fn test_expand_all_placeholder_empty() {
        let empty: Vec<String> = vec![];
        assert_eq!(expand_arguments("echo {@}", &empty), "echo ");
    }

    #[test]
    fn test_expand_combined_placeholder_empty() {
        let empty: Vec<String> = vec![];
        assert_eq!(expand_arguments("echo {*}", &empty), "echo ");
    }

    #[test]
    fn test_expand_all_placeholder() {
        assert_eq!(
            expand_arguments("echo {@}", &["foo".into(), "bar".into()]),
            "echo foo bar"
        );
    }

    #[test]
    fn test_expand_combined_placeholder() {
        assert_eq!(
            expand_arguments("echo {*}", &["foo".into(), "bar".into()]),
            "echo 'foo bar'"
        );
    }

    #[test]
    fn test_expand_escaped_placeholders() {
        assert_eq!(
            expand_arguments(r"echo \{1} \{@} \{*}", &["foo".into(), "bar".into()]),
            "echo {1} {@} {*}"
        );
    }

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("foo"), "foo");
        assert_eq!(shell_quote("foo-bar"), "foo-bar");
        assert_eq!(shell_quote("foo_bar"), "foo_bar");
    }

    #[test]
    fn test_shell_quote_with_space() {
        assert_eq!(shell_quote("foo bar"), "'foo bar'");
    }

    #[test]
    fn test_shell_quote_with_single_quote() {
        assert_eq!(shell_quote("foo's"), "'foo'\"'\"'s'");
    }
}
