use agent_ui::{AgentPanel, BrowserAnnotationFocus};
use anyhow::{Context as _, Result};
use browser_annotation_protocol::{
    BROWSER_ANNOTATION_NOT_PAIRED, BrowserAnnotation, BrowserAnnotationPairingMetadata,
    INVALID_PARAMS, INVALID_REQUEST, JsonRpcRequest, JsonRpcResponse, METHOD_NOT_FOUND,
    PAIRING_STATE_FILE_NAME, PARSE_ERROR, ZED_AGENT_PANEL_UNAVAILABLE, parse_authenticated_request,
    parse_browser_annotation, parse_browser_annotation_sync, parse_pairing_metadata,
    read_framed_message, write_framed_message,
};
use futures::{StreamExt as _, channel::mpsc};
use gpui::{App, AsyncApp, Global, Rgba};
use serde_json::json;
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use workspace::AppState;

#[cfg(unix)]
use std::os::unix::{
    fs::{OpenOptionsExt as _, PermissionsExt as _},
    net::{UnixListener, UnixStream},
};
use theme::ActiveTheme as _;

trait BrowserAnnotationIpcTarget: Send + Sync {
    fn insert_annotation(&self, annotation: agent_ui::BrowserAnnotation) -> Result<()>;
    fn sync_annotations(&self, annotations: Vec<agent_ui::BrowserAnnotation>) -> Result<()>;
    fn theme(&self) -> Result<serde_json::Value>;
}

const FOREGROUND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const IPC_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const FOCUS_FALLBACK_DELAY: Duration = Duration::from_secs(2);
const FOCUS_REQUEST_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct ForegroundBrowserAnnotationTarget {
    tx: mpsc::UnboundedSender<ForegroundBrowserAnnotationRequest>,
}

enum ForegroundBrowserAnnotationRequest {
    Sync {
        annotations: Vec<agent_ui::BrowserAnnotation>,
        response_tx: std_mpsc::Sender<Result<()>>,
    },
    Theme {
        response_tx: std_mpsc::Sender<Result<serde_json::Value>>,
    },
}

impl BrowserAnnotationIpcTarget for ForegroundBrowserAnnotationTarget {
    fn insert_annotation(&self, annotation: agent_ui::BrowserAnnotation) -> Result<()> {
        let (response_tx, response_rx) = std_mpsc::channel();
        self.tx
            .unbounded_send(ForegroundBrowserAnnotationRequest::Sync {
                annotations: vec![annotation],
                response_tx,
            })
            .context("browser annotation foreground target is not running")?;
        response_rx
            .recv_timeout(FOREGROUND_RESPONSE_TIMEOUT)
            .context("browser annotation foreground target stopped or timed out")?
    }

    fn sync_annotations(&self, annotations: Vec<agent_ui::BrowserAnnotation>) -> Result<()> {
        let (response_tx, response_rx) = std_mpsc::channel();
        self.tx
            .unbounded_send(ForegroundBrowserAnnotationRequest::Sync {
                annotations,
                response_tx,
            })
            .context("browser annotation foreground target is not running")?;
        response_rx
            .recv_timeout(FOREGROUND_RESPONSE_TIMEOUT)
            .context("browser annotation foreground target stopped or timed out")?
    }

    fn theme(&self) -> Result<serde_json::Value> {
        let (response_tx, response_rx) = std_mpsc::channel();
        self.tx
            .unbounded_send(ForegroundBrowserAnnotationRequest::Theme { response_tx })
            .context("browser annotation foreground target is not running")?;
        response_rx
            .recv_timeout(FOREGROUND_RESPONSE_TIMEOUT)
            .context("browser annotation foreground target stopped or timed out")?
    }
}

fn to_agent_ui_browser_annotation(annotation: BrowserAnnotation) -> agent_ui::BrowserAnnotation {
    agent_ui::BrowserAnnotation {
        id: annotation.id,
        url: annotation.url,
        title: annotation.title,
        selected_text: annotation.selected_text,
        selector: annotation.selector,
        comment: annotation.comment,
        focus_url: annotation.focus_url,
    }
}

pub fn init(app_state: Arc<AppState>, cx: &mut App) {
    let (tx, mut rx) = mpsc::unbounded::<ForegroundBrowserAnnotationRequest>();
    cx.spawn(async move |cx| {
        while let Some(request) = rx.next().await {
            match request {
                ForegroundBrowserAnnotationRequest::Sync {
                    annotations,
                    response_tx,
                } => {
                    let result = sync_browser_annotations(annotations, app_state.clone(), cx).await;
                    if response_tx.send(result).is_err() {
                        log::warn!(
                            "browser annotation IPC client dropped before receiving response"
                        );
                    }
                }
                ForegroundBrowserAnnotationRequest::Theme { response_tx } => {
                    let result = Ok(cx.update(browser_annotation_theme));
                    if response_tx.send(result).is_err() {
                        log::warn!(
                            "browser annotation IPC client dropped before receiving theme response"
                        );
                    }
                }
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
            agent_ui::set_browser_annotation_focus_handler(cx, |focus, cx| {
                if let Some(server) = cx.try_global::<BrowserAnnotationIpcServerGlobal>() {
                    let focus_id = focus.id.clone();
                    let fallback_url = focus.fallback_url.clone();
                    let focus_requests = server.0.focus_requests.clone();
                    server.0.enqueue_focus_request(focus);
                    if let Some(fallback_url) = fallback_url {
                        cx.spawn(async move |cx| {
                            cx.background_executor().timer(FOCUS_FALLBACK_DELAY).await;
                            if is_focus_request_pending(&focus_requests, &focus_id) {
                                cx.update(|cx| cx.open_url(&fallback_url));
                            }
                        })
                        .detach();
                    }
                }
            });
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

async fn sync_browser_annotations(
    annotations: Vec<agent_ui::BrowserAnnotation>,
    app_state: Arc<AppState>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let multi_workspace = workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

    multi_workspace.update(cx, |multi_workspace, window, cx| {
        let panel = multi_workspace
            .panel::<AgentPanel>(cx)
            .context("No active detached agent panel is available")?;
        panel.update(cx, |panel, cx| {
            panel.sync_browser_annotations_to_active_thread(annotations, window, cx)
        })
    })?
}

fn browser_annotation_theme(cx: &mut App) -> serde_json::Value {
    let theme = cx.theme();
    let colors = theme.colors();
    let status = theme.status();

    json!({
        "appearance": if theme.appearance().is_light() { "light" } else { "dark" },
        "colors": {
            "background": css_color(colors.background),
            "panel_background": css_color(colors.panel_background),
            "elevated_surface_background": css_color(colors.elevated_surface_background),
            "editor_background": css_color(colors.editor_background),
            "element_background": css_color(colors.element_background),
            "element_hover": css_color(colors.element_hover),
            "element_active": css_color(colors.element_active),
            "element_selected": css_color(colors.element_selected),
            "border": css_color(colors.border),
            "border_variant": css_color(colors.border_variant),
            "border_focused": css_color(colors.border_focused),
            "text": css_color(colors.text),
            "text_muted": css_color(colors.text_muted),
            "text_disabled": css_color(colors.text_disabled),
            "text_accent": css_color(colors.text_accent),
            "icon": css_color(colors.icon),
            "icon_muted": css_color(colors.icon_muted),
            "success": css_color(status.success),
            "error": css_color(status.error),
            "warning": css_color(status.warning),
        },
    })
}

fn css_color(color: gpui::Hsla) -> String {
    let color: Rgba = color.into();
    format!(
        "rgba({}, {}, {}, {:.3})",
        css_color_component(color.r),
        css_color_component(color.g),
        css_color_component(color.b),
        color.a.clamp(0.0, 1.0)
    )
}

fn css_color_component(component: f32) -> u8 {
    (component.clamp(0.0, 1.0) * 255.0).round() as u8
}

struct BrowserAnnotationIpcServerGlobal(BrowserAnnotationIpcServer);

impl Global for BrowserAnnotationIpcServerGlobal {}

struct BrowserAnnotationIpcServer {
    metadata: BrowserAnnotationPairingMetadata,
    metadata_path: PathBuf,
    focus_requests: Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

#[derive(Clone, Debug)]
struct QueuedBrowserAnnotationFocus {
    focus: BrowserAnnotationFocus,
    queued_at: Instant,
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

    fn enqueue_focus_request(&self, focus: BrowserAnnotationFocus) {
        let mut focus_requests = lock_focus_requests(&self.focus_requests);
        prune_expired_focus_requests(&mut focus_requests);
        focus_requests.retain(|request| request.focus.id != focus.id);
        focus_requests.push_back(QueuedBrowserAnnotationFocus {
            focus,
            queued_at: Instant::now(),
        });
        while focus_requests.len() > 32 {
            focus_requests.pop_front();
        }
    }

    #[cfg(unix)]
    fn start_with_paths(
        socket_path: PathBuf,
        metadata_path: PathBuf,
        token: String,
        target: ForegroundBrowserAnnotationTarget,
    ) -> Result<Self> {
        let listener = bind_listener(&socket_path)?;

        let metadata = BrowserAnnotationPairingMetadata { socket_path, token };
        write_pairing_metadata(&metadata_path, &metadata)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let focus_requests = Arc::new(Mutex::new(VecDeque::new()));
        let thread_shutdown = shutdown.clone();
        let thread_metadata = metadata.clone();
        let thread_focus_requests = focus_requests.clone();
        let thread = thread::Builder::new()
            .name("browser-annotation-ipc".to_string())
            .spawn(move || {
                run_server(
                    listener,
                    thread_metadata,
                    target,
                    thread_focus_requests,
                    thread_shutdown,
                );
            })
            .context("spawning browser annotation IPC server thread")?;

        Ok(Self {
            metadata,
            metadata_path,
            focus_requests,
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

        #[cfg(unix)]
        remove_socket_directory(&self.metadata.socket_path);
    }
}

fn lock_focus_requests(
    focus_requests: &Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>,
) -> std::sync::MutexGuard<'_, VecDeque<QueuedBrowserAnnotationFocus>> {
    match focus_requests.lock() {
        Ok(focus_requests) => focus_requests,
        Err(error) => {
            log::warn!("browser annotation focus requests lock was poisoned; recovering");
            error.into_inner()
        }
    }
}

fn prune_expired_focus_requests(focus_requests: &mut VecDeque<QueuedBrowserAnnotationFocus>) {
    let now = Instant::now();
    focus_requests.retain(|request| now.duration_since(request.queued_at) <= FOCUS_REQUEST_TTL);
}

fn is_focus_request_pending(
    focus_requests: &Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>,
    focus_id: &str,
) -> bool {
    let mut focus_requests = lock_focus_requests(focus_requests);
    prune_expired_focus_requests(&mut focus_requests);
    focus_requests
        .iter()
        .any(|request| request.focus.id == focus_id)
}

#[cfg(unix)]
fn bind_listener(socket_path: &Path) -> Result<UnixListener> {
    if let Some(parent) = socket_path.parent()
        && is_generated_socket_directory(parent)
    {
        create_private_directory(parent)?;
    }

    if socket_path.exists() {
        fs::remove_file(socket_path).with_context(|| {
            format!(
                "removing stale browser annotation IPC socket {}",
                socket_path.display()
            )
        })?;
    }

    let listener = UnixListener::bind(socket_path).with_context(|| {
        format!(
            "binding browser annotation IPC socket {}",
            socket_path.display()
        )
    })?;

    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "setting browser annotation IPC socket permissions on {}",
            socket_path.display()
        )
    })?;

    Ok(listener)
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| {
        format!(
            "creating browser annotation IPC directory {}",
            path.display()
        )
    })?;

    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "reading browser annotation IPC directory metadata {}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        metadata.file_type().is_dir(),
        "browser annotation IPC path is not a directory: {}",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
        format!(
            "setting browser annotation IPC directory permissions on {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(unix)]
fn configure_connection_timeouts(stream: &UnixStream) -> Result<()> {
    stream
        .set_read_timeout(Some(IPC_CONNECTION_TIMEOUT))
        .context("setting browser annotation IPC read timeout")?;
    stream
        .set_write_timeout(Some(IPC_CONNECTION_TIMEOUT))
        .context("setting browser annotation IPC write timeout")?;
    Ok(())
}

#[cfg(unix)]
fn run_server(
    listener: UnixListener,
    metadata: BrowserAnnotationPairingMetadata,
    target: ForegroundBrowserAnnotationTarget,
    focus_requests: Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }

                if let Err(error) = configure_connection_timeouts(&stream) {
                    log::warn!("failed to configure browser annotation IPC connection: {error:#}");
                }

                let token = metadata.token.clone();
                let target = target.clone();
                let focus_requests = focus_requests.clone();
                if let Err(error) = thread::Builder::new()
                    .name("browser-annotation-ipc-connection".to_string())
                    .spawn(move || {
                        let mut stream = stream;
                        if let Err(error) = serve_connection(
                            &mut stream,
                            &token,
                            Some(&target),
                            Some(&focus_requests),
                        ) {
                            log::warn!("browser annotation IPC connection failed: {error:#}");
                        }
                    })
                {
                    log::warn!("failed to spawn browser annotation IPC connection thread: {error}");
                }
            }
            Err(error) => {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                log::warn!("browser annotation IPC accept failed: {error}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn short_socket_path() -> PathBuf {
    let suffix = Uuid::new_v4().simple().to_string();
    std::env::temp_dir()
        .join(format!("zed-ba-{}-{}", std::process::id(), &suffix[..8]))
        .join("ipc.sock")
}

#[cfg(unix)]
fn remove_socket_directory(socket_path: &Path) {
    let Some(parent) = socket_path.parent() else {
        return;
    };
    if !is_generated_socket_directory(parent) {
        return;
    }
    if let Err(error) = fs::remove_dir(parent) {
        log::debug!(
            "failed to remove browser annotation IPC directory {}: {error}",
            parent.display()
        );
    }
}

#[cfg(unix)]
fn is_generated_socket_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("zed-ba-"))
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

    if fs::symlink_metadata(metadata_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        anyhow::bail!(
            "refusing to write browser annotation pairing metadata through symlink {}",
            metadata_path.display()
        );
    }

    let mut file = metadata_file_options()
        .open(metadata_path)
        .with_context(|| {
            format!(
                "opening browser annotation pairing metadata {}",
                metadata_path.display()
            )
        })?;
    file.write_all(&serde_json::to_vec(metadata)?)
        .with_context(|| {
            format!(
                "writing browser annotation pairing metadata {}",
                metadata_path.display()
            )
        })?;
    file.flush().with_context(|| {
        format!(
            "flushing browser annotation pairing metadata {}",
            metadata_path.display()
        )
    })?;
    set_metadata_file_permissions(metadata_path)?;
    Ok(())
}

fn metadata_file_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
}

#[cfg(unix)]
fn set_metadata_file_permissions(metadata_path: &Path) -> Result<()> {
    fs::set_permissions(metadata_path, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "setting browser annotation pairing metadata permissions on {}",
            metadata_path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn set_metadata_file_permissions(_metadata_path: &Path) -> Result<()> {
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
    target: Option<&dyn BrowserAnnotationIpcTarget>,
    focus_requests: Option<&Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>>,
) -> Result<()> {
    while let Some(message) = read_framed_message(stream)? {
        let response = handle_message_with_target(&message, token, target, focus_requests);
        write_framed_message(stream, &serde_json::to_vec(&response)?)?;
    }

    Ok(())
}

#[cfg(test)]
fn handle_message(message: &[u8], token: &str) -> JsonRpcResponse {
    handle_message_with_target(message, token, None, None)
}

fn handle_message_with_target(
    message: &[u8],
    token: &str,
    target: Option<&dyn BrowserAnnotationIpcTarget>,
    focus_requests: Option<&Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>>,
) -> JsonRpcResponse {
    match parse_authenticated_request(message) {
        Ok((request, request_token)) => {
            handle_request(request, request_token, token, target, focus_requests)
        }
        Err(error) => JsonRpcResponse::error(None, PARSE_ERROR, format!("Parse error: {error}")),
    }
}

fn handle_request(
    request: JsonRpcRequest,
    request_token: Option<String>,
    token: &str,
    target: Option<&dyn BrowserAnnotationIpcTarget>,
    focus_requests: Option<&Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>>,
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
        "browserAnnotation.theme" => handle_theme_request(request, target),
        "browserAnnotation.pollFocus" => handle_poll_focus_request(request, focus_requests),
        "browserAnnotation.ackFocus" => handle_ack_focus_request(request, focus_requests),
        "browserAnnotation.insert" => handle_insert_annotation(request, target),
        "browserAnnotation.sync" => handle_sync_annotations(request, target),
        _ => JsonRpcResponse::error(
            request.id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

fn handle_theme_request(
    request: JsonRpcRequest,
    target: Option<&dyn BrowserAnnotationIpcTarget>,
) -> JsonRpcResponse {
    let id = request.id;
    let Some(target) = target else {
        return JsonRpcResponse::error(
            id,
            ZED_AGENT_PANEL_UNAVAILABLE,
            "No active Zed theme is available",
        );
    };

    match target.theme() {
        Ok(theme) => JsonRpcResponse::success(id, json!({ "theme": theme })),
        Err(error) => JsonRpcResponse::error(id, ZED_AGENT_PANEL_UNAVAILABLE, error.to_string()),
    }
}

fn handle_poll_focus_request(
    request: JsonRpcRequest,
    focus_requests: Option<&Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>>,
) -> JsonRpcResponse {
    let page_url = request.params.get("url").and_then(|url| url.as_str());
    let focus_request = focus_requests.and_then(|focus_requests| {
        let mut focus_requests = lock_focus_requests(focus_requests);
        prune_expired_focus_requests(&mut focus_requests);
        focus_requests
            .iter()
            .find(|request| {
                page_url
                    .map(|page_url| browser_annotation_urls_match(&request.focus.url, page_url))
                    .unwrap_or(true)
            })
            .map(|request| request.focus.clone())
    });

    JsonRpcResponse::success(
        request.id,
        json!({
            "request": focus_request.map(|focus| {
                json!({
                    "id": focus.id,
                    "url": focus.url,
                })
            })
        }),
    )
}

fn handle_ack_focus_request(
    request: JsonRpcRequest,
    focus_requests: Option<&Arc<Mutex<VecDeque<QueuedBrowserAnnotationFocus>>>>,
) -> JsonRpcResponse {
    let id = request.id;
    let Some(focus_id) = request.params.get("id").and_then(|id| id.as_str()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Focus request id is required");
    };

    if let Some(focus_requests) = focus_requests {
        let mut focus_requests = lock_focus_requests(focus_requests);
        focus_requests.retain(|request| request.focus.id != focus_id);
    }

    JsonRpcResponse::success(id, json!({ "ok": true }))
}

fn browser_annotation_urls_match(annotation_url: &str, page_url: &str) -> bool {
    annotation_url == page_url
        || strip_url_fragment(annotation_url)
            .zip(strip_url_fragment(page_url))
            .is_some_and(|(annotation_url, page_url)| annotation_url == page_url)
}

fn strip_url_fragment(value: &str) -> Option<String> {
    let mut url = url::Url::parse(value).ok()?;
    url.set_fragment(None);
    Some(url.to_string())
}

fn handle_sync_annotations(
    request: JsonRpcRequest,
    target: Option<&dyn BrowserAnnotationIpcTarget>,
) -> JsonRpcResponse {
    let id = request.id;
    let sync = match parse_browser_annotation_sync(request.params) {
        Ok(sync) => sync,
        Err(error) => return JsonRpcResponse::error(id, INVALID_PARAMS, error.to_string()),
    };

    for annotation in &sync.annotations {
        if annotation.url.trim().is_empty() {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "Annotation URL is required");
        }
    }

    let Some(target) = target else {
        return JsonRpcResponse::error(
            id,
            ZED_AGENT_PANEL_UNAVAILABLE,
            "No active detached agent panel is available",
        );
    };

    let annotations = sync
        .annotations
        .into_iter()
        .map(to_agent_ui_browser_annotation)
        .collect();
    if sync.submit {
        log::warn!(
            "browser annotation IPC submit flag ignored; annotations require Zed-side review"
        );
    }
    match target.sync_annotations(annotations) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(error) => JsonRpcResponse::error(id, ZED_AGENT_PANEL_UNAVAILABLE, error.to_string()),
    }
}

fn handle_insert_annotation(
    request: JsonRpcRequest,
    target: Option<&dyn BrowserAnnotationIpcTarget>,
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

    impl BrowserAnnotationIpcTarget for FakeBrowserAnnotationTarget {
        fn insert_annotation(&self, annotation: agent_ui::BrowserAnnotation) -> Result<()> {
            self.annotations
                .lock()
                .expect("annotations lock")
                .push(annotation);
            Ok(())
        }

        fn sync_annotations(&self, annotations: Vec<agent_ui::BrowserAnnotation>) -> Result<()> {
            *self.annotations.lock().expect("annotations lock") = annotations;
            Ok(())
        }

        fn theme(&self) -> Result<serde_json::Value> {
            Ok(json!({
                "appearance": "dark",
                "colors": {
                    "panel_background": "rgba(30, 30, 30, 1.000)",
                    "text": "rgba(220, 220, 220, 1.000)"
                }
            }))
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
    fn theme_request_uses_target() {
        let target = FakeBrowserAnnotationTarget::default();
        let response = handle_message_with_target(
            &authenticated_request("browserAnnotation.theme", json!({})),
            "secret",
            Some(&target),
            None,
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(
                Some(json!(1)),
                json!({
                    "theme": {
                        "appearance": "dark",
                        "colors": {
                            "panel_background": "rgba(30, 30, 30, 1.000)",
                            "text": "rgba(220, 220, 220, 1.000)"
                        }
                    }
                })
            )
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
            None,
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
    fn sync_annotations_uses_target_and_ignores_submit_flag() {
        let target = FakeBrowserAnnotationTarget::default();
        let response = handle_message_with_target(
            &authenticated_request(
                "browserAnnotation.sync",
                json!({
                    "annotations": [{
                        "id": "comment-1",
                        "url": "https://example.com",
                        "comment": "Review this"
                    }],
                    "submit": true
                }),
            ),
            "secret",
            Some(&target),
            None,
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
        let annotations = target.annotations.lock().expect("annotations lock");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].id.as_deref(), Some("comment-1"));
    }

    #[test]
    fn poll_focus_returns_matching_page_request_and_ack_removes_it() {
        let focus_requests = Arc::new(Mutex::new(VecDeque::from([QueuedBrowserAnnotationFocus {
            focus: BrowserAnnotationFocus {
                id: "comment-1".to_string(),
                url: "https://example.com/article#section".to_string(),
                fallback_url: Some(
                    "chrome-extension://extension/src/focus.html?tabId=1".to_string(),
                ),
            },
            queued_at: Instant::now(),
        }])));

        let response = handle_message_with_target(
            &authenticated_request(
                "browserAnnotation.pollFocus",
                json!({ "url": "https://example.com/article" }),
            ),
            "secret",
            None,
            Some(&focus_requests),
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(
                Some(json!(1)),
                json!({
                    "request": {
                        "id": "comment-1",
                        "url": "https://example.com/article#section",
                    }
                })
            )
        );

        let response = handle_message_with_target(
            &authenticated_request("browserAnnotation.ackFocus", json!({ "id": "comment-1" })),
            "secret",
            None,
            Some(&focus_requests),
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );
        assert!(focus_requests.lock().expect("focus requests").is_empty());
    }

    #[test]
    fn poll_focus_ignores_non_matching_page_request() {
        let focus_requests = Arc::new(Mutex::new(VecDeque::from([QueuedBrowserAnnotationFocus {
            focus: BrowserAnnotationFocus {
                id: "comment-1".to_string(),
                url: "https://example.com/article".to_string(),
                fallback_url: None,
            },
            queued_at: Instant::now(),
        }])));

        let response = handle_message_with_target(
            &authenticated_request(
                "browserAnnotation.pollFocus",
                json!({ "url": "https://example.test/other" }),
            ),
            "secret",
            None,
            Some(&focus_requests),
        );

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "request": null }))
        );
        assert_eq!(focus_requests.lock().expect("focus requests").len(), 1);
    }

    #[test]
    fn converts_protocol_annotation_to_agent_ui_annotation() {
        let annotation = to_agent_ui_browser_annotation(BrowserAnnotation {
            id: Some("comment-1".to_string()),
            url: "https://example.com".to_string(),
            title: Some("Example".to_string()),
            selected_text: Some("Selected text".to_string()),
            selector: Some("main".to_string()),
            comment: Some("Review this".to_string()),
            focus_url: Some("chrome-extension://extension/src/focus.html?tabId=1".to_string()),
        });

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

        serve_connection(&mut stream, "secret", None, None).expect("serve connection");

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
        assert!(socket_path.starts_with(std::env::temp_dir()));
        assert_eq!(
            socket_path.file_name().and_then(|name| name.to_str()),
            Some("ipc.sock")
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_socket_and_metadata_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let socket_path = short_socket_path();
        let socket_parent = socket_path.parent().expect("socket parent").to_path_buf();
        let socket_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let metadata_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-permissions-{}-{socket_id}.json",
            std::process::id(),
        ));
        let (foreground_tx, _foreground_rx) = mpsc::unbounded();
        let server = BrowserAnnotationIpcServer::start_with_paths(
            socket_path.clone(),
            metadata_path.clone(),
            "secret".to_string(),
            ForegroundBrowserAnnotationTarget { tx: foreground_tx },
        )
        .expect("start server");

        assert_eq!(
            fs::metadata(&socket_parent)
                .expect("socket directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&socket_path)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&metadata_path)
                .expect("pairing metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        drop(server);
        assert!(!metadata_path.exists());
        assert!(!socket_path.exists());
        assert!(!socket_parent.exists());
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

    #[cfg(unix)]
    #[test]
    fn stalled_unix_socket_client_does_not_block_other_clients() {
        let socket_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let socket_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-concurrent-{}-{socket_id}.sock",
            std::process::id(),
        ));
        let metadata_path = PathBuf::from("/tmp").join(format!(
            "zed-ba-concurrent-{}-{socket_id}.json",
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

        let stalled_stream =
            std::os::unix::net::UnixStream::connect(&socket_path).expect("connect stalled client");
        let mut responsive_stream = std::os::unix::net::UnixStream::connect(&socket_path)
            .expect("connect responsive client");
        responsive_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set client timeout");

        write_framed_message(
            &mut responsive_stream,
            &authenticated_request("browserAnnotation.ping", json!({})),
        )
        .expect("write request");
        let response = read_framed_message(&mut responsive_stream)
            .expect("read response")
            .expect("response");
        let response: JsonRpcResponse = serde_json::from_slice(&response).expect("parse response");

        assert_eq!(
            response,
            JsonRpcResponse::success(Some(json!(1)), json!({ "ok": true }))
        );

        drop(stalled_stream);
        drop(server);
    }
}
