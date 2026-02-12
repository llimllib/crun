use clap::Parser;

mod cli;
mod color;
mod command;
mod input;
mod output;
mod runner;

use cli::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // If no commands provided, show help
    if args.commands.is_empty() {
        Args::parse_from(["crun", "--help"]);
        return Ok(());
    }

    // Validate that no command is empty (matching concurrently's behavior)
    let commands = args.get_commands();
    for cmd in &commands {
        if cmd.trim().is_empty() {
            eprintln!("[crun] command cannot be empty");
            std::process::exit(1);
        }
    }

    let exit_code = runner::run(args).await?;
    std::process::exit(exit_code);
}
