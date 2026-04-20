use super::store::with_store;
use picofeedr::cli::SortOrder;
use picofeedr::config;
use picofeedr::entry::{self, EntryListResponse};
use picofeedr::error::{AppError, ErrorDetails, error_details};
use picofeedr::query::EntryQuery;
use serde_json::Value;

pub(crate) fn run_list_command(
    config: &config::AppConfig,
    query: Option<&str>,
    sort: Option<SortOrder>,
    limit: Option<usize>,
    cursor: Option<&str>,
) -> Result<EntryListResponse, AppError> {
    with_store(config, |store| {
        let query = EntryQuery::parse(query, Some(config.unread_tag()))?;
        let sort = sort.unwrap_or(SortOrder::FirstSeenDesc);
        let limit = resolve_list_limit(limit, config.query)?;
        entry::list_entries(store, &query, sort, limit, cursor)
    })
}

/// Resolves the effective list limit from CLI argument and query config.
fn resolve_list_limit(limit: Option<usize>, query: config::QueryConfig) -> Result<usize, AppError> {
    let resolved = limit.unwrap_or(query.default_limit);
    if resolved == 0 {
        return Err(AppError::invalid_query_with_details(
            "--limit must be greater than 0",
            limit_error_details("zero_or_negative", resolved, query.max_limit),
        ));
    }
    if resolved > query.max_limit {
        return Err(AppError::invalid_query_with_details(
            format!("--limit must be less than or equal to {}", query.max_limit),
            limit_error_details("exceeds_max_limit", resolved, query.max_limit),
        ));
    }
    Ok(resolved)
}

/// Builds standardized details payload for limit validation failures.
fn limit_error_details(kind: &str, value: usize, _max_limit: usize) -> ErrorDetails {
    let hint = match kind {
        "zero_or_negative" => "limit_must_be_greater_than_zero",
        "exceeds_max_limit" => "limit_exceeds_configured_max_limit",
        _ => "invalid_limit",
    };
    error_details([
        ("kind", Value::from("limit_out_of_range")),
        ("field", Value::from("limit")),
        ("value", Value::from(value)),
        ("hint", Value::from(hint)),
    ])
}

#[cfg(test)]
mod tests {
    use super::resolve_list_limit;
    use picofeedr::config::QueryConfig;

    #[test]
    fn resolve_list_limit_uses_default_when_cli_limit_missing() {
        let query = QueryConfig {
            default_limit: 25,
            max_limit: 100,
        };

        assert_eq!(resolve_list_limit(None, query).unwrap(), 25);
    }

    #[test]
    fn resolve_list_limit_rejects_zero() {
        let query = QueryConfig {
            default_limit: 25,
            max_limit: 100,
        };

        let error = resolve_list_limit(Some(0), query).unwrap_err();
        assert!(error.to_string().contains("--limit must be greater than 0"));
    }
}
