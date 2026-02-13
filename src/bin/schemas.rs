//! Generate command-wise JSON Schema files for CLI JSON responses.

use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::entry::{EntryDetail, EntryListResponse};
use picofeedr::error::ErrorPayload;
use picofeedr::feed::FeedListResponse;
use picofeedr::response::{Envelope, ResponseMeta};
use picofeedr::status::StatusResponse;
use picofeedr::sync::SyncSummary;
use schemars::JsonSchema;
use schemars::r#gen::SchemaSettings;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// Ping status fixed literal.
#[derive(Debug, Serialize, JsonSchema)]
enum PingStatus {
    /// Fixed status string.
    #[serde(rename = "ok")]
    Ok,
}

/// Ping payload.
#[derive(Debug, Serialize, JsonSchema)]
struct PingResult {
    /// Fixed heartbeat status.
    status: PingStatus,
}

/// Version payload.
#[derive(Debug, Serialize, JsonSchema)]
struct VersionResult {
    /// CLI API version.
    api_version: String,
    /// SQLite schema version.
    schema_version: i64,
    /// Build channel label.
    build: String,
}

/// Tags payload.
#[derive(Debug, Serialize, JsonSchema)]
struct TagsResult {
    /// Known tag dictionary.
    tags: Vec<String>,
}

/// Mark payload.
#[derive(Debug, Serialize, JsonSchema)]
struct MarkResult {
    /// Number of entries updated by mark command.
    updated_entry_count: usize,
}

/// Fatal-only envelope schema.
#[derive(Debug, Serialize, JsonSchema)]
struct FatalErrorEnvelope {
    /// Always false for fatal responses.
    success: bool,
    /// Always error for fatal responses.
    severity: FatalSeverity,
    /// Always null for fatal responses.
    result: (),
    /// Fatal error payload.
    error: ErrorPayload,
    /// Common response metadata.
    meta: ResponseMeta,
}

/// Fatal severity fixed literal.
#[derive(Debug, Serialize, JsonSchema)]
enum FatalSeverity {
    /// Fixed severity string.
    #[serde(rename = "error")]
    Error,
}

/// Writes one JSON schema file under `doc/spec/schema`.
fn write_schema<T: JsonSchema>(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let generator = SchemaSettings::draft07().into_generator();
    let schema = generator.into_root_schema_for::<T>();
    let json = serde_json::to_string_pretty(&schema)?;
    let output = Path::new("doc/spec/schema").join(name);
    fs::write(output, json)?;
    Ok(())
}

/// Generates all command-wise response schemas.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = PingStatus::Ok;
    let _ = FatalSeverity::Error;
    fs::create_dir_all("doc/spec/schema")?;

    write_schema::<Envelope<PingResult>>("ping.response.schema.json")?;
    write_schema::<Envelope<VersionResult>>("version.response.schema.json")?;
    write_schema::<Envelope<FeedListResponse>>("feeds.response.schema.json")?;
    write_schema::<Envelope<ConfigCheckReport>>("config-check.response.schema.json")?;
    write_schema::<Envelope<SyncSummary>>("sync.response.schema.json")?;
    write_schema::<Envelope<StatusResponse>>("status.response.schema.json")?;
    write_schema::<Envelope<EntryListResponse>>("list.response.schema.json")?;
    write_schema::<Envelope<EntryDetail>>("view.response.schema.json")?;
    write_schema::<Envelope<MarkResult>>("mark.response.schema.json")?;
    write_schema::<Envelope<TagsResult>>("tags.response.schema.json")?;
    write_schema::<FatalErrorEnvelope>("fatal-error.response.schema.json")?;

    Ok(())
}
