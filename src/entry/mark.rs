use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::string_set::dedupe_strings_preserve_order;
use crate::tag::{invalid_mark_tag_name_error, validate_tag_name};

/// Updates entry tags and returns the number of affected entries.
///
/// Returns `ENTRY_NOT_FOUND` when any requested entry id does not exist.
pub fn mark_entries(
    store: &mut SqliteStore,
    entry_ids: &[String],
    add_tags: &[String],
    remove_tags: &[String],
) -> Result<usize, AppError> {
    for tag in add_tags.iter().chain(remove_tags) {
        validate_tag_name(tag)
            .map_err(|violation| invalid_mark_tag_name_error(tag.clone(), violation))?;
    }
    if add_tags.is_empty() && remove_tags.is_empty() {
        return Err(AppError::invalid_input(
            "mark tag requires --add or --remove",
        ));
    }
    let unique_ids = dedupe_strings_preserve_order(entry_ids.iter().cloned());
    if unique_ids.is_empty() {
        return Ok(0);
    }
    let tx = store.tx()?;
    let tx_entry_repo = tx.entry_write_repo();
    tx_entry_repo.ensure_all_entry_ids_exist(&unique_ids)?;
    let entry_pks = tx_entry_repo.find_entry_pks_by_ids(&unique_ids)?;
    let add_ids = tx_entry_repo.ensure_tag_ids(add_tags)?;
    let remove_ids = tx_entry_repo.lookup_tag_ids(remove_tags)?;
    if add_ids.is_empty() && remove_ids.is_empty() {
        return Ok(0);
    }
    let staged_entry_pks = unique_ids
        .iter()
        .filter_map(|entry_id| entry_pks.get(entry_id).copied())
        .collect::<Vec<_>>();
    tx_entry_repo.clear_mark_temp_tables()?;
    tx_entry_repo.stage_mark_entry_pks(&staged_entry_pks)?;
    tx_entry_repo.stage_mark_add_tag_ids(&add_ids.values().copied().collect::<Vec<_>>())?;
    tx_entry_repo.stage_mark_remove_tag_ids(&remove_ids.values().copied().collect::<Vec<_>>())?;
    let updated = tx_entry_repo.count_mark_changed_entries()?;
    tx_entry_repo.apply_mark_adds()?;
    tx_entry_repo.apply_mark_removes()?;
    tx_entry_repo.clear_mark_temp_tables()?;
    tx.commit()?;
    Ok(updated)
}
