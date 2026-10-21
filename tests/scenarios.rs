use omni_proxy::{
    faults::{FaultKind, FaultSpec, run_with_fault},
    lab,
    scenario::{junit, run_scenario},
};
use serde_json::json;
use std::collections::BTreeMap;

async fn lab_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, lab::router()).await.unwrap() });
    (origin, task)
}
fn fault(kind: FaultKind) -> FaultSpec {
    FaultSpec {
        path: "/lab/api/orders".into(),
        kind,
        delay_ms: 300,
        status: 503,
        pointer: "/total".into(),
        value: json!(999),
    }
}

#[tokio::test]
async fn an_unmatched_fault_cannot_be_reported_as_a_successful_experiment() {
    let (origin, server) = lab_server().await;
    let scenario = lab::demo_scenario(&origin);
    let mut spec = fault(FaultKind::Status);
    spec.path = "/never-requested".into();
    let report = run_with_fault(&scenario, None, BTreeMap::new(), Some(spec))
        .await
        .unwrap();
    server.abort();
    assert_eq!(report.status, "failed");
    assert_eq!(report.fault_matches, Some(0));
    assert_eq!(report.steps.last().unwrap().checks[0].field, "fault.path");
}

#[tokio::test]
async fn recorded_auth_cookie_and_dynamic_id_are_linked_into_a_fresh_runnable_workflow() {
    use base64::Engine as _;
    let (origin, server) = lab_server().await;
    let mut events = Vec::new();
    let order =
        json!({"id":"old-order-1234","quantity":2,"total":180,"currency":"CNY","state":"created"});
    for (index, (method, path, body, response, status)) in [
        (
            "POST",
            "/lab/api/login",
            Some(json!({"username":"demo"})),
            json!({"token":"old-token-1234","user":{"name":"Demo operator"}}),
            200,
        ),
        (
            "POST",
            "/lab/api/orders",
            Some(json!({"quantity":2})),
            order.clone(),
            201,
        ),
        ("GET", "/lab/api/orders/old-order-1234", None, order, 200),
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = body
            .map(|v| serde_json::to_vec(&v).unwrap())
            .unwrap_or_default();
        let headers = if index == 0 {
            json!([])
        } else {
            json!([
                ["authorization", "Bearer old-token-1234"],
                ["cookie", "omni_lab=old-session-1234"]
            ])
        };
        events.push(json!({"type":"HttpRequest","request_id":index.to_string(),"client":"c","method":method,"uri":format!("{origin}{path}"),"headers":headers,"body_b64":base64::engine::general_purpose::STANDARD.encode(&bytes),"body_size":bytes.len()}));
        let bytes = serde_json::to_vec(&response).unwrap();
        events.push(json!({"type":"HttpResponse","request_id":index.to_string(),"client":"c","status":status,"headers":if index==0{json!([["set-cookie","omni_lab=old-session-1234; Path=/"]])}else{json!([])},"body_b64":base64::engine::general_purpose::STANDARD.encode(&bytes),"body_size":bytes.len()}));
    }
    let capture = omni_proxy::replay::parse_capture(std::io::Cursor::new(
        events
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    ))
    .unwrap();
    let scenario =
        omni_proxy::scenario::from_capture(&capture, &[0, 2, 4], "Recorded workflow".into())
            .unwrap();
    let exported = serde_json::to_string(&scenario).unwrap();
    assert!(!exported.contains("old-token-1234"));
    assert!(!exported.contains("old-order-1234"));
    assert!(!exported.contains("old-session-1234"));
    assert!(scenario.variables.is_empty());
    assert!(!scenario.steps[0].extract.is_empty());
    let report = run_scenario(&scenario, None, BTreeMap::new())
        .await
        .unwrap();
    server.abort();
    assert_eq!(report.status, "passed", "{report:?}");
    assert_eq!(report.passed, 3);
}
#[tokio::test]
async fn dynamic_tokens_cookies_and_order_ids_survive_a_full_workflow() {
    let (origin, server) = lab_server().await;
    let scenario = lab::demo_scenario(&origin);
    let report = run_scenario(&scenario, None, BTreeMap::new())
        .await
        .unwrap();
    server.abort();
    assert_eq!(report.status, "passed", "{report:?}");
    assert_eq!(report.passed, 3);
    assert_eq!(report.steps[2].http_status, Some(200));
    assert_eq!(
        report.steps[0].response.as_ref().unwrap()["token"],
        "<redacted>"
    );
    assert!(junit(&report).contains("failures=\"0\""));
}
#[tokio::test]
async fn fault_is_reproducible_baseline_is_unchanged_and_failed_dependencies_are_skipped() {
    let (origin, server) = lab_server().await;
    let scenario = lab::demo_scenario(&origin);
    for _ in 0..2 {
        let report = run_with_fault(
            &scenario,
            None,
            BTreeMap::new(),
            Some(fault(FaultKind::Status)),
        )
        .await
        .unwrap();
        assert_eq!(report.steps[1].http_status, Some(503));
        assert_eq!(report.failed, 1);
        assert_eq!(report.steps[2].status, "skipped");
        assert!(junit(&report).contains("failures=\"1\""));
        assert!(report.fault.is_some());
    }
    assert_eq!(
        run_scenario(&scenario, None, BTreeMap::new())
            .await
            .unwrap()
            .status,
        "passed"
    );
    server.abort();
}
#[tokio::test]
async fn semantic_drift_is_detected_even_when_http_status_is_success() {
    let (origin, server) = lab_server().await;
    let scenario = lab::demo_scenario(&origin);
    let report = run_with_fault(
        &scenario,
        None,
        BTreeMap::new(),
        Some(fault(FaultKind::Json)),
    )
    .await
    .unwrap();
    server.abort();
    assert_eq!(report.steps[1].http_status, Some(201));
    assert_eq!(report.status, "failed");
    assert!(
        report.steps[1]
            .checks
            .iter()
            .any(|check| !check.passed && check.message.contains("/total"))
    );
}
#[tokio::test]
async fn invalid_json_disconnect_and_timeout_cannot_pass_as_empty_success() {
    let (origin, server) = lab_server().await;
    let mut scenario = lab::demo_scenario(&origin);
    scenario.timeout_ms = 100;
    for kind in [
        FaultKind::InvalidJson,
        FaultKind::Disconnect,
        FaultKind::Delay,
    ] {
        let report = run_with_fault(&scenario, None, BTreeMap::new(), Some(fault(kind)))
            .await
            .unwrap();
        assert_eq!(report.status, "failed", "{report:?}");
        assert_eq!(report.steps[2].status, "skipped");
    }
    server.abort();
}
#[tokio::test]
async fn missing_variables_fail_before_sending_and_type_changes_fail_regression() {
    let (origin, server) = lab_server().await;
    let mut scenario = lab::demo_scenario(&origin);
    scenario.variables.insert("quantity".into(), json!("2"));
    assert_eq!(
        run_scenario(&scenario, None, BTreeMap::new())
            .await
            .unwrap()
            .status,
        "failed"
    );
    scenario.steps[0].path = "/lab/api/{{missing}}".into();
    let report = run_scenario(&scenario, None, BTreeMap::new())
        .await
        .unwrap();
    server.abort();
    assert_eq!(report.steps[0].status, "error");
    assert_eq!(report.steps[1].status, "skipped");
}
#[tokio::test]
async fn no_implicit_redirects_and_ignore_does_not_hide_missing_fields() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route(
            "/redirect",
            axum::routing::post(|| async { axum::response::Redirect::temporary("/lab/api/login") }),
        )
        .merge(lab::router());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut scenario = lab::demo_scenario(&origin);
    scenario.steps.truncate(1);
    scenario.steps[0].path = "/redirect".into();
    assert_eq!(
        run_scenario(&scenario, None, BTreeMap::new())
            .await
            .unwrap()
            .steps[0]
            .http_status,
        Some(307)
    );
    scenario.steps[0].path = "/lab/api/login".into();
    scenario.steps[0].expect.json =
        Some(json!({"token":"<ignored>","user":{"name":"Demo operator"},"missing":1}));
    scenario.steps[0].expect.ignore_json = vec!["/token".into(), "/missing".into()];
    assert_eq!(
        run_scenario(&scenario, None, BTreeMap::new())
            .await
            .unwrap()
            .status,
        "failed"
    );
    server.abort();
}
