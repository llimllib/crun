//! ANSI color codes for terminal output.

/// The auto color cycle, matching concurrently's ACCEPTABLE_CONSOLE_COLORS.
const AUTO_COLORS: &[&str] = &[
    "cyan",
    "yellow",
    "greenBright",
    "blueBright",
    "magentaBright",
    "white",
    "grey",
    "red",
    "bgCyan",
    "bgYellow",
    "bgGreenBright",
    "bgBlueBright",
    "bgMagenta",
    "bgWhiteBright",
    "bgGrey",
    "bgRed",
];

/// Convert a color name to its ANSI escape code.
/// Returns None for unrecognized names.
fn color_to_ansi(name: &str) -> Option<&'static str> {
    match name {
        // Foreground colors
        "black" => Some("\x1b[30m"),
        "red" => Some("\x1b[31m"),
        "green" => Some("\x1b[32m"),
        "yellow" => Some("\x1b[33m"),
        "blue" => Some("\x1b[34m"),
        "magenta" => Some("\x1b[35m"),
        "cyan" => Some("\x1b[36m"),
        "white" => Some("\x1b[37m"),
        "grey" | "gray" => Some("\x1b[90m"),

        // Bright foreground
        "blackBright" => Some("\x1b[90m"),
        "redBright" => Some("\x1b[91m"),
        "greenBright" => Some("\x1b[92m"),
        "yellowBright" => Some("\x1b[93m"),
        "blueBright" => Some("\x1b[94m"),
        "magentaBright" => Some("\x1b[95m"),
        "cyanBright" => Some("\x1b[96m"),
        "whiteBright" => Some("\x1b[97m"),

        // Background colors
        "bgBlack" => Some("\x1b[40m"),
        "bgRed" => Some("\x1b[41m"),
        "bgGreen" => Some("\x1b[42m"),
        "bgYellow" => Some("\x1b[43m"),
        "bgBlue" => Some("\x1b[44m"),
        "bgMagenta" => Some("\x1b[45m"),
        "bgCyan" => Some("\x1b[46m"),
        "bgWhite" => Some("\x1b[47m"),
        "bgGrey" | "bgGray" => Some("\x1b[100m"),

        // Bright background
        "bgBlackBright" => Some("\x1b[100m"),
        "bgRedBright" => Some("\x1b[101m"),
        "bgGreenBright" => Some("\x1b[102m"),
        "bgYellowBright" => Some("\x1b[103m"),
        "bgBlueBright" => Some("\x1b[104m"),
        "bgMagentaBright" => Some("\x1b[105m"),
        "bgCyanBright" => Some("\x1b[106m"),
        "bgWhiteBright" => Some("\x1b[107m"),

        // Modifiers
        "bold" => Some("\x1b[1m"),
        "dim" => Some("\x1b[2m"),
        "italic" => Some("\x1b[3m"),
        "underline" => Some("\x1b[4m"),
        "inverse" => Some("\x1b[7m"),
        "strikethrough" => Some("\x1b[9m"),

        _ => None,
    }
}

const RESET: &str = "\x1b[0m";

/// Resolve a color spec (which may contain dots like "red.bold") to an ANSI escape sequence.
fn resolve_color_spec(spec: &str) -> Option<String> {
    let parts: Vec<&str> = spec.split('.').collect();
    let mut codes = String::new();
    for part in &parts {
        codes.push_str(color_to_ansi(part)?);
    }
    if codes.is_empty() {
        None
    } else {
        Some(codes)
    }
}

/// Assigns colors to commands based on the `--prefix-colors` argument.
///
/// Returns a vector of ANSI escape sequences (one per command), or an empty
/// vector if colors are disabled.
pub fn assign_colors(prefix_colors: &str, num_commands: usize, no_color: bool) -> Vec<String> {
    if no_color || std::env::var("NO_COLOR").is_ok() {
        return vec![String::new(); num_commands];
    }

    if prefix_colors.is_empty() {
        return vec![String::new(); num_commands];
    }

    let specs: Vec<&str> = prefix_colors.split(',').collect();

    // Build auto color generator state
    let mut auto_index = 0;
    // Filter auto colors to exclude any explicitly used colors
    let used_colors: Vec<&str> = specs.iter().filter(|s| **s != "auto").copied().collect();
    let auto_pool: Vec<&&str> = AUTO_COLORS
        .iter()
        .filter(|c| {
            !used_colors.iter().any(|u| {
                u.split('.')
                    .any(|part| part == c.replace("Bright", "").as_str())
            })
        })
        .collect();

    let mut result = Vec::with_capacity(num_commands);
    let mut last_color = String::new();

    for i in 0..num_commands {
        let spec = if i < specs.len() {
            specs[i]
        } else {
            // After explicit specs run out, repeat the last one (or "auto" if last was "auto")
            specs.last().copied().unwrap_or("")
        };

        if spec == "auto" {
            // Pick next auto color, avoiding repeating the last one
            let color = pick_auto_color(&auto_pool, &mut auto_index, &last_color);
            last_color = color.clone();
            result.push(color);
        } else if let Some(ansi) = resolve_color_spec(spec) {
            last_color = spec.to_string();
            result.push(ansi);
        } else {
            result.push(String::new());
        }
    }

    result
}

/// Pick the next auto color, skipping if it would repeat the last color.
fn pick_auto_color(pool: &[&&str], index: &mut usize, last: &str) -> String {
    if pool.is_empty() {
        // Fall back to full auto colors list
        let i = *index % AUTO_COLORS.len();
        *index += 1;
        return resolve_color_spec(AUTO_COLORS[i]).unwrap_or_default();
    }

    // Try to find a color that isn't the same as the last one
    for _ in 0..pool.len() {
        let i = *index % pool.len();
        *index += 1;
        let color_name = pool[i];
        if *color_name != last {
            return resolve_color_spec(color_name).unwrap_or_default();
        }
    }

    // If all colors match last (shouldn't happen), just use the next one
    let i = *index % pool.len();
    *index += 1;
    resolve_color_spec(pool[i]).unwrap_or_default()
}

/// Wrap a string with a color code and reset.
pub fn colorize(text: &str, color: &str) -> String {
    if color.is_empty() {
        text.to_string()
    } else {
        format!("{}{}{}", color, text, RESET)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_to_ansi() {
        assert_eq!(color_to_ansi("red"), Some("\x1b[31m"));
        assert_eq!(color_to_ansi("cyan"), Some("\x1b[36m"));
        assert_eq!(color_to_ansi("bgRed"), Some("\x1b[41m"));
        assert_eq!(color_to_ansi("bold"), Some("\x1b[1m"));
        assert_eq!(color_to_ansi("nonsense"), None);
    }

    #[test]
    fn test_resolve_color_spec() {
        assert_eq!(resolve_color_spec("red"), Some("\x1b[31m".to_string()));
        assert_eq!(
            resolve_color_spec("red.bold"),
            Some("\x1b[31m\x1b[1m".to_string())
        );
        assert_eq!(resolve_color_spec("nonsense"), None);
    }

    #[test]
    fn test_colorize() {
        assert_eq!(colorize("hello", ""), "hello");
        assert_eq!(colorize("hello", "\x1b[31m"), "\x1b[31mhello\x1b[0m");
    }

    #[test]
    fn test_assign_colors_empty() {
        let colors = assign_colors("", 3, false);
        assert_eq!(colors, vec!["", "", ""]);
    }

    #[test]
    fn test_assign_colors_no_color() {
        let colors = assign_colors("auto", 3, true);
        assert_eq!(colors, vec!["", "", ""]);
    }

    #[test]
    fn test_assign_colors_explicit() {
        let colors = assign_colors("red,green,blue", 3, false);
        assert_eq!(colors[0], "\x1b[31m");
        assert_eq!(colors[1], "\x1b[32m");
        assert_eq!(colors[2], "\x1b[34m");
    }

    #[test]
    fn test_assign_colors_auto() {
        let colors = assign_colors("auto", 3, false);
        assert!(!colors[0].is_empty());
        assert!(!colors[1].is_empty());
        assert!(!colors[2].is_empty());
        // Should all be different
        assert_ne!(colors[0], colors[1]);
        assert_ne!(colors[1], colors[2]);
    }

    #[test]
    fn test_assign_colors_mixed() {
        let colors = assign_colors("red,auto", 3, false);
        assert_eq!(colors[0], "\x1b[31m"); // explicit red
        assert!(!colors[1].is_empty()); // auto
        assert!(!colors[2].is_empty()); // auto (last spec was auto, so continues)
    }

    #[test]
    fn test_assign_colors_dot_notation() {
        let colors = assign_colors("red.bold", 1, false);
        assert_eq!(colors[0], "\x1b[31m\x1b[1m");
    }
}
