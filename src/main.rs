//! Feeder CLI entrypoint.

mod cli;
mod config;
mod db;
mod error;
mod feed;
mod sync;
mod tag;
mod time;

use crate::cli::{Cli, Command};
use crate::error::{AppError, ErrorResponse};
use crate::tag::TagManager;
use clap::Parser;
use serde_json::json;

/// Runs the CLI and prints JSON output or error to stdout.
fn main() {
    if let Err(error) = run() {
        let response = ErrorResponse::from_error(&error);
        println!(
            "{}",
            serde_json::to_string(&response).unwrap_or_else(|_| {
                "{\"error\":{\"code\":\"INTERNAL\",\"message\":\"Failed to serialize error\",\"retry\":false}}".to_string()
            })
        );
        std::process::exit(1);
    }
}

/// Executes the CLI command and prints a JSON response.
fn run() -> Result<(), AppError> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Ping => {
            println!("{}", json!({"ok": true}));
            return Ok(());
        }
        Command::Version => {
            println!(
                "{}",
                json!({
                    "api_version": "0.5.0",
                    "schema_version": 1,
                    "build": "dev"
                })
            );
            return Ok(());
        }
        _ => {}
    }

    let mut config = config::AppConfig::load(cli.config.clone())?;
    if let Some(db_path) = cli.db.clone() {
        config.override_db_path(db_path)?;
    }

    let store = db::sqlite::SqliteStore::open(&config.database.path)?;
    store.migrate()?;

    match &cli.command {
        Command::Tags => {
            let tag_manager = TagManager::new(&store);
            let tags = tag_manager.list_tags()?;
            println!("{}", json!({"tags": tags}));
        }
        Command::Feeds { config_check } => {
            let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
            if *config_check {
                let db_feeds = store.list_feeds()?;
                let diff = feed::diff_config_vs_db(&feeds_config, &db_feeds);
                println!("{}", serde_json::to_string(&diff)?);
            } else {
                feed::reconcile_feeds(&store, &feeds_config, &config.unread_tag)?;
                let db_feeds = store.list_feeds()?;
                let feeds = feed::render_feed_list(&feeds_config, &db_feeds);
                println!("{}", serde_json::to_string(&feeds)?);
            }
        }
        Command::Sync => {
            let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
            let summary = sync::run_sync(&store, &config, &feeds_config)?;
            println!("{}", serde_json::to_string(&summary)?);
        }
        Command::Ping | Command::Version => {}
    }

    Ok(())
}
