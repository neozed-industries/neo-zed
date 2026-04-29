use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::PathBuf,
};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const ZED_IPC_NOT_CONNECTED: i64 = -32000;
pub const ZED_AGENT_PANEL_UNAVAILABLE: i64 = -32001;
pub const BROWSER_ANNOTATION_NOT_PAIRED: i64 = -32002;
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;
pub const PAIRING_STATE_FILE_NAME: &str = "browser_annotation_pairing.json";

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, rename = "params")]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct AuthenticatedJsonRpcRequest<'a> {
    pub jsonrpc: &'a str,
    pub id: &'a Option<Value>,
    pub method: &'a str,
    pub token: &'a str,
    #[serde(rename = "params")]
    pub params: &'a Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BrowserAnnotation {
    pub id: Option<String>,
    pub url: String,
    pub title: Option<String>,
    pub selected_text: Option<String>,
    pub selector: Option<String>,
    pub comment: Option<String>,
    pub focus_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BrowserAnnotationSync {
    #[serde(default)]
    pub annotations: Vec<BrowserAnnotation>,
    #[serde(default)]
    pub submit: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BrowserAnnotationPairingMetadata {
    pub socket_path: PathBuf,
    pub token: String,
}

pub fn parse_request(message: &[u8]) -> Result<JsonRpcRequest> {
    serde_json::from_slice(message).context("parsing JSON-RPC request")
}

pub fn parse_authenticated_request(message: &[u8]) -> Result<(JsonRpcRequest, Option<String>)> {
    let value: Value = serde_json::from_slice(message).context("parsing JSON-RPC request")?;
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .map(str::to_string);
    let request = serde_json::from_value(value).context("parsing JSON-RPC request")?;
    Ok((request, token))
}

pub fn parse_browser_annotation(params: Value) -> Result<BrowserAnnotation> {
    serde_json::from_value(params).context("invalid browser annotation params")
}

pub fn parse_browser_annotation_sync(params: Value) -> Result<BrowserAnnotationSync> {
    serde_json::from_value(params).context("invalid browser annotation sync params")
}

pub fn authenticated_request<'a>(
    request: &'a JsonRpcRequest,
    token: &'a str,
) -> AuthenticatedJsonRpcRequest<'a> {
    AuthenticatedJsonRpcRequest {
        jsonrpc: &request.jsonrpc,
        id: &request.id,
        method: &request.method,
        token,
        params: &request.params,
    }
}

pub fn parse_pairing_metadata(message: &[u8]) -> Result<BrowserAnnotationPairingMetadata> {
    let metadata: BrowserAnnotationPairingMetadata =
        serde_json::from_slice(message).context("parsing browser annotation pairing metadata")?;
    if metadata.socket_path.as_os_str().is_empty() {
        anyhow::bail!("browser annotation pairing metadata is missing socket_path");
    }
    if metadata.token.trim().is_empty() {
        anyhow::bail!("browser annotation pairing metadata is missing token");
    }
    Ok(metadata)
}

pub fn read_framed_message(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut length_bytes = [0; 4];
    let mut bytes_read = 0;
    while bytes_read < length_bytes.len() {
        let count = reader
            .read(&mut length_bytes[bytes_read..])
            .context("reading browser annotation message length")?;
        if count == 0 {
            return if bytes_read == 0 {
                Ok(None)
            } else {
                Err(anyhow!(
                    "unexpected EOF while reading browser annotation message length"
                ))
            };
        }
        bytes_read += count;
    }

    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_MESSAGE_SIZE {
        return Err(anyhow!("browser annotation message exceeds maximum size"));
    }

    let mut message = vec![0; length];
    reader
        .read_exact(&mut message)
        .context("reading browser annotation message body")?;
    Ok(Some(message))
}

pub fn write_framed_message(writer: &mut impl Write, message: &[u8]) -> Result<()> {
    let length = u32::try_from(message.len()).context("browser annotation message is too large")?;
    writer
        .write_all(&length.to_le_bytes())
        .context("writing browser annotation message length")?;
    writer
        .write_all(message)
        .context("writing browser annotation message body")?;
    writer
        .flush()
        .context("flushing browser annotation message")?;
    Ok(())
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn parses_browser_annotation() {
        let annotation = parse_browser_annotation(json!({
            "id": "comment-1",
            "url": "https://example.com",
            "title": "Example",
            "selected_text": "Selected text",
            "selector": "main",
            "comment": "Review this",
            "focus_url": "chrome-extension://extension/src/focus.html?tabId=1"
        }))
        .expect("parse annotation");

        assert_eq!(annotation.id.as_deref(), Some("comment-1"));
        assert_eq!(annotation.url, "https://example.com");
        assert_eq!(annotation.title.as_deref(), Some("Example"));
        assert_eq!(annotation.selected_text.as_deref(), Some("Selected text"));
        assert_eq!(annotation.selector.as_deref(), Some("main"));
        assert_eq!(annotation.comment.as_deref(), Some("Review this"));
        assert_eq!(
            annotation.focus_url.as_deref(),
            Some("chrome-extension://extension/src/focus.html?tabId=1")
        );
    }

    #[test]
    fn parses_browser_annotation_sync() {
        let sync = parse_browser_annotation_sync(json!({
            "annotations": [{ "id": "comment-1", "url": "https://example.com" }],
            "submit": true
        }))
        .expect("parse sync");

        assert_eq!(sync.annotations.len(), 1);
        assert_eq!(sync.annotations[0].id.as_deref(), Some("comment-1"));
        assert!(sync.submit);
    }

    #[test]
    fn parses_pairing_metadata() {
        let metadata = parse_pairing_metadata(
            br#"{"socket_path":"/tmp/zed-browser-annotation.sock","token":"secret"}"#,
        )
        .expect("parse metadata");

        assert_eq!(
            metadata.socket_path,
            PathBuf::from("/tmp/zed-browser-annotation.sock")
        );
        assert_eq!(metadata.token, "secret");
    }

    #[test]
    fn rejects_pairing_metadata_without_token() {
        let error = parse_pairing_metadata(br#"{"socket_path":"/tmp/socket","token":" "}"#)
            .expect_err("missing token should fail");

        assert!(
            error
                .to_string()
                .contains("browser annotation pairing metadata is missing token")
        );
    }

    #[test]
    fn serializes_authenticated_request_with_token() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "browserAnnotation.insert".to_string(),
            params: json!({ "url": "https://example.com" }),
        };
        let request =
            serde_json::to_value(authenticated_request(&request, "secret")).expect("serialize");

        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["id"], 2);
        assert_eq!(request["method"], "browserAnnotation.insert");
        assert_eq!(request["token"], "secret");
        assert_eq!(request["params"], json!({ "url": "https://example.com" }));
    }

    #[test]
    fn parses_authenticated_request_token() {
        let (request, token) = parse_authenticated_request(
            br#"{"jsonrpc":"2.0","id":1,"method":"browserAnnotation.ping","token":"secret"}"#,
        )
        .expect("parse authenticated request");

        assert_eq!(request.method, "browserAnnotation.ping");
        assert_eq!(token.as_deref(), Some("secret"));
    }

    #[test]
    fn reads_and_writes_framed_messages() {
        let mut output = Vec::new();
        write_framed_message(&mut output, br#"{"jsonrpc":"2.0"}"#).expect("write message");

        let mut cursor = Cursor::new(output);
        let message = read_framed_message(&mut cursor).expect("read message");

        assert_eq!(message.as_deref(), Some(br#"{"jsonrpc":"2.0"}"#.as_slice()));
    }

    #[test]
    fn detects_partial_frame_length() {
        let mut cursor = Cursor::new(vec![1, 0]);
        let error = read_framed_message(&mut cursor).expect_err("partial frame should fail");

        assert!(
            error
                .to_string()
                .contains("unexpected EOF while reading browser annotation message length")
        );
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut input = Vec::new();
        input.extend_from_slice(&((MAX_MESSAGE_SIZE as u32) + 1).to_le_bytes());
        let mut cursor = Cursor::new(input);
        let error = read_framed_message(&mut cursor).expect_err("oversized frame should fail");

        assert!(
            error
                .to_string()
                .contains("browser annotation message exceeds maximum size")
        );
    }
}
