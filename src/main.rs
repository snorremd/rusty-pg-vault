mod cli;
mod commands;
mod modules;
mod utils;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        cli::Commands::Backup(opts) => commands::backup::run(opts).await?,
        cli::Commands::Restore(opts) => commands::restore::run(opts).await?,
        cli::Commands::List(opts) => commands::list::run(opts).await?,
    }

    Ok(())
}
