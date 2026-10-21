use omni_proxy::{
    lab,
    workbench::{Workbench, router},
};
use serde_json::{Value, json};
struct Fixture {
    origin: String,
    client: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
    root: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let root =
            std::env::temp_dir().join(format!("omni-workbench-test-{}", uuid::Uuid::new_v4()));
        let state = Workbench::new(root.clone(), address).await.unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        Self {
            origin: format!("http://{address}"),
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            task,
            root,
        }
    }
    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("{}{path}", self.origin))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap()
    }
    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{path}", self.origin))
            .send()
            .await
            .unwrap()
    }
}
async fn json(response: reqwest::Response) -> Value {
    serde_json::from_slice(&response.bytes().await.unwrap()).unwrap()
}
#[tokio::test]
async fn browser_api_saves_runs_persists_and_exports_real_reports() {
    let f = Fixture::new().await;
    let scenario = lab::demo_scenario(&f.origin);
    assert_eq!(
        f.post("/api/scenarios", serde_json::to_value(scenario).unwrap())
            .await
            .status(),
        200
    );
    let report = json(f.post("/api/scenarios/demo-checkout/run", json!({})).await).await;
    assert_eq!(report["status"], "passed", "{report}");
    assert_eq!(report["steps"][0]["response"]["token"], "<redacted>");
    assert_eq!(report["scenario_sha256"].as_str().unwrap().len(), 64);
    let id = report["id"].as_str().unwrap();
    assert_eq!(
        json(f.get(&format!("/api/reports/{id}")).await).await["status"],
        "passed"
    );
    let xml = f.get(&format!("/api/reports/{id}/junit")).await;
    assert_eq!(
        xml.headers()["content-type"],
        "application/xml; charset=utf-8"
    );
    assert!(xml.text().await.unwrap().contains("failures=\"0\""));
    assert_eq!(
        json(f.get("/api/reports").await).await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(f.root.join("reports").join(format!("{id}.json")).exists());
}
#[tokio::test]
async fn browser_origin_host_and_file_boundaries_are_enforced() {
    let f = Fixture::new().await;
    let response = f
        .client
        .get(format!("{}/api/status", f.origin))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let response = f
        .client
        .post(format!("{}/api/scenarios/demo-checkout/run", f.origin))
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let response = f.get("/").await;
    assert_eq!(response.status(), 200);
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("Reproduction Workbench")
    );
    assert_eq!(f.get("/api/scenarios/invalid.id").await.status(), 400);
    assert_eq!(f.get("/proxy/ca.key").await.status(), 404);
    assert!(
        Workbench::new(f.root.clone(), "0.0.0.0:9092".parse().unwrap())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn incomplete_captures_cannot_become_false_regression_baselines() {
    let f = Fixture::new().await;
    let events = [
        json!({"type":"HttpRequest","client":"c","request_id":"a","method":"POST","uri":"http://localhost/items","body_truncated":true,"body_size":10}),
        json!({"type":"HttpResponse","client":"c","request_id":"a","status":200,"body_size":0}),
    ];
    let content = events
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let result = json(f.post("/api/captures", json!({"content":content})).await).await;
    let id = result["id"].as_str().unwrap();
    let view = json(f.get(&format!("/api/captures/{id}")).await).await;
    assert_eq!(view["requests"][0]["replayable"], false);
    assert_eq!(
        f.post(
            &format!("/api/captures/{id}/scenario"),
            json!({"indexes":[0],"name":"bad recording"})
        )
        .await
        .status(),
        400
    );
}
#[tokio::test]
async fn empty_assertions_and_invalid_targets_are_rejected_before_run() {
    let f = Fixture::new().await;
    let mut scenario = serde_json::to_value(lab::demo_scenario(&f.origin)).unwrap();
    scenario["steps"][0]["expect"] = json!({});
    assert_eq!(f.post("/api/scenarios", scenario).await.status(), 400);
    assert_eq!(
        f.post(
            "/api/scenarios/demo-checkout/run",
            json!({"target":"file:///tmp"})
        )
        .await
        .status(),
        400
    );
    assert_eq!(
        json(f.get("/api/reports").await).await["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
