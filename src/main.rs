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
        cli::Commands::Backup(backup_cmd) => match backup_cmd {
            cli::BackupCommands::Create(opts) => commands::backup::run(opts).await?,
            cli::BackupCommands::List(opts) => commands::list::run(opts).await?,
            cli::BackupCommands::Restore(opts) => commands::restore::run(opts).await?,
        },
        cli::Commands::Database(db_cmd) => match db_cmd {
            cli::DatabaseCommands::List(opts) => commands::databases::run(opts).await?,
        },
    }

    Ok(())
}
