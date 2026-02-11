use clap::Parser;

mod cli;
mod runner;

use cli::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // If no commands provided, show help
    if args.commands.is_empty() {
        Args::parse_from(["cancurrently", "--help"]);
        return Ok(());
    }

    let exit_code = runner::run(args).await?;
    std::process::exit(exit_code);
}
