//! Assertion helpers for CLI integration tests.

use serde_json::Value;

/// Parses a JSON envelope from command output.
pub fn parse_envelope(output: &[u8]) -> Value {
    serde_json::from_slice(output).expect("json")
}

/// Extracts the `result` object from a successful JSON envelope.
pub fn extract_ok_data(output: &[u8]) -> Value {
    let value = parse_envelope(output);
    let status = value["status"].as_str().expect("expected status field");
    assert!(
        matches!(status, "ok" | "warning"),
        "expected ok/warning status envelope, got {status}"
    );
    assert!(value.get("meta").is_some(), "expected meta field");
    assert!(value.get("result").is_some(), "expected result field");
    assert!(value.get("error").is_some(), "expected error field");
    assert!(
        value["error"].is_null(),
        "expected error=null for ok/warning"
    );
    value.get("result").cloned().expect("result")
}

/// Extracts error code from a failed JSON envelope.
pub fn extract_error_code(output: &[u8]) -> String {
    let value = parse_envelope(output);
    assert_eq!(value["status"], "error", "expected status=error envelope");
    assert!(value.get("meta").is_some(), "expected meta field");
    assert!(value.get("result").is_some(), "expected result field");
    assert!(value["result"].is_null(), "expected result=null for error");
    value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(|code| code.as_str())
        .expect("error code")
        .to_string()
}

/// Extracts error payload from a failed JSON envelope.
pub fn extract_error_payload(output: &[u8]) -> Value {
    let value = parse_envelope(output);
    assert_eq!(value["status"], "error", "expected status=error envelope");
    assert!(value.get("meta").is_some(), "expected meta field");
    assert!(value.get("result").is_some(), "expected result field");
    assert!(value["result"].is_null(), "expected result=null for error");
    value.get("error").cloned().expect("error")
}

/// Extracts machine-readable error details from a failed JSON envelope.
pub fn extract_error_details(output: &[u8]) -> Value {
    let error = extract_error_payload(output);
    let details = error.get("details").cloned().expect("details");
    assert!(
        details.is_object(),
        "expected error.details to be an object, got {details}"
    );
    details
}

/// Asserts the envelope status value.
pub fn assert_envelope_status(output: &[u8], status: &str) {
    let value = parse_envelope(output);
    assert_eq!(value["status"], status);
}

/// Asserts a failed JSON envelope with expected error metadata.
pub fn assert_error_envelope(output: &[u8], code: &str, retry: bool) {
    let error = extract_error_payload(output);
    assert_eq!(error["code"], code);
    assert_eq!(error["retryable"], retry);
}

/// Asserts plain output contract: non-JSON and required snippets are present.
pub fn assert_plain_contract(output: &[u8], required_snippets: &[&str]) {
    let parsed = serde_json::from_slice::<Value>(output);
    assert!(parsed.is_err(), "expected plain (non-JSON) output");

    let output_str = String::from_utf8_lossy(output);
    let line_count = output_str.lines().count();
    assert!(line_count >= 2, "expected multi-line plain output");
    for snippet in required_snippets {
        assert!(
            output_str.contains(snippet),
            "expected plain output to contain `{snippet}`"
        );
    }
}
