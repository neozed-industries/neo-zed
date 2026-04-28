use agent_ui::AgentPanel;
use anyhow::{Context as _, Result};
use browser_annotation_protocol::{
    BROWSER_ANNOTATION_NOT_PAIRED, BrowserAnnotation, BrowserAnnotationPairingMetadata,
    INVALID_PARAMS, INVALID_REQUEST, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND,
    PAIRING_STATE_FILE_NAME, PARSE_ERROR, ZED_AGENT_PANEL_UNAVAILABLE, parse_authenticated_request,
    parse_browser_annotation, parse_pairing_metadata, read_framed_message, write_framed_message,
};
use futures::{StreamExt as _, channel::mpsc};
use gpui::{App, AsyncApp, Global};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::Duration,
};
use uuid::Uuid;
use workspace::AppState;

#[cfg(unix)]
use std::os::unix::net::UnixListener;

trait BrowserAnnotationTarget: Send + Sync {
    fn insert_annotation(&self, annotation: agent_ui::BrowserAnnotation) -> Result<()>;
}

#[derive(Clone)]
struct ForegroundBrowserAnnotationTarget {
    tx: mpsc::UnboundedSender<ForegroundBrowserAnnotationRequest>,
}

struct ForegroundBrowserAnnotationRequest {
    annotation: agent_ui::BrowserAnnotation,
    response_tx: std_mpsc::Sender<Result<()>>,
}

impl BrowserAnnotationTarget for ForegroundBrowserAnnotationTarget {
    fn insert_annotation(&self, annotation: agent_ui::BrowserAnnotation) -> Result<()> {
        let (response_tx, response_rx) = std_mpsc::channel();
        self.tx
            .unbounded_send(ForegroundBrowserAnnotationRequest {
                annotation,
                response_tx,
            })
            .context("browser annotation foreground target is not running")?;
        response_rx
            .recv()
            .context("browser annotation foreground target stopped")?
    }
}

fn to_agent_ui_browser_annotation(annotation: BrowserAnnotation) -> agent_ui::BrowserAnnotation {
    agent_ui::BrowserAnnotation {
        url: annotation.url,
        title: annotation.title,
        selected_text: annotation.selected_text,
        selector: annotation.selector,
        comment: annotation.comment,
    }
}

pub fn init(app_state: Arc<AppState>, cx: &mut App) {
    let (tx, mut rx) = mpsc::unbounded::<ForegroundBrowserAnnotationRequest>();
    cx.spawn(async move |cx| {
        while let Some(request) = rx.next().await {
            let result = insert_browser_annotation(request.annotation, app_state.clone(), cx).await;
            if request.response_tx.send(result).is_err() {
                log::warn!("browser annotation IPC client dropped before receiving response");
            }
        }
    })
    .detach();

    match BrowserAnnotationIpcServer::start(ForegroundBrowserAnnotationTarget { tx }) {
        Ok(server) => {
            log::info!(
                "browser annotation IPC listening on {}",
                server.metadata().socket_path.display()
            );
            cx.set_global(BrowserAnnotationIpcServerGlobal(server));
        }
        Err(error) => {
            log::error!("failed to start browser annotation IPC server: {error:#}");
        }
    }
}

pub fn handle_pairing_deep_link(token: Option<&str>, cx: &App) -> Result<()> {
    let Some(server) = cx.try_global::<BrowserAnnotationIpcServerGlobal>() else {
        anyhow::bail!("browser annotation IPC server is not running");
    };

    if let Some(token) = token
        && token != server.0.metadata().token
    {
        anyhow::bail!("browser annotation pairing token did not match");
    }

    Ok(())
}

async fn insert_browser_annotation(
    annotation: agent_ui::BrowserAnnotation,
    app_state: Arc<AppState>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let multi_workspace = workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

    cx.update(|cx| cx.activate(true));
    multi_workspace.update(cx, |multi_workspace, window, cx| {
        multi_workspace.workspace().update(cx, |workspace, cx| {
            let panel = workspace
                .focus_panel::<AgentPanel>(window, cx)
                .context("No active detached agent panel is available")?;
            panel.update(cx, |panel, cx| {
                panel.append_browser_annotation_to_active_thread(annotation, window, cx)
            })
        })
    })?
}

struct BrowserAnnotationIpcServerGlobal(BrowserAnnotationIpcServer);

impl Global for BrowserAnnotationIpcServerGlobal {}

struct BrowserAnnotationIpcServer {
    metadata: BrowserAnnotationPairingMetadata,
    metadata_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl BrowserAnnotationIpcServer {
    fn start(target: ForegroundBrowserAnnotationTarget) -> Result<Self> {
        Self::start_with_paths(
            short_socket_path(),
            pairing_metadata_path(),
            Uuid::new_v4().simple().to_string(),
            target,
        )
    }

    fn metadata(&self) -> &BrowserAnnotationPairingMetadata {
        &self.metadata
    }

    #[cfg(unix)]
    fn start_with_paths(
        socket_path: PathBuf,
        metadata_path: PathBuf,
        token: String,
        target: ForegroundBrowserAnnotationTarget,
    ) -> Result<Self> {
        let listener = bind_listener(&socket_path)?;
        listener
            .set_nonblocking(true)
            .context("setting browser annotation IPC listener nonblocking")?;

        let metadata = BrowserAnnotationPairingMetadata { socket_path, token };
        write_pairing_metadata(&metadata_path, &metadata)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = shutdown.clone();
        let thread_metadata = metadata.clone();
        let thread = thread::Builder::new()
            .name("browser-annotation-ipc".to_string())
            .spawn(move || {
                run_server(listener, thread_metadata, target, thread_shutdown);
            })
            .context("spawning browser annotation IPC server thread")?;

        Ok(Self {
            metadata,
            metadata_path,
            shutdown,
            thread: Some(thread),
        })
    }

    #[cfg(not(unix))]
    fn start_with_paths(
        _socket_path: PathBuf,
        _metadata_path: PathBuf,
        _token: String,
        _target: ForegroundBrowserAnnotationTarget,
    ) -> Result<Self> {
        anyhow::bail!("browser annotation IPC is not supported on this platform yet")
    }
}

impl Drop for BrowserAnnotationIpcServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);

        #[cfg(unix)]
        if let Err(error) = std::os::unix::net::UnixStream::connect(&self.metadata.socket_path) {
            log::debug!("failed to wake browser annotation IPC server during shutdown: {error}");
        }

        if let Some(thread) = self.thread.take()
            && let Err(error) = thread.join()
        {
            log::warn!("browser annotation IPC server thread panicked: {error:?}");
        }

        remove_pairing_metadata(&self.metadata_path, &self.metadata);

        #[cfg(unix)]
        if self.metadata.socket_path.exists()
            && let Err(error) = fs::remove_file(&self.metadata.socket_path)
        {
            log::warn!(
                "failed to remove browser annotation IPC socket {}: {error}",
                self.metadata.socket_path.display()
            );
        }
    }
}

#[cfg(unix)]
fn bind_listener(socket_path: &Path) -> Result<UnixListener> {
    if socket_path.exists() {
        fs::remove_file(socket_path).with_context(|| {
            format!(
                "removing stale browser annotation IPC socket {}",
                socket_path.display()
            )
        })?;
    }

    UnixListener::bind(socket_path).with_context(|| {
        format!(
            "binding browser annotation IPC socket {}",
            socket_path.display()
        )
    })
}

#[cfg(unix)]
fn run_server(
    listener: UnixListener,
    metadata: BrowserAnnotationPairingMetadata,
    target: ForegroundBrowserAnnotationTarget,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) = serve_connection(&mut stream, &metadata.token, Some(&target)) {
                    log::warn!("browser annotation IPC connection failed: {error:#}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                log::warn!("browser annotation IPC accept failed: {error}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn short_socket_path() -> PathBuf {
    let suffix = Uuid::new_v4().simple().to_string();
    PathBuf::from("/tmp").join(format!(
        "zed-ba-{}-{}.sock",
        std::process::id(),
        &suffix[..8]
    ))
}

fn pairing_metadata_path() -> PathBuf {
    paths::state_dir().join(PAIRING_STATE_FILE_NAME)
}

fn write_pairing_metadata(
    metadata_path: &Path,
    metadata: &BrowserAnnotationPairingMetadata,
) -> Result<()> {
    if let Some(parent) = metadata_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "creating browser annotation pairing metadata directory {}",
                parent.display()
            )
        })?;
    }

    fs::write(metadata_path, serde_json::to_vec(metadata)?).with_context(|| {
        format!(
            "writing browser annotation pairing metadata {}",
            metadata_path.display()
        )
    })?;
    Ok(())
}

fn remove_pairing_metadata(metadata_path: &Path, metadata: &BrowserAnnotationPairingMetadata) {
    let should_remove = fs::read(metadata_path)
        .ok()
        .and_then(|contents| parse_pairing_metadata(&contents).ok())
        .is_some_and(|current| current == *metadata);

    if should_remove && let Err(error) = fs::remove_file(metadata_path) {
        log::warn!(
            "failed to remove browser annotation pairing metadata {}: {error}",
            metadata_path.display()
        );
    }
}

fn serve_connection(
    stream: &mut (impl Read + Write),
    token: &str,
    target: Option<&dyn BrowserAnnotationTarget>,
) -> Result<()> {
    while let Some(message) = read_framed_message(stream)? {
        let response = handle_message_with_target(&message, token, target);
        write_framed_message(stream, &serde_json::to_vec(&response)?)?;
    }

    Ok(())
}

#[cfg(test)]
fn handle_message(message: &[u8], token: &str) -> JsonRpcResponse {
    handle_message_with_target(message, token, None)
}

fn handle_message_with_target(
    message: &[u8],
    token: &str,
    target: Option<&dyn BrowserAnnotationTarget>,
) -> JsonRpcResponse {
    match parse_authenticated_request(message) {
        Ok((request, request_token)) => handle_request(request, request_token, token, target),
        Err(error) => JsonRpcResponse::error(None, PARSE_ERROR, format!("Parse error: {error}")),
    }
}

fn handle_request(
    request: JsonRpcRequest,
    request_token: Option<String>,
    token: &str,
    target: Option<&dyn BrowserAnnotationTarget>,
) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse::error(request.id, INVALID_REQUEST, "Invalid JSON-RPC version");
    }

    if request_token.as_deref() != Some(token) {
        return JsonRpcResponse::error(
            request.id,
            BROWSER_ANNOTATION_NOT_PAIRED,
            "Browser annotation client is not paired with this Zed instance",
        );
    }

    match request.method.as_str() {
        "browserAnnotation.ping" => JsonRpcResponse::success(request.id, json!({ "ok": true })),
        "browserAnnotation.insert" => handle_insert_annotation(request, target),
        _ => JsonRpcResponse::error(
            request.id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

fn handle_insert_annotation(
    request: JsonRpcRequest,
    target: Option<&dyn BrowserAnnotationTarget>,
) -> JsonRpcResponse {
    let id = request.id;
    let annotation = match parse_browser_annotation(request.params) {
        Ok(annotation) => annotation,
        Err(error) => return JsonRpcResponse::error(id, INVALID_PARAMS, error.to_string()),
    };

    if annotation.url.trim().is_empty() {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Annotation URL is required");
    }

    let Some(target) = target else {
        return JsonRpcResponse::error(
            id,
            ZED_AGENT_PANEL_UNAVAILABLE,
            "No active detached agent panel is available",
        );
    };

    let annotation = to_agent_ui_browser_annotation(annotation);
    match target.insert_annotation(annotation) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(error) => JsonRpcResponse::error(id, ZED_AGENT_PANEL_UNAVAILABLE, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::Cursor,
        sync::{Mutex, mpsc as std_mpsc},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[derive(Default)]
    struct FakeBrowserAnnotationTarget {
        annotations: Mutex<Vec<agent_ui::BrowserAnnotation>>,
    }

    impl BrowserAnnotationTarget for FakeBrowserAnnotationTarget {
        fn insert_annotation(&self, annotation: agent_ui::BrowserAnnotation) -> Result<()> {
            self.annotations
                .lock()
                .expect("annotations lock")
                .push(annotation);
            Ok(())
        }
    }

    fn authenticated_request(method: &str, params: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "token": "secret",
            "params": params,
        }))
        .expect("serialize request")
    }

    #[test]
    fn handles_authenticated_ping() {
        let response = handle_message(
            &authenticated_request("browserAnnotation.ping", json!({})),
            "secret",
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
    }

    #[test]
    fn rejects_unpaired_request() {
        let response = handle_message(
            br#"{"jsonrpc":"2.0","id":1,"method":"browserAnnotation.ping"}"#,
            "secret",
        );

        assert_eq!(
            response,
            JsonRpcResponse::error(
                Some(json!(1)),
                BROWSER_ANNOTATION_NOT_PAIRED,
                "Browser annotation client is not paired with this Zed instance"
            )
        );
    }

    #[test]
    fn rejects_unknown_method() {
        let response = handle_message(
            &authenticated_request("browserAnnotation.missing", json!({})),
            "secret",
        );

        assert_eq!(
            response,
            JsonRpcResponse::error(
                Some(json!(1)),
                METHOD_NOT_FOUND,
                "Method not found: browserAnnotation.missing"
            )
        );
    }

    #[test]
    fn rejects_parse_error() {
        let response = handle_message(br#"{"jsonrpc":"2.0""#, "secret");

        assert!(
            response
                .error
                .as_ref()
                .is_some_and(|error| error.code == PARSE_ERROR)
        );
    }

    #[test]
    fn insert_annotation_requires_agent_panel_target() {
        let response = handle_message(
            &authenticated_request(
                "browserAnnotation.insert",
                json!({
                    "url": "https://example.com",
                    "title": "Example",
                    "selected_text": "Text",
                    "selector": "main",
                    "comment": "Review this",
                }),
            ),
            "secret",
        );

        assert_eq!(
            response,
            JsonRpcResponse::error(
                Some(json!(1)),
                ZED_AGENT_PANEL_UNAVAILABLE,
                "No active detached agent panel is available"
            )
        );
    }

    #[test]
    fn insert_annotation_uses_target() {
        let target = FakeBrowserAnnotationTarget::default();
        let response = handle_message_with_target(
            &authenticated_request(
                "browserAnnotation.insert",
                json!({
                    "url": "https://example.com",
                    "title": "Example",
                    "selected_text": "Text",
                    "selector": "main",
                    "comment": "Review this",
                }),
            ),
            "secret",
            Some(&target),
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
        let annotations = target.annotations.lock().expect("annotations lock");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].url, "https://example.com");
    }

    #[test]
    fn converts_protocol_annotation_to_agent_ui_annotation() {
        let annotation = to_agent_ui_browser_annotation(BrowserAnnotation {
            url: "https://example.com".to_string(),
            title: Some("Example".to_string()),
            selected_text: Some("Selected text".to_string()),
            selector: Some("main".to_string()),
            comment: Some("Review this".to_string()),
        });

        assert_eq!(annotation.url, "https://example.com");
        assert_eq!(annotation.title.as_deref(), Some("Example"));
        assert_eq!(annotation.selected_text.as_deref(), Some("Selected text"));
        assert_eq!(annotation.selector.as_deref(), Some("main"));
        assert_eq!(annotation.comment.as_deref(), Some("Review this"));
    }

    #[test]
    fn insert_annotation_requires_url() {
        let response = handle_message(
            &authenticated_request("browserAnnotation.insert", json!({ "url": " " })),
            "secret",
        );

        assert_eq!(
            response,
            JsonRpcResponse::error(Some(json!(1)), INVALID_PARAMS, "Annotation URL is required")
        );
    }

    #[test]
    fn serves_framed_ping_connection() {
        let mut stream = Cursor::new(Vec::new());
        write_framed_message(
            &mut stream,
            &authenticated_request("browserAnnotation.ping", json!({})),
        )
        .expect("write request");
        stream.set_position(0);

        serve_connection(&mut stream, "secret", None).expect("serve connection");

        let output_start =
            (4 + authenticated_request("browserAnnotation.ping", json!({})).len()) as u64;
        stream.set_position(output_start);
        let response = read_framed_message(&mut stream)
            .expect("read response")
            .expect("response");
        let response: JsonRpcResponse = serde_json::from_slice(&response).expect("parse response");

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
    }

    #[test]
    fn socket_path_is_short_for_unix_socket_limits() {
        let socket_path = short_socket_path();

        assert!(
            socket_path.as_os_str().len() < 100,
            "socket path was too long: {}",
            socket_path.display()
        );
        assert!(socket_path.starts_with("/tmp"));
    }

    #[cfg(unix)]
    #[test]
    fn server_lifecycle_writes_and_removes_pairing_metadata() {
        let socket_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let socket_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-lifecycle-{}-{socket_id}.sock",
            std::process::id(),
        ));
        let metadata_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-lifecycle-{}-{socket_id}.json",
            std::process::id(),
        ));
        let (tx, _rx) = mpsc::unbounded();

        {
            let server = BrowserAnnotationIpcServer::start_with_paths(
                socket_path.clone(),
                metadata_path.clone(),
                "secret".to_string(),
                ForegroundBrowserAnnotationTarget { tx },
            )
            .expect("start server");

            assert_eq!(server.metadata().socket_path, socket_path);
            let contents = fs::read(&metadata_path).expect("read metadata");
            let metadata = parse_pairing_metadata(&contents).expect("parse metadata");
            assert_eq!(metadata.socket_path, socket_path);
            assert_eq!(metadata.token, "secret");
        }

        assert!(!metadata_path.exists());
        assert!(!socket_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn serves_ping_over_unix_socket() {
        let socket_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let socket_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-test-{}-{socket_id}.sock",
            std::process::id(),
        ));
        let metadata_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-test-{}-{socket_id}.json",
            std::process::id(),
        ));
        let (foreground_tx, _foreground_rx) = mpsc::unbounded();
        let server = BrowserAnnotationIpcServer::start_with_paths(
            socket_path.clone(),
            metadata_path,
            "secret".to_string(),
            ForegroundBrowserAnnotationTarget { tx: foreground_tx },
        )
        .expect("start server");
        let (ready_tx, ready_rx) = std_mpsc::channel();

        let client_thread = thread::spawn(move || {
            ready_tx.send(()).expect("signal ready");
            let mut stream = std::os::unix::net::UnixStream::connect(socket_path).expect("connect");
            write_framed_message(
                &mut stream,
                &authenticated_request("browserAnnotation.ping", json!({})),
            )
            .expect("write request");
            let response = read_framed_message(&mut stream)
                .expect("read response")
                .expect("response");
            serde_json::from_slice::<JsonRpcResponse>(&response).expect("parse response")
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("client ready");

        let response = client_thread.join().expect("client thread");
        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
        drop(server);
    }
}
