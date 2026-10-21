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
