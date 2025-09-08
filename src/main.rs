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
            cli::BackupCommands::Create(opts) => commands::backup::create::run(opts).await?,
            cli::BackupCommands::List(opts) => commands::backup::list::run(opts).await?,
            cli::BackupCommands::Restore(opts) => commands::backup::restore::run(opts).await?,
        },
        cli::Commands::Database(db_cmd) => match db_cmd {
            cli::DatabaseCommands::List(opts) => commands::database::list::run(opts).await?,
        },
    }

    Ok(())
}
