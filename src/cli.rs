use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "crun")]
#[command(author, version, about = "Run commands concurrently", long_about = None)]
#[command(after_help = "For documentation, visit: https://github.com/llimllib/crun")]
pub struct Args {
    /// Commands to run concurrently
    #[arg(required = false)]
    pub commands: Vec<String>,

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
}
