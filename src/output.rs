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
    /// Template string with placeholders like `{index}`, `{name}`, `{pid}`, `{time}`, `{command}`.
    Template(String),
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
            // Anything else is treated as a template
            Some(template) => PrefixStyle::Template(template.to_string()),
        }
    }

    /// Returns true if this is a template-style prefix (no brackets).
    pub fn is_template(&self) -> bool {
        matches!(self, PrefixStyle::Template(_))
    }
}

/// Compute the prefix string for a command.
pub fn format_prefix(
    cmd: &CommandInfo,
    style: &PrefixStyle,
    pid: Option<u32>,
    prefix_length: usize,
    timestamp_format: &str,
) -> String {
    match style {
        PrefixStyle::None => String::new(),
        PrefixStyle::Template(template) => {
            expand_template(template, cmd, pid, prefix_length, timestamp_format)
        }
        _ => {
            let inner = match style {
                PrefixStyle::None => return String::new(),
                PrefixStyle::Index => cmd.index.to_string(),
                PrefixStyle::Pid => pid.map_or("?".to_string(), |p| p.to_string()),
                PrefixStyle::Command => truncate_middle(&cmd.command_line, prefix_length),
                PrefixStyle::Name => cmd.name.clone(),
                PrefixStyle::Time => format_timestamp(timestamp_format),
                PrefixStyle::Template(_) => unreachable!(),
            };
            format!("[{}]", inner)
        }
    }
}

/// Format a timestamp using the given format string.
/// The format string uses Unicode date field symbols (like concurrently),
/// which are converted to strftime format for jiff.
fn format_timestamp(format: &str) -> String {
    let strftime_format = unicode_to_strftime_format(format);
    jiff::Zoned::now().strftime(&strftime_format).to_string()
}

/// Convert a Unicode date format string to strftime format.
/// Supports the most common patterns used by concurrently.
///
/// Unicode reference: <https://unicode.org/reports/tr35/tr35-dates.html#Date_Field_Symbol_Table>
fn unicode_to_strftime_format(format: &str) -> String {
    let mut result = String::with_capacity(format.len() * 2);
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Handle quoted literals (single quotes)
        if c == '\'' {
            i += 1;
            // Handle escaped single quote ('')
            if i < chars.len() && chars[i] == '\'' {
                result.push('\'');
                i += 1;
                continue;
            }
            // Copy literal content until closing quote
            while i < chars.len() && chars[i] != '\'' {
                result.push(chars[i]);
                i += 1;
            }
            i += 1; // Skip closing quote
            continue;
        }

        // Count consecutive identical characters
        let mut count = 1;
        while i + count < chars.len() && chars[i + count] == c {
            count += 1;
        }

        // Convert Unicode pattern to strftime format
        let strftime_pattern = match (c, count) {
            // Year
            ('y', 4) | ('Y', 4) => "%Y",
            ('y', 2) | ('Y', 2) => "%y",
            ('y', _) | ('Y', _) => "%Y",
            // Month
            ('M', 1) => "%-m",
            ('M', 2) => "%m",
            ('M', 3) => "%b",
            ('M', 4..) => "%B",
            ('L', 1) => "%-m",
            ('L', 2) => "%m",
            ('L', 3) => "%b",
            ('L', 4..) => "%B",
            // Day
            ('d', 1) => "%-d",
            ('d', 2..) => "%d",
            // Hour (24-hour)
            ('H', 1) => "%-H",
            ('H', 2..) => "%H",
            // Hour (12-hour)
            ('h', 1) => "%-I",
            ('h', 2..) => "%I",
            // Minute
            ('m', 1) => "%-M",
            ('m', 2..) => "%M",
            // Second
            ('s', 1) => "%-S",
            ('s', 2..) => "%S",
            // Fractional seconds (milliseconds)
            ('S', 1) => "%.1f", // tenths
            ('S', 2) => "%.2f", // hundredths
            ('S', 3..) => "%.3f",
            // AM/PM
            ('a', _) => "%p",
            // Day of week
            ('E', 1..=3) | ('e', 3) => "%a",
            ('E', 4..) | ('e', 4..) => "%A",
            // Timezone
            ('z', 1..=3) => "%Z",
            ('z', 4..) => "%Z",
            ('Z', _) => "%z",
            // Non-pattern characters (pass through)
            _ => {
                for _ in 0..count {
                    result.push(c);
                }
                i += count;
                continue;
            }
        };

        // Handle fractional seconds specially - jiff uses %.Nf which outputs "0.NNN"
        // We want just "NNN" like Unicode SSS
        if c == 'S' {
            // We'll handle this specially in the final output
            result.push_str("%3f");
        } else {
            result.push_str(strftime_pattern);
        }
        i += count;
    }

    result
}

/// Expand a template string, replacing placeholders with values.
fn expand_template(
    template: &str,
    cmd: &CommandInfo,
    pid: Option<u32>,
    prefix_length: usize,
    timestamp_format: &str,
) -> String {
    let mut result = template.to_string();

    // Replace placeholders in order
    result = result.replace("{index}", &cmd.index.to_string());
    result = result.replace("{name}", &cmd.name);
    result = result.replace(
        "{command}",
        &truncate_middle(&cmd.command_line, prefix_length),
    );
    result = result.replace("{pid}", &pid.map_or(String::new(), |p| p.to_string()));
    result = result.replace("{time}", &format_timestamp(timestamp_format));

    result
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
/// If `is_template` is true, the prefix is not wrapped in brackets.
pub fn pad_prefix(prefix: &str, width: usize, is_template: bool) -> String {
    if prefix.is_empty() {
        return String::new();
    }
    if is_template {
        // Template prefixes are not wrapped in brackets, just pad the whole string
        format!("{:width$}", prefix, width = width)
    } else {
        // The prefix is `[content]`, we want to pad the content
        // Strip brackets, pad, re-add brackets
        if let Some(inner) = prefix.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            format!("[{:width$}]", inner, width = width)
        } else {
            prefix.to_string()
        }
    }
}

/// Calculate the max inner width across all prefixes for padding.
/// If `is_template` is true, the prefixes are not wrapped in brackets.
pub fn max_prefix_inner_width(prefixes: &[String], is_template: bool) -> usize {
    if is_template {
        // For templates, just use the full prefix length
        prefixes.iter().map(|p| p.len()).max().unwrap_or(0)
    } else {
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
        // Non-template prefixes (with brackets)
        assert_eq!(pad_prefix("[foo]", 6, false), "[foo   ]");
        assert_eq!(pad_prefix("[barbaz]", 6, false), "[barbaz]");
        assert_eq!(pad_prefix("", 6, false), "");

        // Template prefixes (no brackets)
        assert_eq!(pad_prefix("foo", 6, true), "foo   ");
        assert_eq!(pad_prefix("barbaz", 6, true), "barbaz");
        assert_eq!(pad_prefix("", 6, true), "");
    }

    #[test]
    fn test_format_line() {
        assert_eq!(format_line("[0]", "hello"), "[0] hello");
        assert_eq!(format_line("", "hello"), "hello");
    }

    #[test]
    fn test_max_prefix_inner_width() {
        // Non-template prefixes
        let prefixes = vec!["[foo]".to_string(), "[barbaz]".to_string()];
        assert_eq!(max_prefix_inner_width(&prefixes, false), 6);

        // Template prefixes
        let prefixes = vec!["foo".to_string(), "barbaz".to_string()];
        assert_eq!(max_prefix_inner_width(&prefixes, true), 6);
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
        // Template strings
        assert!(matches!(
            PrefixStyle::from_arg(Some("{index}"), false),
            PrefixStyle::Template(_)
        ));
        assert!(matches!(
            PrefixStyle::from_arg(Some("{time} {pid}"), false),
            PrefixStyle::Template(_)
        ));
    }

    #[test]
    fn test_format_prefix_template() {
        let cmd = CommandInfo::new(0, "test".to_string(), "echo hello".to_string());

        // Test {index} template
        let style = PrefixStyle::Template("{index}".to_string());
        let prefix = format_prefix(&cmd, &style, Some(12345), 10, "%Y-%m-%d %H:%M:%S%.3f");
        assert_eq!(prefix, "0");

        // Test {name} template
        let style = PrefixStyle::Template("{name}".to_string());
        let prefix = format_prefix(&cmd, &style, Some(12345), 10, "%Y-%m-%d %H:%M:%S%.3f");
        assert_eq!(prefix, "test");

        // Test {pid} template
        let style = PrefixStyle::Template("{pid}".to_string());
        let prefix = format_prefix(&cmd, &style, Some(12345), 10, "%Y-%m-%d %H:%M:%S%.3f");
        assert_eq!(prefix, "12345");

        // Test {pid} when pid is None
        let style = PrefixStyle::Template("{pid}".to_string());
        let prefix = format_prefix(&cmd, &style, None, 10, "%Y-%m-%d %H:%M:%S%.3f");
        assert_eq!(prefix, "");

        // Test {command} template
        let style = PrefixStyle::Template("{command}".to_string());
        let prefix = format_prefix(&cmd, &style, Some(12345), 100, "%Y-%m-%d %H:%M:%S%.3f");
        assert_eq!(prefix, "echo hello");

        // Test combined template
        let style = PrefixStyle::Template("{index}-{name}".to_string());
        let prefix = format_prefix(&cmd, &style, Some(12345), 10, "%Y-%m-%d %H:%M:%S%.3f");
        assert_eq!(prefix, "0-test");
    }

    #[test]
    fn test_is_template() {
        assert!(!PrefixStyle::Index.is_template());
        assert!(!PrefixStyle::Name.is_template());
        assert!(!PrefixStyle::None.is_template());
        assert!(PrefixStyle::Template("{index}".to_string()).is_template());
    }
}
