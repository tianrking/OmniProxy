//! Portable, bounded HTTP scenarios and semantic regression reports.
use crate::replay::Capture;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use reqwest::{
    Client, Method, Url,
    header::{HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub base_url: String,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    pub steps: Vec<Step>,
}
fn schema_version() -> u32 {
    SCHEMA_VERSION
}
fn default_timeout() -> u64 {
    5000
}
fn default_method() -> String {
    "GET".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub name: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
    #[serde(default)]
    pub extract: BTreeMap<String, String>,
    pub expect: Expectations,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    #[serde(default)]
    pub ignore_json: Vec<String>,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    /// RFC 6901 JSON Pointer, e.g. /order/total.
    pub pointer: String,
    pub op: AssertionOp,
    #[serde(default)]
    pub value: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionOp {
    Equals,
    Exists,
    Contains,
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub field: String,
    pub passed: bool,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepReport {
    pub id: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub status: String,
    pub http_status: Option<u16>,
    pub duration_ms: u64,
    pub checks: Vec<Check>,
    pub response: Option<Value>,
    pub extracted: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_matches: Option<usize>,
    #[serde(default)]
    pub scenario_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<crate::faults::FaultSpec>,
    pub schema_version: u32,
    pub id: String,
    pub scenario_id: String,
    pub scenario_name: String,
    pub started_at_ms: u64,
    pub target: String,
    pub status: String,
    pub duration_ms: u64,
    pub passed: usize,
    pub failed: usize,
    pub steps: Vec<StepReport>,
}

impl Scenario {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!("unsupported scenario schema version");
        }
        if self.id.is_empty()
            || self.id.len() > 120
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!("scenario ID must contain only letters, digits, - and _");
        }
        if self.name.trim().is_empty() || self.name.len() > 200 {
            bail!("scenario name is required (max 200 characters)");
        }
        validate_origin(&self.base_url)?;
        if !(50..=30_000).contains(&self.timeout_ms) {
            bail!("timeout_ms must be 50..30000");
        }
        if self.steps.is_empty() || self.steps.len() > 100 {
            bail!("scenario needs 1..100 steps");
        }
        let mut ids = BTreeSet::new();
        for step in &self.steps {
            if !ids.insert(&step.id) || step.id.is_empty() {
                bail!("step IDs must be unique and non-empty");
            }
            Method::from_bytes(step.method.as_bytes()).context("invalid HTTP method")?;
            if !step.path.starts_with('/') || step.path.starts_with("//") {
                bail!("step paths must be relative to the target origin and start with /");
            }
            if step.body.is_some() && step.body_base64.is_some() {
                bail!("choose JSON body or base64 body, not both");
            }
            if let Some(body) = &step.body_base64 {
                if body.len() > MAX_BODY * 2 {
                    bail!("request body too large");
                }
                base64::engine::general_purpose::STANDARD
                    .decode(body)
                    .context("invalid base64 request body")?;
            }
            if step.expect.status.is_none()
                && step.expect.json.is_none()
                && step.expect.assertions.is_empty()
                && step.expect.headers.is_empty()
                && step.expect.max_latency_ms.is_none()
                && step.expect.body_sha256.is_none()
            {
                bail!("every step needs at least one assertion");
            }
            if let Some(status) = step.expect.status
                && !(100..=599).contains(&status)
            {
                bail!("invalid expected status");
            }
            for pointer in step
                .extract
                .values()
                .chain(step.expect.ignore_json.iter())
                .chain(step.expect.assertions.iter().map(|a| &a.pointer))
            {
                if !pointer.is_empty() && !pointer.starts_with('/') {
                    bail!("JSON selectors use JSON Pointer, e.g. /token");
                }
            }
            if step.expect.ignore_json.iter().any(String::is_empty) {
                bail!("cannot ignore the entire JSON document");
            }
            for (name, value) in &step.headers {
                HeaderName::from_bytes(name.as_bytes()).context("invalid header name")?;
                HeaderValue::from_str(value).context("invalid header value")?;
                if is_transport_header(name) {
                    bail!("transport headers are managed by the runner");
                }
            }
        }
        Ok(())
    }
}

pub fn validate_origin(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("target must be an absolute HTTP(S) origin")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        bail!("target must be an HTTP(S) origin without credentials, path, query or fragment");
    }
    Ok(url)
}

pub async fn run_scenario(
    scenario: &Scenario,
    target: Option<&str>,
    overrides: BTreeMap<String, Value>,
) -> Result<RunReport> {
    scenario.validate()?;
    let origin = validate_origin(target.unwrap_or(&scenario.base_url))?;
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .timeout(Duration::from_millis(scenario.timeout_ms))
        .build()?;
    let started = Instant::now();
    let mut report = RunReport {
        fault_matches: None,
        scenario_sha256: {
            use sha2::{Digest, Sha256};
            Sha256::digest(serde_json::to_vec(scenario)?)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        },
        fault: None,
        schema_version: SCHEMA_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        scenario_id: scenario.id.clone(),
        scenario_name: scenario.name.clone(),
        started_at_ms: crate::api::now_ms(),
        target: origin.origin().ascii_serialization(),
        status: "passed".into(),
        duration_ms: 0,
        passed: 0,
        failed: 0,
        steps: Vec::new(),
    };
    let mut variables = scenario.variables.clone();
    variables.extend(overrides);
    let mut blocked = false;
    for step in &scenario.steps {
        if blocked {
            report.steps.push(StepReport {
                id: step.id.clone(),
                name: step.name.clone(),
                method: step.method.clone(),
                path: step.path.clone(),
                status: "skipped".into(),
                http_status: None,
                duration_ms: 0,
                checks: Vec::new(),
                response: None,
                extracted: Vec::new(),
            });
            continue;
        }
        let step_started = Instant::now();
        let result = execute_step(&client, &origin, step, &mut variables).await;
        let step_report = match result {
            Ok(result) => result,
            Err(error) => StepReport {
                id: step.id.clone(),
                name: step.name.clone(),
                method: step.method.clone(),
                path: step.path.clone(),
                status: "error".into(),
                http_status: None,
                duration_ms: step_started.elapsed().as_millis() as u64,
                checks: vec![Check {
                    field: "execution".into(),
                    passed: false,
                    message: error.to_string(),
                }],
                response: None,
                extracted: Vec::new(),
            },
        };
        if step_report.status == "passed" {
            report.passed += 1;
        } else {
            report.failed += 1;
            report.status = "failed".into();
            blocked = true;
        }
        report.steps.push(step_report);
    }
    report.duration_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

async fn execute_step(
    client: &Client,
    origin: &Url,
    step: &Step,
    variables: &mut BTreeMap<String, Value>,
) -> Result<StepReport> {
    let started = Instant::now();
    let path = render_path(&step.path, variables)?;
    let url = origin.join(&path).context("invalid step path")?;
    if url.origin() != origin.origin() {
        bail!("step attempted to leave target origin");
    }
    let mut request = client.request(Method::from_bytes(step.method.as_bytes())?, url);
    for (key, value) in &step.headers {
        request = request.header(key.as_str(), render_text(value, variables)?);
    }
    if let Some(body) = &step.body {
        let body = render_json(body, variables)?;
        let bytes = serde_json::to_vec(&body)?;
        if bytes.len() > MAX_BODY {
            bail!("request body exceeds 2 MiB");
        }
        request = request
            .header("content-type", "application/json")
            .body(bytes);
    } else if let Some(body) = &step.body_base64 {
        request = request.body(base64::engine::general_purpose::STANDARD.decode(body)?);
    }
    let mut response = request.send().await.map_err(|e| {
        anyhow::anyhow!(if e.is_timeout() {
            "request timed out"
        } else {
            "request failed; check target, TLS trust and connectivity"
        })
    })?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    if response
        .content_length()
        .is_some_and(|len| len > MAX_BODY as u64)
    {
        bail!("response exceeds 2 MiB limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("response body interrupted or timed out"))?
    {
        if bytes.len() + chunk.len() > MAX_BODY {
            bail!("response exceeds 2 MiB limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    let duration_ms = started.elapsed().as_millis() as u64;
    let actual: Option<Value> = serde_json::from_slice(&bytes).ok();
    let mut checks = Vec::new();
    if let Some(expected) = step.expect.status {
        checks.push(check(
            "status",
            expected == status,
            format!("expected {expected}, received {status}"),
        ));
    }
    if let Some(max) = step.expect.max_latency_ms {
        checks.push(check(
            "latency",
            duration_ms <= max,
            format!("{duration_ms} ms / {max} ms budget"),
        ));
    }
    for (name, value) in &step.expect.headers {
        let expected = render_text(value, variables)?;
        checks.push(check(
            &format!("header:{name}"),
            headers.get(name).and_then(|v| v.to_str().ok()) == Some(expected.as_str()),
            "header value comparison".into(),
        ));
    }
    if let Some(baseline) = &step.expect.json {
        if let Some(actual) = &actual {
            let mut expected = render_json(baseline, variables)?;
            let mut actual = actual.clone();
            for pointer in &step.expect.ignore_json {
                mask_pointer(&mut expected, pointer);
                mask_pointer(&mut actual, pointer);
            }
            let mut differences = Vec::new();
            json_differences("", &expected, &actual, &mut differences);
            checks.push(check(
                "json",
                differences.is_empty(),
                if differences.is_empty() {
                    "JSON matches baseline".into()
                } else {
                    format!("changed: {}", differences.join(", "))
                },
            ));
        } else {
            checks.push(check("json", false, "expected a JSON response".into()));
        }
    }
    for assertion in &step.expect.assertions {
        let expected = render_json(&assertion.value, variables)?;
        let found = actual
            .as_ref()
            .and_then(|value| value.pointer(&assertion.pointer));
        let passed = match assertion.op {
            AssertionOp::Exists => found.is_some(),
            AssertionOp::Equals => found == Some(&expected),
            AssertionOp::Contains => match (found, &expected) {
                (Some(Value::String(v)), Value::String(e)) => v.contains(e),
                (Some(Value::Array(v)), e) => v.contains(e),
                _ => false,
            },
            AssertionOp::Type => {
                found.is_some_and(|v| json_type(v) == expected.as_str().unwrap_or(""))
            }
        };
        checks.push(check(
            &format!("json:{}", assertion.pointer),
            passed,
            format!(
                "{:?} assertion {}",
                assertion.op,
                if passed { "passed" } else { "failed" }
            ),
        ));
    }
    if let Some(expected) = &step.expect.body_sha256 {
        use sha2::{Digest, Sha256};
        let actual = Sha256::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        checks.push(check(
            "body_sha256",
            actual == *expected,
            "body digest comparison".into(),
        ));
    }
    let mut extracted = Vec::new();
    let mut pending = BTreeMap::new();
    for (name, pointer) in &step.extract {
        if let Some(value) = actual.as_ref().and_then(|v| v.pointer(pointer)) {
            pending.insert(name.clone(), value.clone());
            extracted.push(name.clone());
        } else {
            checks.push(check(
                &format!("extract:{name}"),
                false,
                format!("missing JSON value at {pointer}"),
            ));
        }
    }
    let passed = checks.iter().all(|c| c.passed);
    if passed {
        variables.extend(pending);
    } else {
        extracted.clear();
    }
    Ok(StepReport {
        id: step.id.clone(),
        name: step.name.clone(),
        method: step.method.clone(),
        path: step.path.clone(),
        status: if passed { "passed" } else { "failed" }.into(),
        http_status: Some(status),
        duration_ms,
        checks,
        response: actual.map(redact).map(|value| {
            if serde_json::to_vec(&value).map_or(true,|bytes|bytes.len()>16384) {
                json!({"notice":"Response preview omitted (over 16 KiB). All assertions used the complete bounded response."})
            } else {value}
        }),
        extracted,
    })
}

fn check(field: &str, passed: bool, message: String) -> Check {
    Check {
        field: field.into(),
        passed,
        message,
    }
}

pub fn render_text(template: &str, variables: &BTreeMap<String, Value>) -> Result<String> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let remaining = &rest[start + 2..];
        let end = remaining
            .find("}}")
            .context("unterminated variable placeholder")?;
        let name = remaining[..end].trim();
        let value = variables
            .get(name)
            .filter(|v| !v.is_null())
            .with_context(|| format!("provide variable '{name}' before this step"))?;
        match value {
            Value::String(value) => output.push_str(value),
            Value::Bool(_) | Value::Number(_) => output.push_str(&value.to_string()),
            _ => bail!("variable '{name}' must be a scalar in text"),
        }
        rest = &remaining[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

fn render_path(template: &str, variables: &BTreeMap<String, Value>) -> Result<String> {
    let Some((path, query)) = template.split_once('?') else {
        return render_text(template, variables);
    };
    let path = render_text(path, variables)?;
    let input = Url::parse(&format!("http://local/?{query}"))?;
    let pairs = input
        .query_pairs()
        .map(|(key, value)| {
            Ok((
                render_text(&key, variables)?,
                render_text(&value, variables)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let url = Url::parse_with_params("http://local/", pairs)?;
    Ok(format!("{path}?{}", url.query().unwrap_or("")))
}

fn render_json(value: &Value, variables: &BTreeMap<String, Value>) -> Result<Value> {
    Ok(match value {
        Value::String(text) => {
            if text.starts_with("{{") && text.ends_with("}}") && text.matches("{{").count() == 1 {
                let key = text[2..text.len() - 2].trim();
                variables
                    .get(key)
                    .filter(|v| !v.is_null())
                    .with_context(|| format!("provide variable '{key}' before this step"))?
                    .clone()
            } else {
                Value::String(render_text(text, variables)?)
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| render_json(v, variables))
                .collect::<Result<_>>()?,
        ),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), render_json(v, variables)?)))
                .collect::<Result<_>>()?,
        ),
        other => other.clone(),
    })
}

fn mask_pointer(value: &mut Value, pointer: &str) {
    if let Some(target) = value.pointer_mut(pointer) {
        *target = Value::String("<ignored>".into());
    }
}
fn pointer_key(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
pub fn json_differences(path: &str, expected: &Value, actual: &Value, output: &mut Vec<String>) {
    if expected == actual || output.len() >= 30 {
        return;
    }
    match (expected, actual) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let path = format!("{path}/{}", pointer_key(key));
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) => json_differences(&path, a, b, output),
                    _ => output.push(path),
                }
                if output.len() >= 30 {
                    break;
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                json_differences(&format!("{path}/{i}"), a, b, output);
            }
        }
        _ => output.push(if path.is_empty() {
            "/".into()
        } else {
            path.into()
        }),
    }
}
fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
pub fn sensitive(name: &str) -> bool {
    let key = name.to_ascii_lowercase().replace(['-', '_'], "");
    [
        "authorization",
        "cookie",
        "setcookie",
        "password",
        "passwd",
        "secret",
        "token",
        "accesstoken",
        "refreshtoken",
        "apikey",
        "clientsecret",
    ]
    .iter()
    .any(|n| key == *n || key.ends_with(n))
}
pub fn redact(value: Value) -> Value {
    match value {
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(k, v)| {
                    let v = if sensitive(&k) {
                        json!("<redacted>")
                    } else {
                        redact(v)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(v) => Value::Array(v.into_iter().map(redact).collect()),
        v => v,
    }
}
pub fn is_transport_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-authorization"
            | "proxy-connection"
            | "upgrade"
            | "te"
            | "trailer"
            | "accept-encoding"
            | "x-omni-request-id"
    )
}

pub fn from_capture(capture: &Capture, indexes: &[usize], name: String) -> Result<Scenario> {
    if indexes.is_empty() || indexes.len() > 100 {
        bail!("select 1..100 requests for a focused regression scenario");
    }
    if !capture.issues.is_empty() {
        bail!(
            "recording has integrity warnings; make a complete recording before exporting a regression scenario"
        );
    }
    let selected: Vec<_> = capture
        .requests
        .iter()
        .filter(|r| indexes.contains(&r.index))
        .collect();
    if selected.is_empty() || selected.len() != indexes.iter().collect::<BTreeSet<_>>().len() {
        bail!("select existing captured requests");
    }
    let first =
        Url::parse(&selected[0].uri).context("captured request needs an absolute HTTP URL")?;
    let origin = first.origin();
    let mut variables = BTreeMap::new();
    let mut steps = Vec::new();
    for (i, request) in selected.iter().enumerate() {
        if !request.replayable() {
            bail!(
                "request {} is incomplete or cannot be replayed",
                request.index
            );
        }
        let url = Url::parse(&request.uri)?;
        if url.origin() != origin {
            bail!("select requests from a single origin");
        }
        let response = request
            .captured_response
            .as_ref()
            .context("request has no captured response")?;
        if !response.issues.is_empty() {
            bail!("response is incomplete; record a full response first");
        }
        let mut headers = BTreeMap::new();
        for (name, value) in &request.headers {
            if is_transport_header(name) {
                continue;
            }
            if sensitive(name) {
                let key = format!("input_{}", name.to_ascii_lowercase().replace('-', "_"));
                variables.insert(key.clone(), Value::Null);
                headers.insert(name.clone(), format!("{{{{{key}}}}}"));
            } else {
                headers.insert(name.clone(), value.clone());
            }
        }
        let mut body = request
            .body
            .as_ref()
            .and_then(|b| serde_json::from_slice::<Value>(b).ok());
        if let Some(body) = &mut body {
            parameterize(body, &mut variables, &format!("step{}", i + 1));
        }
        let body_base64 = if body.is_none() {
            request
                .body
                .as_ref()
                .map(|b| base64::engine::general_purpose::STANDARD.encode(b))
        } else {
            None
        };
        let mut expect = Expectations {
            status: Some(response.status),
            json: response
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice::<Value>(b).ok()),
            ..Default::default()
        };
        if let Some(json) = &mut expect.json {
            remove_sensitive_baseline(json, "", &mut expect.ignore_json);
        }
        if expect.json.is_none() {
            expect.body_sha256 = response.body_hash.clone();
        }
        let mut path = url.path().to_string();
        if url.query().is_some() {
            path.push('?');
            let pairs: Vec<_> = url
                .query_pairs()
                .enumerate()
                .map(|(n, (key, value))| {
                    let encoded_key: String =
                        reqwest::Url::parse_with_params("http://local/", &[(key.as_ref(), "")])
                            .unwrap()
                            .query()
                            .unwrap()
                            .trim_end_matches('=')
                            .into();
                    if sensitive(&key) {
                        let input = format!("step{}_query_{}", i + 1, n);
                        variables.insert(input.clone(), Value::Null);
                        format!("{encoded_key}={{{{{input}}}}}")
                    } else {
                        reqwest::Url::parse_with_params(
                            "http://local/",
                            &[(key.as_ref(), value.as_ref())],
                        )
                        .unwrap()
                        .query()
                        .unwrap()
                        .to_string()
                    }
                })
                .collect();
            path.push_str(&pairs.join("&"));
        }
        steps.push(Step {
            id: format!("step-{}", i + 1),
            name: format!("{} {}", request.method, url.path()),
            method: request.method.clone(),
            path,
            headers,
            body,
            body_base64,
            extract: BTreeMap::new(),
            expect,
        });
    }
    infer_dependencies(&selected, &mut steps, &mut variables);
    let scenario = Scenario {schema_version:1,id:uuid::Uuid::new_v4().to_string(),name,description:"Created from a complete capture. Exact token/ID matches are linked automatically; review inferred dependencies, dynamic fields and sensitive inputs before running or sharing.".into(),base_url:origin.ascii_serialization(),variables,timeout_ms:5000,steps};
    scenario.validate()?;
    Ok(scenario)
}

// Deliberately conservative: only exact, nontrivial string values and path segments.
// This is deterministic data-flow inference, not a claim to understand an API.
fn infer_dependencies(
    requests: &[&crate::replay::ReplayCandidate],
    steps: &mut [Step],
    variables: &mut BTreeMap<String, Value>,
) {
    fn strings(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
        if out.len() >= 64 {
            return;
        }
        match value {
            Value::String(text) if text.len() >= 8 && text.len() <= 2048 => {
                out.push((path.into(), text.clone()))
            }
            Value::Object(items) => {
                for (key, value) in items {
                    strings(value, &format!("{path}/{}", pointer_key(key)), out);
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    strings(value, &format!("{path}/{index}"), out);
                }
            }
            _ => {}
        }
    }
    fn replace_json(value: &mut Value, needle: &str, replacement: &str) -> bool {
        match value {
            Value::String(text) if text == needle => {
                *text = replacement.into();
                true
            }
            Value::Object(items) => items.values_mut().fold(false, |changed, v| {
                replace_json(v, needle, replacement) || changed
            }),
            Value::Array(items) => items.iter_mut().fold(false, |changed, v| {
                replace_json(v, needle, replacement) || changed
            }),
            _ => false,
        }
    }
    let mut cookies = BTreeSet::new();
    for (index, request) in requests.iter().enumerate() {
        if let Some((name, value)) = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        {
            let inherited = value.split(';').all(|pair| {
                pair.trim()
                    .split_once('=')
                    .is_some_and(|(name, _)| cookies.contains(name))
            });
            if inherited {
                steps[index].headers.remove(name);
            }
        }
        if let Some(response) = &request.captured_response {
            for (_, value) in response
                .headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            {
                if let Some((name, _)) = value.split(';').next().unwrap_or("").split_once('=') {
                    cookies.insert(name.to_string());
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    for source in 0..steps.len() {
        let Some(response) = requests[source]
            .captured_response
            .as_ref()
            .and_then(|r| r.body.as_ref())
            .and_then(|body| serde_json::from_slice::<Value>(body).ok())
        else {
            continue;
        };
        let mut candidates = Vec::new();
        strings(&response, "", &mut candidates);
        for (number, (pointer, value)) in candidates.into_iter().enumerate() {
            if pointer.is_empty() || seen.contains(&value) {
                continue;
            }
            let name = format!("capture_{}_{}", source + 1, number + 1);
            let replacement = format!("{{{{{name}}}}}");
            let mut linked = false;
            for next in source + 1..steps.len() {
                for (key, raw) in &requests[next].headers {
                    if key.eq_ignore_ascii_case("cookie") {
                        continue;
                    }
                    if *raw == value {
                        steps[next].headers.insert(key.clone(), replacement.clone());
                        linked = true;
                    } else if raw.strip_prefix("Bearer ") == Some(value.as_str()) {
                        steps[next]
                            .headers
                            .insert(key.clone(), format!("Bearer {replacement}"));
                        linked = true;
                    }
                }
                let (path, query) = steps[next]
                    .path
                    .split_once('?')
                    .map(|(p, q)| (p.to_string(), Some(q.to_string())))
                    .unwrap_or_else(|| (steps[next].path.clone(), None));
                let parts: Vec<_> = path
                    .split('/')
                    .map(|part| {
                        if part == value {
                            linked = true;
                            replacement.clone()
                        } else {
                            part.to_string()
                        }
                    })
                    .collect();
                let path = parts.join("/");
                let query = query.map(|query| {
                    let input = Url::parse(&format!("http://local/?{query}")).unwrap();
                    input
                        .query_pairs()
                        .map(|(key, current)| {
                            let raw_url = Url::parse(&requests[next].uri).unwrap();
                            let raw_matches =
                                raw_url.query_pairs().any(|(k, v)| k == key && v == value);
                            let encoded = Url::parse_with_params(
                                "http://local/",
                                &[(key.as_ref(), current.as_ref())],
                            )
                            .unwrap();
                            if raw_matches {
                                linked = true;
                                let prefix =
                                    Url::parse_with_params("http://local/", &[(key.as_ref(), "")])
                                        .unwrap();
                                format!("{}{}", prefix.query().unwrap(), replacement)
                            } else {
                                encoded
                                    .query()
                                    .unwrap()
                                    .replace("%7B%7B", "{{")
                                    .replace("%7D%7D", "}}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("&")
                });
                steps[next].path = format!(
                    "{path}{}",
                    query.map(|q| format!("?{q}")).unwrap_or_default()
                );
                if let Some(body) = &mut steps[next].body {
                    linked = replace_json(body, &value, &replacement) || linked;
                    if let Some(raw) = requests[next]
                        .body
                        .as_ref()
                        .and_then(|b| serde_json::from_slice::<Value>(b).ok())
                    {
                        let mut leaves = Vec::new();
                        strings(&raw, "", &mut leaves);
                        for (path, text) in leaves {
                            if text == value
                                && let Some(target) = body.pointer_mut(&path)
                            {
                                *target = json!(replacement);
                                linked = true;
                            }
                        }
                    }
                }
            }
            if linked {
                for step in steps.iter_mut().skip(source + 1) {
                    if let Some(baseline) = &mut step.expect.json {
                        replace_json(baseline, &value, &replacement);
                    }
                }
                steps[source].extract.insert(name, pointer.clone());
                if let Some(baseline) = &mut steps[source].expect.json {
                    mask_pointer(baseline, &pointer);
                }
                if !steps[source].expect.ignore_json.contains(&pointer) {
                    steps[source].expect.ignore_json.push(pointer);
                }
                seen.insert(value);
            }
        }
    }
    for step in steps.iter_mut() {
        step.name = format!(
            "{} {}",
            step.method,
            step.path.split('?').next().unwrap_or("/")
        );
    }
    let serialized = serde_json::to_string(steps).unwrap_or_default();
    variables.retain(|key, _| serialized.contains(&format!("{{{{{key}}}}}")));
}
fn parameterize(value: &mut Value, variables: &mut BTreeMap<String, Value>, prefix: &str) {
    match value {
        Value::Object(items) => {
            for (k, v) in items {
                if sensitive(k) {
                    let name = format!("{prefix}_{}", k.replace('-', '_'.to_string().as_str()));
                    variables.insert(name.clone(), Value::Null);
                    *v = json!(format!("{{{{{name}}}}}"));
                } else {
                    parameterize(v, variables, &format!("{prefix}_{k}"));
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter_mut().enumerate() {
                parameterize(v, variables, &format!("{prefix}_{i}"));
            }
        }
        _ => {}
    }
}
fn remove_sensitive_baseline(value: &mut Value, path: &str, ignored: &mut Vec<String>) {
    match value {
        Value::Object(items) => {
            for (k, v) in items {
                let p = format!("{path}/{}", pointer_key(k));
                if sensitive(k) {
                    *v = json!("<redacted>");
                    ignored.push(p);
                } else {
                    remove_sensitive_baseline(v, &p, ignored);
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter_mut().enumerate() {
                remove_sensitive_baseline(v, &format!("{path}/{i}"), ignored);
            }
        }
        _ => {}
    }
}

pub fn junit(report: &RunReport) -> String {
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
        escape(&report.scenario_name),
        report.steps.len(),
        report.failed,
        report
            .steps
            .iter()
            .filter(|s| s.status == "skipped")
            .count()
    );
    for step in &report.steps {
        xml.push_str(&format!(
            "<testcase name=\"{}\" time=\"{}\">",
            escape(&step.name),
            step.duration_ms as f64 / 1000.0
        ));
        if step.status == "skipped" {
            xml.push_str("<skipped/>");
        } else if step.status != "passed" {
            let message = step
                .checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| format!("{}: {}", c.field, c.message))
                .collect::<Vec<_>>()
                .join("; ");
            xml.push_str(&format!("<failure message=\"{}\"/>", escape(&message)));
        }
        xml.push_str("</testcase>");
    }
    xml.push_str("</testsuite>");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_diff_preserves_types_and_array_order() {
        let mut diff = Vec::new();
        json_differences(
            "",
            &json!({"total":10,"items":[1,2]}),
            &json!({"items":[2,1],"total":"10"}),
            &mut diff,
        );
        assert!(diff.contains(&"/total".into()));
        assert!(diff.contains(&"/items/0".into()));
    }
    #[test]
    fn templates_preserve_json_types_and_missing_inputs_fail() {
        let vars = BTreeMap::from([("id".into(), json!(42))]);
        assert_eq!(
            render_json(&json!({"id":"{{id}}"}), &vars).unwrap(),
            json!({"id":42})
        );
        assert!(render_text("{{missing}}", &vars).is_err());
    }
    #[test]
    fn secrets_are_removed_from_report_objects() {
        assert_eq!(
            redact(json!({"access_token":"secret","nested":{"password":"secret","count":1}})),
            json!({"access_token":"<redacted>","nested":{"password":"<redacted>","count":1}})
        );
    }
    #[test]
    fn origins_cannot_contain_paths_credentials_or_other_protocols() {
        for origin in [
            "file:///tmp/test",
            "http://u:p@localhost",
            "http://localhost/path",
            "http://localhost?x=1",
        ] {
            assert!(validate_origin(origin).is_err());
        }
        assert!(validate_origin("http://127.0.0.1:1234").is_ok());
    }
}
