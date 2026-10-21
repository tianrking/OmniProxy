//! Local-only workbench API and embedded, dependency-free browser interface.
use crate::{
    faults::{FaultSpec, run_with_fault},
    replay::{Capture, load_capture},
    scenario::{RunReport, Scenario, from_capture, junit, redact, sensitive},
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{
    process::{Child, Command},
    sync::{Mutex, Semaphore},
};

macro_rules! bail { ($($arg:tt)*) => { return Err(anyhow::anyhow!($($arg)*).into()) }; }

#[derive(Clone)]
pub struct Workbench {
    root: PathBuf,
    origin: String,
    port: u16,
    capture: Arc<Mutex<Option<Recording>>>,
    runs: Arc<Semaphore>,
}
struct Recording {
    child: Child,
    id: String,
    port: u16,
}
type ApiResult<T> = std::result::Result<T, ApiError>;
pub struct ApiError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
impl Workbench {
    pub async fn new(root: PathBuf, address: SocketAddr) -> Result<Self> {
        if !address.ip().is_loopback() {
            bail!("workbench must listen on loopback; remote authentication is not implemented");
        }
        for directory in ["scenarios", "reports", "captures", "proxy"] {
            tokio::fs::create_dir_all(root.join(directory)).await?;
        }
        let root = tokio::fs::canonicalize(root).await?;
        let workbench = Self {
            root,
            origin: format!("http://{address}"),
            port: address.port(),
            capture: Arc::new(Mutex::new(None)),
            runs: Arc::new(Semaphore::new(1)),
        };
        let demo = crate::lab::demo_scenario(&workbench.origin);
        // The demo origin is refreshed on startup; user scenarios are never overwritten.
        write_json(&workbench.file("scenarios", &demo.id, "json")?, &demo).await?;
        Ok(workbench)
    }
    fn file(&self, folder: &str, id: &str, extension: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 120
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!("invalid document ID");
        }
        Ok(self.root.join(folder).join(format!("{id}.{extension}")))
    }
}
pub fn router(state: Workbench) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/app.css"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route("/api/status", get(status))
        .route("/api/scenarios", get(list_scenarios).post(save_scenario))
        .route(
            "/api/scenarios/{id}",
            get(get_scenario).delete(delete_scenario),
        )
        .route("/api/scenarios/{id}/run", post(run))
        .route("/api/reports", get(list_reports))
        .route("/api/reports/{id}", get(get_report))
        .route("/api/reports/{id}/junit", get(get_junit))
        .route("/api/captures", get(list_captures).post(import_capture))
        .route("/api/captures/{id}", get(get_capture))
        .route("/api/captures/{id}/scenario", post(export_capture))
        .route("/api/recording/start", post(start_recording))
        .route("/api/recording/stop", post(stop_recording))
        .route("/api/recording/demo", post(record_demo))
        .route("/api/ca", get(ca_certificate))
        .with_state(state.clone())
        .merge(crate::lab::router())
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), local_guard))
}
async fn local_guard(State(state): State<Workbench>, request: Request, next: Next) -> Response {
    let allowed = [
        format!("127.0.0.1:{}", state.port),
        format!("localhost:{}", state.port),
        format!("[::1]:{}", state.port),
    ];
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !allowed.iter().any(|candidate| candidate == host) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or("");
        if !allowed
            .iter()
            .any(|candidate| origin == format!("http://{candidate}"))
        {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    if request
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response.headers_mut().insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}
async fn read_json<T: serde::de::DeserializeOwned>(path: &FsPath) -> Result<T> {
    if tokio::fs::metadata(path).await?.len() > 2 * 1024 * 1024 {
        bail!("document exceeds 2 MiB");
    }
    Ok(serde_json::from_slice(&tokio::fs::read(path).await?)?)
}
async fn write_json<T: serde::Serialize>(path: &FsPath, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if bytes.len() > 2 * 1024 * 1024 {
        bail!("document exceeds 2 MiB");
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&temporary, bytes).await?;
    if let Err(error) = tokio::fs::rename(&temporary, path).await {
        let _ = tokio::fs::remove_file(temporary).await;
        return Err(error.into());
    }
    Ok(())
}
async fn files(state: &Workbench, folder: &str, extension: &str) -> Result<Vec<PathBuf>> {
    let mut entries = tokio::fs::read_dir(state.root.join(folder)).await?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == extension)
            && entry.file_type().await?.is_file()
        {
            paths.push(path);
        }
        if paths.len() > 1000 {
            bail!("workspace contains more than 1000 documents; archive old files");
        }
    }
    Ok(paths)
}
async fn status(State(state): State<Workbench>) -> ApiResult<Json<Value>> {
    let mut recording = state.capture.lock().await;
    if let Some(active) = recording.as_mut()
        && active.child.try_wait()?.is_some()
    {
        *recording = None;
    }
    Ok(Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"workspace":state.root,"origin":state.origin,"recording":recording.as_ref().map(|active|json!({"id":active.id,"proxy":format!("http://127.0.0.1:{}",active.port)})),"busy":state.runs.available_permits()==0}),
    ))
}
async fn list_scenarios(State(state): State<Workbench>) -> ApiResult<Json<Value>> {
    let mut documents = Vec::new();
    let mut warnings = Vec::new();
    for path in files(&state, "scenarios", "json").await? {
        match read_json::<Scenario>(&path).await{Ok(s)=>documents.push(json!({"id":s.id,"name":s.name,"description":s.description,"base_url":s.base_url,"steps":s.steps.len()})),Err(_)=>warnings.push(path.file_name().unwrap().to_string_lossy().into_owned())}
    }
    Ok(Json(json!({"items":documents,"warnings":warnings})))
}
async fn save_scenario(
    State(state): State<Workbench>,
    Json(scenario): Json<Scenario>,
) -> ApiResult<Json<Value>> {
    scenario.validate()?;
    write_json(&state.file("scenarios", &scenario.id, "json")?, &scenario).await?;
    Ok(Json(json!({"id":scenario.id,"saved":true})))
}
async fn get_scenario(
    State(state): State<Workbench>,
    Path(id): Path<String>,
) -> ApiResult<Json<Scenario>> {
    Ok(Json(
        read_json(&state.file("scenarios", &id, "json")?).await?,
    ))
}
async fn delete_scenario(
    State(state): State<Workbench>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if id == "demo-checkout" {
        bail!("the built-in demo cannot be deleted; duplicate it to edit");
    }
    tokio::fs::remove_file(state.file("scenarios", &id, "json")?).await?;
    Ok(Json(json!({"deleted":true})))
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RunOptions {
    target: Option<String>,
    #[serde(default)]
    variables: BTreeMap<String, Value>,
    fault: Option<FaultSpec>,
}
async fn run(
    State(state): State<Workbench>,
    Path(id): Path<String>,
    Json(options): Json<RunOptions>,
) -> ApiResult<Json<RunReport>> {
    let _permit = state
        .runs
        .clone()
        .try_acquire_owned()
        .context("another run is active; wait for it to finish")?;
    let scenario: Scenario = read_json(&state.file("scenarios", &id, "json")?).await?;
    let report = run_with_fault(
        &scenario,
        options.target.as_deref(),
        options.variables,
        options.fault,
    )
    .await?;
    write_json(&state.file("reports", &report.id, "json")?, &report).await?;
    Ok(Json(report))
}
async fn list_reports(State(state): State<Workbench>) -> ApiResult<Json<Value>> {
    let mut reports = Vec::new();
    let mut warnings = Vec::new();
    for path in files(&state, "reports", "json").await? {
        match read_json::<RunReport>(&path).await{Ok(r)=>reports.push(json!({"id":r.id,"scenario_id":r.scenario_id,"scenario_name":r.scenario_name,"started_at_ms":r.started_at_ms,"status":r.status,"passed":r.passed,"failed":r.failed,"duration_ms":r.duration_ms,"fault":r.fault})),Err(_)=>warnings.push(path.file_name().unwrap().to_string_lossy().into_owned())}
    }
    reports.sort_by_key(|r| std::cmp::Reverse(r["started_at_ms"].as_u64().unwrap_or(0)));
    Ok(Json(json!({"items":reports,"warnings":warnings})))
}
async fn get_report(
    State(state): State<Workbench>,
    Path(id): Path<String>,
) -> ApiResult<Json<RunReport>> {
    Ok(Json(read_json(&state.file("reports", &id, "json")?).await?))
}
async fn get_junit(State(state): State<Workbench>, Path(id): Path<String>) -> ApiResult<Response> {
    let report: RunReport = read_json(&state.file("reports", &id, "json")?).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=regression.xml",
            ),
        ],
        junit(&report),
    )
        .into_response())
}
#[derive(Deserialize)]
struct ImportCapture {
    content: String,
}
async fn import_capture(
    State(state): State<Workbench>,
    Json(input): Json<ImportCapture>,
) -> ApiResult<Json<Value>> {
    if input.content.len() > 48 * 1024 * 1024 {
        bail!("capture import exceeds 48 MiB");
    }
    let capture = crate::replay::parse_capture(std::io::Cursor::new(&input.content))?;
    if capture.requests.is_empty() {
        bail!("no HTTP requests found in this JSONL capture");
    }
    let id = uuid::Uuid::new_v4().to_string();
    tokio::fs::write(state.file("captures", &id, "jsonl")?, input.content).await?;
    Ok(Json(
        json!({"id":id,"requests":capture.requests.len(),"issues":capture.issues}),
    ))
}
async fn list_captures(State(state): State<Workbench>) -> ApiResult<Json<Value>> {
    let mut captures = Vec::new();
    for path in files(&state, "captures", "jsonl").await? {
        let metadata = tokio::fs::metadata(&path).await?;
        captures.push(json!({"id":path.file_stem().unwrap().to_string_lossy(),"bytes":metadata.len(),"modified_ms":metadata.modified().ok().and_then(|m|m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_millis())}));
    }
    captures.sort_by_key(|c| std::cmp::Reverse(c["modified_ms"].as_u64().unwrap_or(0)));
    Ok(Json(json!({"items":captures})))
}
fn display_headers(headers: &[(String, String)]) -> Value {
    json!(
        headers
            .iter()
            .map(|(k, v)| (k, if sensitive(k) { "<redacted>" } else { v }))
            .collect::<Vec<_>>()
    )
}
fn display_body(body: &Option<Vec<u8>>) -> Value {
    match body {
        Some(bytes) => match serde_json::from_slice::<Value>(bytes) {
            Ok(value) => {
                if bytes.len() > 16384 {
                    json!({"notice":"JSON preview omitted (over 16 KiB). Full bytes remain in the local recording.","bytes":bytes.len()})
                } else {
                    redact(value)
                }
            }
            Err(_) => {
                json!({"preview":String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]),"bytes":bytes.len(),"notice":"Non-JSON body: automatic secret masking unavailable. Review before sharing."})
            }
        },
        None => Value::Null,
    }
}
fn capture_view(capture: Capture) -> Value {
    json!({"issues":capture.issues,"skipped_events":capture.skipped_events,"requests":capture.requests.iter().map(|request|{
        let uri=redact_uri(&request.uri);
        json!({"index":request.index,"timestamp_ms":request.timestamp_ms,"method":request.method,"uri":uri,"headers":display_headers(&request.headers),"body":display_body(&request.body),"issues":request.issues,"replayable":request.replayable(),"response":request.captured_response.as_ref().map(|response|json!({"status":response.status,"headers":display_headers(&response.headers),"body":display_body(&response.body),"issues":response.issues,"duration_ms":response.timestamp_ms.saturating_sub(request.timestamp_ms)}))})
    }).collect::<Vec<_>>()})
}
fn redact_uri(uri: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(uri) else {
        return "<invalid URL>".into();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    if url.query().is_some() {
        let pairs: Vec<_> = url
            .query_pairs()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    if sensitive(&k) {
                        "<redacted>".into()
                    } else {
                        v.to_string()
                    },
                )
            })
            .collect();
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    url.to_string()
}
async fn get_capture(
    State(state): State<Workbench>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let path = state.file("captures", &id, "jsonl")?;
    Ok(Json(
        tokio::task::spawn_blocking(move || load_capture(&path).map(capture_view)).await??,
    ))
}
#[derive(Deserialize)]
struct ExportCapture {
    indexes: Vec<usize>,
    name: String,
}
async fn export_capture(
    State(state): State<Workbench>,
    Path(id): Path<String>,
    Json(input): Json<ExportCapture>,
) -> ApiResult<Json<Scenario>> {
    let path = state.file("captures", &id, "jsonl")?;
    let capture = tokio::task::spawn_blocking(move || load_capture(&path)).await??;
    let scenario = from_capture(&capture, &input.indexes, input.name)?;
    write_json(&state.file("scenarios", &scenario.id, "json")?, &scenario).await?;
    Ok(Json(scenario))
}
#[derive(Deserialize)]
struct StartRecording {
    #[serde(default = "proxy_port")]
    port: u16,
}
fn proxy_port() -> u16 {
    9090
}
async fn start_recording(
    State(state): State<Workbench>,
    Json(input): Json<StartRecording>,
) -> ApiResult<Json<Value>> {
    let mut recording = state.capture.lock().await;
    if let Some(active) = recording.as_mut()
        && active.child.try_wait()?.is_none()
    {
        bail!("a recording is already active");
    }
    *recording = None;
    if input.port < 1024 || input.port == state.port {
        bail!("choose an available proxy port from 1024..65535, different from the UI port");
    }
    let available = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, input.port))
        .await
        .context("proxy port is already in use")?;
    let api = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api_address = api.local_addr()?;
    let executable = core_executable(&std::env::current_exe()?)?;
    let id = uuid::Uuid::new_v4().to_string();
    let proxy_dir = state.root.join("proxy");
    let log = std::fs::File::create(proxy_dir.join("process.log"))?;
    drop(available);
    drop(api);
    let mut child = Command::new(executable)
        .args([
            "--listen",
            &format!("127.0.0.1:{}", input.port),
            "--api-listen",
            &api_address.to_string(),
            "--capture-body-max-bytes",
            "524288",
            "--flow-log-rotate-bytes",
            "50331648",
            "--flow-log-max-files",
            "3",
        ])
        .arg("--flow-log")
        .arg(state.file("captures", &id, "jsonl")?)
        .arg("--ca-cert")
        .arg(proxy_dir.join("ca.crt"))
        .arg("--ca-key")
        .arg(proxy_dir.join("ca.key"))
        .arg("--rule-file")
        .arg(proxy_dir.join("rules.txt"))
        .arg("--plugin-dir")
        .arg(proxy_dir.join("plugins"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .kill_on_drop(true)
        .spawn()?;
    for _ in 0..100 {
        if child.try_wait()?.is_some() {
            bail!("proxy exited during startup; inspect workspace/proxy/process.log");
        }
        if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, input.port))
            .await
            .is_ok()
        {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if child.try_wait()?.is_some() {
                bail!("proxy failed to bind; inspect workspace/proxy/process.log");
            }
            *recording = Some(Recording {
                child,
                id: id.clone(),
                port: input.port,
            });
            return Ok(Json(
                json!({"id":id,"proxy":format!("http://127.0.0.1:{}",input.port)}),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    child.kill().await?;
    bail!("proxy did not become ready within 10 seconds")
}

fn core_executable(current: &FsPath) -> Result<PathBuf> {
    let names = if cfg!(windows) {
        ["omni_proxy.exe", "omni-proxy.exe"]
    } else {
        ["omni_proxy", "omni-proxy"]
    };
    names.into_iter().map(|name|current.with_file_name(name)).find(|path|path.is_file()).context("proxy core missing next to workbench; install the full package or run cargo build --locked --bins")
}

#[cfg(test)]
mod packaging_tests {
    use super::*;
    #[test]
    fn recorder_finds_both_cargo_and_debian_core_names() {
        let root = std::env::temp_dir().join(format!("omni-core-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let current = root.join(if cfg!(windows) {
            "omni-workbench.exe"
        } else {
            "omni-workbench"
        });
        assert!(core_executable(&current).is_err());
        let debian = root.join(if cfg!(windows) {
            "omni-proxy.exe"
        } else {
            "omni-proxy"
        });
        std::fs::write(&debian, b"fixture").unwrap();
        assert_eq!(core_executable(&current).unwrap(), debian);
        let cargo = root.join(if cfg!(windows) {
            "omni_proxy.exe"
        } else {
            "omni_proxy"
        });
        std::fs::write(&cargo, b"fixture").unwrap();
        assert_eq!(core_executable(&current).unwrap(), cargo);
        std::fs::remove_dir_all(root).unwrap();
    }
}
async fn stop_recording(State(state): State<Workbench>) -> ApiResult<Json<Value>> {
    let mut recording = state.capture.lock().await;
    if let Some(mut active) = recording.take() {
        if active.child.try_wait()?.is_none() {
            active.child.kill().await?;
        }
        Ok(Json(json!({"id":active.id,"stopped":true})))
    } else {
        bail!("no managed recording is active")
    }
}
async fn ca_certificate(State(state): State<Workbench>) -> ApiResult<Response> {
    let bytes = tokio::fs::read(state.root.join("proxy/ca.crt"))
        .await
        .context("start a recording first to create the CA certificate")?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-x509-ca-cert"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=omni-proxy-ca.crt",
            ),
        ],
        bytes,
    )
        .into_response())
}
async fn record_demo(State(state): State<Workbench>) -> ApiResult<Json<Value>> {
    let port = {
        let mut recording = state.capture.lock().await;
        let active = recording.as_mut().context("start recording first")?;
        if active.child.try_wait()?.is_some() {
            bail!("managed proxy is no longer running");
        }
        active.port
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?)
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let url = format!("{}/lab/api", state.origin);
    let login: Value = serde_json::from_slice(
        &client
            .post(format!("{url}/login"))
            .header("content-type", "application/json")
            .body(r#"{"username":"demo"}"#)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?,
    )?;
    let token = login["token"]
        .as_str()
        .context("lab login did not return a token")?;
    let order: Value = serde_json::from_slice(
        &client
            .post(format!("{url}/orders"))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(r#"{"quantity":2}"#)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?,
    )?;
    let id = order["id"]
        .as_str()
        .context("lab did not return an order ID")?;
    client
        .get(format!("{url}/orders/{id}"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(Json(
        json!({"recorded":3,"message":"Three real HTTP requests were sent through the managed proxy. Refresh the capture to inspect."}),
    ))
}
