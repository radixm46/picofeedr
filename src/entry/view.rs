use super::EntryDetail;
use crate::config::AppConfig;
use crate::db::sqlite::SqliteStore;
use crate::error::{AppError, error_details};
use serde_json::Value as JsonValue;

/// Loads entry detail by id.
pub fn view_entry(
    store: &SqliteStore,
    config: &AppConfig,
    entry_id: &str,
) -> Result<EntryDetail, AppError> {
    let entry_repo = store.entry_read_repo();
    let row = entry_repo.view_entry_row(entry_id)?.ok_or_else(|| {
        AppError::entry_not_found_with_details(
            format!("Entry {entry_id} not found"),
            error_details([
                ("resource", JsonValue::from("entry")),
                ("entry_id", JsonValue::from(entry_id.to_string())),
            ]),
        )
    })?;
    let tags = entry_repo
        .load_tags(&[row.entry_pk])?
        .remove(&row.entry_pk)
        .unwrap_or_default();
    let (content, content_type) =
        entry_repo.load_content(&config.storage.data_dir, row.entry_pk)?;
    let enclosures = entry_repo.load_enclosures(row.entry_pk)?;

    Ok(EntryDetail {
        entry_id: row.entry_id,
        feed_id: row.feed_id,
        feed_title: row.feed_title,
        title: row.title,
        link: row.link,
        author: row.author,
        published_at: row.published_at,
        first_seen_at: row.first_seen_at,
        content,
        content_type,
        tags,
        enclosures,
    })
}
