use super::store::with_store;
use picofeedr::cli::MarkCommand;
use picofeedr::config;
use picofeedr::current_epoch;
use picofeedr::db;
use picofeedr::entry;
use picofeedr::error::AppError;
use picofeedr::parse_tag_csv;
use picofeedr::response::MarkResponse;
use std::io::{self, Read};

const MAX_STDIN_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn run_mark_command(
    config: &config::AppConfig,
    command: &MarkCommand,
) -> Result<MarkResponse, AppError> {
    let entry_ids = resolve_entry_ids(command)?;
    with_store(config, |store| {
        let updated = apply_mark_command(store, config, command, &entry_ids)?;
        store.bump_revision(current_epoch())?;
        Ok(MarkResponse {
            updated_entry_count: updated,
        })
    })
}

fn apply_mark_command(
    store: &mut db::sqlite::SqliteStore,
    config: &config::AppConfig,
    command: &MarkCommand,
    entry_ids: &[String],
) -> Result<usize, AppError> {
    match command {
        MarkCommand::Read { .. } => {
            entry::mark_entries(store, entry_ids, &[], &[config.unread_tag().to_string()])
        }
        MarkCommand::Unread { .. } => {
            entry::mark_entries(store, entry_ids, &[config.unread_tag().to_string()], &[])
        }
        MarkCommand::Tag { add, remove, .. } => {
            let add_tags = parse_tag_csv(add.as_deref());
            let remove_tags = parse_tag_csv(remove.as_deref());
            entry::mark_entries(store, entry_ids, &add_tags, &remove_tags)
        }
    }
}

fn resolve_entry_ids(command: &MarkCommand) -> Result<Vec<String>, AppError> {
    let ids = match command {
        MarkCommand::Read { ids } | MarkCommand::Unread { ids } | MarkCommand::Tag { ids, .. } => {
            ids
        }
    };
    let stdin_count = ids.iter().filter(|id| id.as_str() == "-").count();
    if stdin_count > 1 {
        return Err(AppError::config("mark stdin '-' cannot be repeated"));
    }
    if stdin_count == 1 && ids.len() > 1 {
        return Err(AppError::config(
            "mark stdin '-' cannot be combined with entry ids",
        ));
    }
    if stdin_count == 0 {
        return Ok(ids.clone());
    }

    let mut input = Vec::new();
    io::stdin()
        .take((MAX_STDIN_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|error| AppError::io_with_source("failed to read entry ids from stdin", error))?;
    if input.len() > MAX_STDIN_BYTES {
        return Err(AppError::config("stdin exceeds 16 MiB limit"));
    }
    let input = String::from_utf8(input)
        .map_err(|error| AppError::io_with_source("stdin is not valid UTF-8", error))?;
    let input = input.strip_prefix('\u{feff}').unwrap_or(input.as_str());
    let entry_ids = input
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if entry_ids.is_empty() {
        return Err(AppError::config("stdin did not contain any entry ids"));
    }
    Ok(entry_ids)
}
