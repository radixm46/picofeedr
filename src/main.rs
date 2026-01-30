//! Feeder CLI entrypoint.

mod cli;
mod config;
mod db;
mod error;
mod feed;
mod identity;
mod response;
mod sync;
mod tag;
mod time;

use crate::cli::{Cli, Command, OutputFormat};
use crate::error::AppError;
use crate::response::Envelope;
use crate::tag::TagManager;
use clap::Parser;
use serde_json::json;

/// Runs the CLI and prints JSON output or error to stdout.
fn main() {
    let cli = Cli::parse();
    let output = cli.output;
    if let Err(error) = run(cli) {
        match output {
            OutputFormat::Json => {
                print_json_or_fallback(&Envelope::<serde_json::Value>::fatal(&error))
            }
            OutputFormat::Plain => eprintln!("{}", error),
        }
        std::process::exit(1);
    }
}

/// Executes the CLI command and prints a JSON response.
fn run(cli: Cli) -> Result<(), AppError> {
    match cli.output {
        OutputFormat::Json => run_json(cli),
        OutputFormat::Plain => run_plain(cli),
    }
}

/// Executes the CLI command and prints a JSON envelope response.
fn run_json(cli: Cli) -> Result<(), AppError> {
    let data = match &cli.command {
        Command::Ping => json!({"ok": true}),
        Command::Version => json!({
            "api_version": "0.5.0",
            "schema_version": 1,
            "build": "dev"
        }),
        Command::Tags | Command::Feeds { .. } | Command::Sync => {
            let mut config = config::AppConfig::load(cli.config.clone())?;
            if let Some(db_path) = cli.db.clone() {
                config.override_db_path(db_path)?;
            }
            let mut store = db::sqlite::SqliteStore::open(&config.database.path)?;
            store.migrate()?;

            match &cli.command {
                Command::Tags => {
                    let tag_manager = TagManager::new(&store);
                    let tags = tag_manager.list_tags()?;
                    json!({ "tags": tags })
                }
                Command::Feeds { config_check } => {
                    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
                    if *config_check {
                        let db_feeds = store.list_feeds()?;
                        serde_json::to_value(feed::diff_config_vs_db(&feeds_config, &db_feeds))?
                    } else {
                        feed::reconcile_feeds(&store, &feeds_config, &config.unread_tag)?;
                        let db_feeds = store.list_feeds()?;
                        serde_json::to_value(feed::render_feed_list(&feeds_config, &db_feeds))?
                    }
                }
                Command::Sync => {
                    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
                    let summary = sync::run_sync(&mut store, &config, &feeds_config)?;
                    serde_json::to_value(summary)?
                }
                Command::Ping | Command::Version => unreachable!("handled above"),
            }
        }
    };

    print_json_or_fallback(&Envelope::ok(data));
    Ok(())
}

/// Executes the CLI command and prints human-readable output.
fn run_plain(cli: Cli) -> Result<(), AppError> {
    match &cli.command {
        Command::Ping => {
            println!("ok");
            return Ok(());
        }
        Command::Version => {
            println!("api_version=0.5.0 schema_version=1 build=dev");
            return Ok(());
        }
        _ => {}
    }

    let mut config = config::AppConfig::load(cli.config.clone())?;
    if let Some(db_path) = cli.db.clone() {
        config.override_db_path(db_path)?;
    }

    let mut store = db::sqlite::SqliteStore::open(&config.database.path)?;
    store.migrate()?;

    match &cli.command {
        Command::Tags => {
            let tag_manager = TagManager::new(&store);
            let tags = tag_manager.list_tags()?;
            for tag in tags {
                println!("{tag}");
            }
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
            let summary = sync::run_sync(&mut store, &config, &feeds_config)?;
            println!("{}", serde_json::to_string(&summary)?);
        }
        Command::Ping | Command::Version => {}
    }

    Ok(())
}

/// Prints JSON to stdout, falling back to a hard-coded INTERNAL error JSON on failure.
fn print_json_or_fallback<T: serde::Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(json) => println!("{json}"),
        Err(_) => println!("{FALLBACK_INTERNAL_ERROR_JSON}"),
    }
}

/// Fallback JSON printed when JSON serialization fails unexpectedly.
const FALLBACK_INTERNAL_ERROR_JSON: &str = "{\"ok\":false,\"data\":null,\"error\":{\"code\":\"INTERNAL\",\"message\":\"Failed to serialize response\",\"retry\":false}}";
