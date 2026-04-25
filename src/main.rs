use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod parser;
mod types;
mod util;
mod validator;

#[derive(Parser)]
#[command(name = "po-cli")]
#[command(version, about = "Analyze and update Django gettext .po files", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a .po file: statistics, untranslated, fuzzy entries
    Analyze {
        /// Path to the .po file
        po_file: PathBuf,
    },
    /// Validate translations and update the .po file
    Update {
        /// Path to the .po file
        po_file: PathBuf,

        /// Path to JSON file containing translations array
        #[arg(short, long)]
        translations: PathBuf,

        /// Disable strict validation (variables, HTML, URLs, JS)
        #[arg(long)]
        no_strict: bool,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,

        /// Update file even if some translations are invalid
        #[arg(long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze { po_file } => {
            commands::analyze::run(&po_file, cli.json)?;
        }
        Commands::Update {
            po_file,
            translations,
            no_strict,
            dry_run,
            force,
        } => {
            commands::update::run(
                &po_file,
                &translations,
                !no_strict,
                dry_run,
                force,
                cli.json,
            )?;
        }
    }

    Ok(())
}
