use super::store::with_store;
use picofeedr::cli::MarkCommand;
use picofeedr::config;
use picofeedr::current_epoch;
use picofeedr::db;
use picofeedr::entry;
use picofeedr::error::AppError;
use picofeedr::parse_tag_csv;
use picofeedr::response::MarkResponse;

pub(crate) fn run_mark_command(
    config: &config::AppConfig,
    command: &MarkCommand,
) -> Result<MarkResponse, AppError> {
    with_store(config, |store| {
        let updated = apply_mark_command(store, config, command)?;
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
) -> Result<usize, AppError> {
    match command {
        MarkCommand::Read { ids } => {
            entry::mark_entries(store, ids, &[], std::slice::from_ref(&config.unread_tag))
        }
        MarkCommand::Unread { ids } => {
            entry::mark_entries(store, ids, std::slice::from_ref(&config.unread_tag), &[])
        }
        MarkCommand::Tag { ids, add, remove } => {
            let add_tags = parse_tag_csv(add.as_deref());
            let remove_tags = parse_tag_csv(remove.as_deref());
            entry::mark_entries(store, ids, &add_tags, &remove_tags)
        }
    }
}
