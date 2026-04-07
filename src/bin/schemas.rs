//! Generate command-wise JSON Schema files for CLI JSON responses.

use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::entry::{EntryDetail, EntryListResponse};
use picofeedr::error::ErrorPayload;
use picofeedr::feed::FeedListResponse;
use picofeedr::response::{
    Envelope, MarkResponse, PingResponse, PingStatus, ResponseMeta, TagListResponse,
    VersionResponse,
};
use picofeedr::status::StatusResponse;
use picofeedr::sync::SyncSummary;
use schemars::JsonSchema;
use schemars::r#gen::SchemaSettings;
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

/// Fatal-only envelope schema.
#[derive(Debug, Serialize, JsonSchema)]
struct FatalErrorEnvelope {
    /// Always `error` for fatal responses.
    status: FatalStatus,
    /// Always null for fatal responses.
    result: (),
    /// Fatal error payload.
    error: ErrorPayload,
    /// Common response metadata.
    meta: ResponseMeta,
}

/// Fatal status fixed literal.
#[derive(Debug, Serialize, JsonSchema)]
enum FatalStatus {
    /// Fixed status string.
    #[serde(rename = "error")]
    Error,
}

/// Writes one JSON schema file under `doc/spec/schema`.
fn write_schema<T: JsonSchema>(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let generator = SchemaSettings::draft07().into_generator();
    let schema = generator.into_root_schema_for::<T>();
    let mut value = serde_json::to_value(&schema)?;
    enforce_envelope_contract(&mut value)?;
    let json = serde_json::to_string_pretty(&value)?;
    let output = Path::new("doc/spec/schema").join(name);
    fs::write(output, json)?;
    Ok(())
}

/// Enforces top-level envelope contract constraints on generated schemas.
fn enforce_envelope_contract(schema: &mut Value) -> Result<(), Box<dyn std::error::Error>> {
    let root = schema
        .as_object_mut()
        .ok_or_else(|| "schema root must be an object".to_string())?;
    let root_ref = root
        .get("$ref")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let target = if let Some(reference) = root_ref.as_deref() {
        let parts: Vec<&str> = reference.split('/').collect();
        if parts.len() != 3 || parts[0] != "#" {
            return Err(format!("unsupported $ref format: {reference}").into());
        }
        let defs = root
            .get_mut(parts[1])
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("missing ref container: {}", parts[1]))?;
        defs.get_mut(parts[2])
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("missing ref target: {reference}"))?
    } else {
        root
    };
    target.insert(
        "required".to_string(),
        json!(["status", "result", "error", "meta"]),
    );
    target.insert("additionalProperties".to_string(), Value::Bool(false));
    target.insert(
        "properties".to_string(),
        target
            .get("properties")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    if let Some(properties) = target.get_mut("properties").and_then(Value::as_object_mut)
        && !properties.contains_key("status")
    {
        properties.insert(
            "status".to_string(),
            json!({
                "type": "string",
                "enum": ["ok", "warning", "error"]
            }),
        );
    }
    target.insert(
        "allOf".to_string(),
        json!([
            {
                "if": {
                    "required": ["status"],
                    "properties": { "status": { "const": "error" } }
                },
                "then": {
                    "properties": {
                        "result": { "type": "null" },
                        "error": { "not": { "type": "null" } }
                    }
                }
            },
            {
                "if": {
                    "required": ["status"],
                    "properties": { "status": { "enum": ["ok", "warning"] } }
                },
                "then": {
                    "properties": {
                        "result": { "not": { "type": "null" } },
                        "error": { "type": "null" }
                    }
                }
            }
        ]),
    );
    Ok(())
}

/// Generates all command-wise response schemas.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = PingStatus::Ok;
    let _ = FatalStatus::Error;
    fs::create_dir_all("doc/spec/schema")?;

    write_schema::<Envelope<PingResponse>>("ping.response.schema.json")?;
    write_schema::<Envelope<VersionResponse>>("version.response.schema.json")?;
    write_schema::<Envelope<FeedListResponse>>("feeds.response.schema.json")?;
    write_schema::<Envelope<ConfigCheckReport>>("config-check.response.schema.json")?;
    write_schema::<Envelope<SyncSummary>>("sync.response.schema.json")?;
    write_schema::<Envelope<StatusResponse>>("status.response.schema.json")?;
    write_schema::<Envelope<EntryListResponse>>("list.response.schema.json")?;
    write_schema::<Envelope<EntryDetail>>("view.response.schema.json")?;
    write_schema::<Envelope<MarkResponse>>("mark.response.schema.json")?;
    write_schema::<Envelope<TagListResponse>>("tags.response.schema.json")?;
    write_schema::<FatalErrorEnvelope>("fatal-error.response.schema.json")?;

    Ok(())
}
