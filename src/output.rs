use crate::command::CommandInfo;

/// The type of prefix to display before each output line.
#[derive(Debug, Clone)]
pub enum PrefixStyle {
    /// No prefix.
    None,
    /// Zero-based index: `[0]`, `[1]`, etc.
    Index,
    /// Process ID: `[12345]`.
    Pid,
    /// The command string: `[echo foo]`.
    Command,
    /// Custom name from `--names`: `[foo]`.
    Name,
    /// Timestamp.
    Time,
}

impl PrefixStyle {
    /// Parse a prefix style from the CLI `--prefix` argument.
    pub fn from_arg(arg: Option<&str>, has_names: bool) -> Self {
        match arg {
            Some("none") => PrefixStyle::None,
            Some("index") => PrefixStyle::Index,
            Some("pid") => PrefixStyle::Pid,
            Some("command") => PrefixStyle::Command,
            Some("name") => PrefixStyle::Name,
            Some("time") => PrefixStyle::Time,
            // Default: if names are provided, use name; otherwise index
            None => {
                if has_names {
                    PrefixStyle::Name
                } else {
                    PrefixStyle::Index
                }
            }
            // Anything else is treated as a template (future)
            Some(_) => PrefixStyle::Index,
        }
    }
}

/// Compute the prefix string for a command.
pub fn format_prefix(
    cmd: &CommandInfo,
    style: &PrefixStyle,
    pid: Option<u32>,
    prefix_length: usize,
) -> String {
    let inner = match style {
        PrefixStyle::None => return String::new(),
        PrefixStyle::Index => cmd.index.to_string(),
        PrefixStyle::Pid => pid.map_or("?".to_string(), |p| p.to_string()),
        PrefixStyle::Command => truncate_middle(&cmd.command_line, prefix_length),
        PrefixStyle::Name => cmd.name.clone(),
        PrefixStyle::Time => {
            // Simple timestamp - could be enhanced later
            chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
        }
    };
    format!("[{}]", inner)
}

/// Truncate a string in the middle, replacing the middle with `..`.
/// E.g., `truncate_middle("echo foo", 5)` => `"ec..o"`.
/// If the string is already short enough, return it as-is.
pub fn truncate_middle(s: &str, max_len: usize) -> String {
    if s.len() <= max_len || max_len < 4 {
        return s.to_string();
    }
    // We need at least 4 chars: 1 start + ".." + 1 end
    let available = max_len - 2; // subtract 2 for ".."
                                 // Favor giving more chars to the start
    let end_len = available / 2;
    let start_len = available - end_len;
    format!("{}..{}", &s[..start_len], &s[s.len() - end_len..])
}

/// Pad a prefix to a given width (for `--pad-prefix`).
pub fn pad_prefix(prefix: &str, width: usize) -> String {
    if prefix.is_empty() {
        return String::new();
    }
    // The prefix is `[content]`, we want to pad the content
    // Strip brackets, pad, re-add brackets
    if let Some(inner) = prefix.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        format!("[{:width$}]", inner, width = width)
    } else {
        prefix.to_string()
    }
}

/// Calculate the max inner width across all prefixes for padding.
pub fn max_prefix_inner_width(prefixes: &[String]) -> usize {
    prefixes
        .iter()
        .filter_map(|p| {
            p.strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .map(|inner| inner.len())
        })
        .max()
        .unwrap_or(0)
}

/// Format a line with its prefix.
pub fn format_line(prefix: &str, line: &str) -> String {
    if prefix.is_empty() {
        line.to_string()
    } else {
        format!("{} {}", prefix, line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_middle() {
        assert_eq!(truncate_middle("echo foo", 5), "ec..o");
        assert_eq!(truncate_middle("echo bar", 5), "ec..r");
        assert_eq!(truncate_middle("hi", 5), "hi");
        assert_eq!(truncate_middle("echo foo", 8), "echo foo");
        assert_eq!(truncate_middle("echo foo", 10), "echo foo");
    }

    #[test]
    fn test_pad_prefix() {
        assert_eq!(pad_prefix("[foo]", 6), "[foo   ]");
        assert_eq!(pad_prefix("[barbaz]", 6), "[barbaz]");
        assert_eq!(pad_prefix("", 6), "");
    }

    #[test]
    fn test_format_line() {
        assert_eq!(format_line("[0]", "hello"), "[0] hello");
        assert_eq!(format_line("", "hello"), "hello");
    }

    #[test]
    fn test_max_prefix_inner_width() {
        let prefixes = vec!["[foo]".to_string(), "[barbaz]".to_string()];
        assert_eq!(max_prefix_inner_width(&prefixes), 6);
    }

    #[test]
    fn test_prefix_style_from_arg() {
        assert!(matches!(
            PrefixStyle::from_arg(None, false),
            PrefixStyle::Index
        ));
        assert!(matches!(
            PrefixStyle::from_arg(None, true),
            PrefixStyle::Name
        ));
        assert!(matches!(
            PrefixStyle::from_arg(Some("command"), false),
            PrefixStyle::Command
        ));
        assert!(matches!(
            PrefixStyle::from_arg(Some("none"), false),
            PrefixStyle::None
        ));
    }
}
