use anyhow::{Context as _, Result};
use browser_annotation_protocol::{
    BROWSER_ANNOTATION_NOT_PAIRED, BrowserAnnotationPairingMetadata, INVALID_PARAMS,
    INVALID_REQUEST, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND, PAIRING_STATE_FILE_NAME,
    PARSE_ERROR, ZED_IPC_NOT_CONNECTED, authenticated_request, parse_browser_annotation,
    parse_pairing_metadata, parse_request, read_framed_message, write_framed_message,
};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn run<R, W>(mut reader: R, mut writer: W) -> Result<()>
where
    R: Read,
    W: Write,
{
    let zed_ipc_connection = ZedIpcConnection::discover()?;
    run_with_connection(&mut reader, &mut writer, zed_ipc_connection)
}

pub fn run_with_connection<R, W>(
    reader: &mut R,
    writer: &mut W,
    zed_ipc_connection: Option<ZedIpcConnection>,
) -> Result<()>
where
    R: Read,
    W: Write,
{
    while let Some(message) = read_native_message(reader)? {
        let response = handle_message_with_connection(&message, zed_ipc_connection.as_ref());
        write_native_message(writer, &serde_json::to_vec(&response)?)?;
    }

    Ok(())
}

pub fn run_with_client<R, W>(
    reader: &mut R,
    writer: &mut W,
    zed_ipc_client: Option<&mut dyn ZedIpcClient>,
) -> Result<()>
where
    R: Read,
    W: Write,
{
    match zed_ipc_client {
        Some(zed_ipc_client) => {
            while let Some(message) = read_native_message(reader)? {
                let response = handle_message_with_client(&message, Some(zed_ipc_client));
                write_native_message(writer, &serde_json::to_vec(&response)?)?;
            }
        }
        None => {
            while let Some(message) = read_native_message(reader)? {
                let response = handle_message_with_client(&message, None);
                write_native_message(writer, &serde_json::to_vec(&response)?)?;
            }
        }
    }

    Ok(())
}

pub fn handle_message(message: &[u8]) -> JsonRpcResponse {
    handle_message_with_connection(message, None)
}

pub fn handle_message_with_client(
    message: &[u8],
    zed_ipc_client: Option<&mut dyn ZedIpcClient>,
) -> JsonRpcResponse {
    match parse_request(message) {
        Ok(request) => handle_request_with_client(request, zed_ipc_client),
        Err(error) => JsonRpcResponse::error(None, PARSE_ERROR, format!("Parse error: {error}")),
    }
}

pub trait ZedIpcClient {
    fn send(&mut self, request: &JsonRpcRequest, token: Option<&str>) -> Result<JsonRpcResponse>;
}

#[cfg(unix)]
pub struct UnixSocketZedIpcClient {
    stream: net::UnixStream,
}

#[cfg(unix)]
impl UnixSocketZedIpcClient {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            stream: net::UnixStream::connect(path)
                .context("connecting to Zed browser annotation IPC")?,
        })
    }
}

#[cfg(unix)]
impl ZedIpcClient for UnixSocketZedIpcClient {
    fn send(&mut self, request: &JsonRpcRequest, token: Option<&str>) -> Result<JsonRpcResponse> {
        let request = match token {
            Some(token) => serde_json::to_vec(&authenticated_request(request, token))?,
            None => serde_json::to_vec(request)?,
        };
        write_native_message(&mut self.stream, &request)?;
        let response = read_native_message(&mut self.stream)?
            .context("Zed browser annotation IPC closed without a response")?;
        serde_json::from_slice(&response).context("parsing Zed browser annotation IPC response")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZedIpcConnection {
    socket_path: PathBuf,
    token: Option<String>,
}

impl ZedIpcConnection {
    pub fn discover() -> Result<Option<Self>> {
        if let Some(connection) = zed_ipc_connection_from_state_file()? {
            return Ok(Some(connection));
        }

        zed_ipc_connection_from_env()
    }

    fn connect(&self) -> Result<Box<dyn ZedIpcClient>> {
        zed_ipc_client_from_socket_path(&self.socket_path)
    }

    fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }
}

fn zed_ipc_connection_from_state_file() -> Result<Option<ZedIpcConnection>> {
    let state_file = pairing_state_file();
    read_pairing_metadata_from_state_file(&state_file).with_context(|| {
        format!(
            "reading browser annotation pairing state from {}",
            state_file.display()
        )
    })
}

fn read_pairing_metadata_from_state_file(path: &Path) -> Result<Option<ZedIpcConnection>> {
    let metadata = match fs::read(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("reading browser annotation pairing state file"),
    };
    let metadata = parse_pairing_metadata(&metadata)?;
    Ok(Some(ZedIpcConnection::from(metadata)))
}

impl From<BrowserAnnotationPairingMetadata> for ZedIpcConnection {
    fn from(metadata: BrowserAnnotationPairingMetadata) -> Self {
        Self {
            socket_path: metadata.socket_path,
            token: Some(metadata.token),
        }
    }
}

fn pairing_state_file() -> PathBuf {
    std::env::var_os("ZED_BROWSER_ANNOTATION_STATE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_pairing_state_file())
}

fn default_pairing_state_file() -> PathBuf {
    state_dir().join(PAIRING_STATE_FILE_NAME)
}

fn state_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return dirs::home_dir()
            .expect("failed to determine home directory")
            .join(".local")
            .join("state")
            .join("Zed");
    }

    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return std::env::var_os("FLATPAK_XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(dirs::state_dir)
            .expect("failed to determine XDG_STATE_HOME directory")
            .join("zed");
    }

    dirs::data_local_dir()
        .expect("failed to determine LocalAppData directory")
        .join("Zed")
}

fn zed_ipc_connection_from_env() -> Result<Option<ZedIpcConnection>> {
    let Some(socket_path) = std::env::var_os("ZED_BROWSER_ANNOTATION_SOCKET") else {
        return Ok(None);
    };

    Ok(Some(ZedIpcConnection {
        socket_path: PathBuf::from(socket_path),
        token: std::env::var("ZED_BROWSER_ANNOTATION_TOKEN").ok(),
    }))
}

fn zed_ipc_client_from_socket_path(socket_path: &Path) -> Result<Box<dyn ZedIpcClient>> {
    #[cfg(unix)]
    {
        Ok(Box::new(UnixSocketZedIpcClient::connect(socket_path)?))
    }

    #[cfg(not(unix))]
    {
        anyhow::bail!("Zed browser annotation IPC is not supported on this platform yet")
    }
}

fn handle_message_with_connection(
    message: &[u8],
    zed_ipc_connection: Option<&ZedIpcConnection>,
) -> JsonRpcResponse {
    match parse_request(message) {
        Ok(request) => handle_request_with_connection(request, zed_ipc_connection),
        Err(error) => JsonRpcResponse::error(None, PARSE_ERROR, format!("Parse error: {error}")),
    }
}

fn handle_request_with_connection(
    request: JsonRpcRequest,
    zed_ipc_connection: Option<&ZedIpcConnection>,
) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse::error(request.id, INVALID_REQUEST, "Invalid JSON-RPC version");
    }

    match request.method.as_str() {
        "browserAnnotation.ping" => JsonRpcResponse::success(request.id, json!({ "ok": true })),
        "browserAnnotation.insert" => {
            handle_insert_annotation_with_connection(request, zed_ipc_connection)
        }
        _ => JsonRpcResponse::error(
            request.id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

fn handle_request_with_client(
    request: JsonRpcRequest,
    zed_ipc_client: Option<&mut dyn ZedIpcClient>,
) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse::error(request.id, INVALID_REQUEST, "Invalid JSON-RPC version");
    }

    match request.method.as_str() {
        "browserAnnotation.ping" => JsonRpcResponse::success(request.id, json!({ "ok": true })),
        "browserAnnotation.insert" => handle_insert_annotation_with_client(request, zed_ipc_client),
        _ => JsonRpcResponse::error(
            request.id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

fn handle_insert_annotation_with_connection(
    request: JsonRpcRequest,
    zed_ipc_connection: Option<&ZedIpcConnection>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    if let Some(error) = validate_insert_annotation(&request) {
        return error;
    }

    let Some(zed_ipc_connection) = zed_ipc_connection else {
        return JsonRpcResponse::error(
            id,
            BROWSER_ANNOTATION_NOT_PAIRED,
            format!(
                "Browser annotations are not paired with Zed. Open Zed and pair the browser extension; expected pairing state at {}.",
                pairing_state_file().display()
            ),
        );
    };

    let mut zed_ipc_client = match zed_ipc_connection.connect() {
        Ok(zed_ipc_client) => zed_ipc_client,
        Err(error) => {
            return JsonRpcResponse::error(
                id,
                ZED_IPC_NOT_CONNECTED,
                format!(
                    "Zed is not available for browser annotations. Open Zed and make sure pairing is active. Failed to connect to {}: {error}",
                    zed_ipc_connection.socket_path.display()
                ),
            );
        }
    };

    match zed_ipc_client.send(&request, zed_ipc_connection.token()) {
        Ok(response) => response,
        Err(error) => JsonRpcResponse::error(
            id,
            ZED_IPC_NOT_CONNECTED,
            format!("Zed browser annotation request failed: {error}"),
        ),
    }
}

fn handle_insert_annotation_with_client(
    request: JsonRpcRequest,
    zed_ipc_client: Option<&mut dyn ZedIpcClient>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    if let Some(error) = validate_insert_annotation(&request) {
        return error;
    }

    let Some(zed_ipc_client) = zed_ipc_client else {
        return JsonRpcResponse::error(
            id,
            BROWSER_ANNOTATION_NOT_PAIRED,
            "Browser annotations are not paired with Zed. Open Zed and pair the browser extension.",
        );
    };

    match zed_ipc_client.send(&request, None) {
        Ok(response) => response,
        Err(error) => JsonRpcResponse::error(
            id,
            ZED_IPC_NOT_CONNECTED,
            format!("Zed IPC request failed: {error}"),
        ),
    }
}

fn validate_insert_annotation(request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let annotation = match parse_browser_annotation(request.params.clone()) {
        Ok(annotation) => annotation,
        Err(error) => {
            return Some(JsonRpcResponse::error(
                request.id.clone(),
                INVALID_PARAMS,
                error.to_string(),
            ));
        }
    };

    if annotation.url.trim().is_empty() {
        return Some(JsonRpcResponse::error(
            request.id.clone(),
            INVALID_PARAMS,
            "Annotation URL is required",
        ));
    }

    None
}

pub fn read_native_message(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    read_framed_message(reader).context("reading native message")
}

pub fn write_native_message(writer: &mut impl Write, message: &[u8]) -> Result<()> {
    write_framed_message(writer, message).context("writing native message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::Cursor,
        sync::mpsc,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct FakeZedIpcClient {
        response: JsonRpcResponse,
        method: Option<String>,
        token: Option<String>,
    }

    impl ZedIpcClient for FakeZedIpcClient {
        fn send(
            &mut self,
            request: &JsonRpcRequest,
            token: Option<&str>,
        ) -> Result<JsonRpcResponse> {
            self.method = Some(request.method.clone());
            self.token = token.map(str::to_string);
            Ok(self.response.clone())
        }
    }

    #[test]
    fn handles_ping() {
        let response =
            handle_message(br#"{"jsonrpc":"2.0","id":1,"method":"browserAnnotation.ping"}"#);

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
    }

    #[test]
    fn rejects_unknown_method() {
        let response =
            handle_message(br#"{"jsonrpc":"2.0","id":"abc","method":"browserAnnotation.missing"}"#);

        assert_eq!(
            response,
            JsonRpcResponse::error(
                Some(json!("abc")),
                METHOD_NOT_FOUND,
                "Method not found: browserAnnotation.missing"
            )
        );
    }

    #[test]
    fn reads_and_writes_native_message_frames() {
        let mut output = Vec::new();
        write_native_message(&mut output, br#"{"jsonrpc":"2.0"}"#).unwrap();

        let mut cursor = Cursor::new(output);
        let message = read_native_message(&mut cursor).unwrap();

        assert_eq!(message.as_deref(), Some(br#"{"jsonrpc":"2.0"}"#.as_slice()));
    }

    #[test]
    fn insert_annotation_requires_zed_ipc() {
        let response = handle_message(
            br#"{"jsonrpc":"2.0","id":2,"method":"browserAnnotation.insert","params":{"url":"https://example.com","title":"Example","selected_text":"Text","selector":"main","comment":"Review this"}}"#,
        );

        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(BROWSER_ANNOTATION_NOT_PAIRED)
        );
        assert!(
            response
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("pair the browser extension"))
        );
    }

    #[test]
    fn insert_annotation_forwards_to_zed_ipc() {
        let mut zed_ipc_client = FakeZedIpcClient {
            response: JsonRpcResponse::success(Some(json!(2)), json!({ "ok": true })),
            method: None,
            token: None,
        };
        let response = handle_message_with_client(
            br#"{"jsonrpc":"2.0","id":2,"method":"browserAnnotation.insert","params":{"url":"https://example.com","title":"Example","selected_text":"Text","selector":"main","comment":"Review this"}}"#,
            Some(&mut zed_ipc_client),
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(2)), json!({ "ok": true }))
        );
        assert_eq!(
            zed_ipc_client.method.as_deref(),
            Some("browserAnnotation.insert")
        );
    }

    #[test]
    fn reads_pairing_metadata_from_state_file() {
        let state_file = unique_test_path("pairing-state", "json");
        fs::write(
            &state_file,
            br#"{"socket_path":"/tmp/zed-browser-annotation.sock","token":"secret"}"#,
        )
        .expect("write state file");

        let connection = read_pairing_metadata_from_state_file(&state_file)
            .expect("read state")
            .expect("connection");

        assert_eq!(
            connection,
            ZedIpcConnection {
                socket_path: PathBuf::from("/tmp/zed-browser-annotation.sock"),
                token: Some("secret".to_string())
            }
        );
        fs::remove_file(&state_file).expect("remove state file");
    }

    #[test]
    fn missing_pairing_metadata_state_file_is_unpaired() {
        let state_file = unique_test_path("missing-pairing-state", "json");

        let connection =
            read_pairing_metadata_from_state_file(&state_file).expect("read missing state");

        assert_eq!(connection, None);
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_client_sends_request_to_zed() {
        let socket_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let socket_path = std::path::PathBuf::from("/tmp").join(format!(
            "zed-browser-annotation-host-test-{}-{socket_id}.sock",
            std::process::id(),
        ));
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind listener");
        let (ready_tx, ready_rx) = mpsc::channel();

        let server_thread = thread::spawn(move || {
            ready_tx.send(()).expect("signal ready");
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_framed_message(&mut stream)
                .expect("read request")
                .expect("request");
            let request: serde_json::Value =
                serde_json::from_slice(&request).expect("parse request");
            assert_eq!(request["method"], "browserAnnotation.insert");
            assert_eq!(request["token"], "secret");
            write_framed_message(
                &mut stream,
                &serde_json::to_vec(&JsonRpcResponse::success(
                    request.get("id").cloned(),
                    json!({ "ok": true }),
                ))
                .expect("serialize response"),
            )
            .expect("write response");
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("server ready");

        let mut client = UnixSocketZedIpcClient::connect(&socket_path).expect("connect client");
        let response = client
            .send(
                &JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    id: Some(json!(2)),
                    method: "browserAnnotation.insert".to_string(),
                    params: json!({ "url": "https://example.com" }),
                },
                Some("secret"),
            )
            .expect("send request");

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(2)), json!({ "ok": true }))
        );
        server_thread.join().expect("server thread");
        std::fs::remove_file(socket_path).expect("remove socket");
    }

    #[cfg(unix)]
    #[test]
    fn insert_annotation_reports_zed_unavailable_when_socket_is_missing() {
        let socket_path = unique_test_path("missing-zed-socket", "sock");
        let response = handle_message_with_connection(
            br#"{"jsonrpc":"2.0","id":2,"method":"browserAnnotation.insert","params":{"url":"https://example.com"}}"#,
            Some(&ZedIpcConnection {
                socket_path,
                token: Some("secret".to_string()),
            }),
        );

        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ZED_IPC_NOT_CONNECTED)
        );
        assert!(
            response
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("Open Zed"))
        );
    }

    #[test]
    fn insert_annotation_requires_url() {
        let response = handle_message(
            br#"{"jsonrpc":"2.0","id":2,"method":"browserAnnotation.insert","params":{"url":" "}}"#,
        );

        assert_eq!(
            response,
            JsonRpcResponse::error(Some(json!(2)), INVALID_PARAMS, "Annotation URL is required")
        );
    }

    #[test]
    fn insert_annotation_rejects_invalid_params() {
        let response = handle_message(
            br#"{"jsonrpc":"2.0","id":2,"method":"browserAnnotation.insert","params":[]}"#,
        );

        assert!(
            response
                .error
                .as_ref()
                .is_some_and(|error| error.code == INVALID_PARAMS)
        );
    }

    fn unique_test_path(label: &str, extension: &str) -> PathBuf {
        let path_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "zed-browser-annotation-host-test-{label}-{}-{path_id}.{extension}",
            std::process::id(),
        ))
    }
}
