//! An isolated local HTTP application for testing sessions and dynamic workflows.
use crate::scenario::Scenario;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct LabState(Arc<Mutex<LabData>>);
#[derive(Default)]
struct LabData {
    sessions: HashMap<String, String>,
    orders: HashMap<String, (String, Value)>,
}
pub fn router() -> Router {
    Router::new()
        .route("/lab/api/login", post(login))
        .route("/lab/api/orders", post(create_order))
        .route("/lab/api/orders/{id}", get(order))
        .with_state(LabState::default())
}
async fn login(State(state): State<LabState>, Json(_): Json<Value>) -> Response {
    let token = uuid::Uuid::new_v4().to_string();
    let session = uuid::Uuid::new_v4().to_string();
    let mut data = state.0.lock().unwrap();
    if data.sessions.len() >= 1000 {
        data.sessions.clear();
        data.orders.clear();
    }
    data.sessions.insert(token.clone(), session.clone());
    (
        [(
            header::SET_COOKIE,
            format!("omni_lab={session}; HttpOnly; SameSite=Strict; Path=/"),
        )],
        Json(json!({"token":token,"user":{"name":"Demo operator"}})),
    )
        .into_response()
}
fn authenticated(headers: &HeaderMap, data: &LabData) -> Option<String> {
    let token = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    let session = data.sessions.get(token)?;
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie
        .split(';')
        .any(|part| part.trim() == format!("omni_lab={session}"))
        .then(|| token.to_string())
}
async fn create_order(
    State(state): State<LabState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let mut data = state.0.lock().unwrap();
    let Some(token) = authenticated(&headers, &data) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"both bearer token and session cookie are required"})),
        )
            .into_response();
    };
    let Some(quantity) = body["quantity"].as_u64().filter(|q| (1..=100).contains(q)) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"quantity must be an integer from 1 to 100"})),
        )
            .into_response();
    };
    if data.orders.len() >= 1000 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let id = uuid::Uuid::new_v4().to_string();
    let order =
        json!({"id":id,"quantity":quantity,"total":quantity*90,"currency":"CNY","state":"created"});
    data.orders.insert(id, (token, order.clone()));
    (StatusCode::CREATED, Json(order)).into_response()
}
async fn order(
    State(state): State<LabState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let data = state.0.lock().unwrap();
    let Some(token) = authenticated(&headers, &data) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match data.orders.get(&id) {
        Some((owner, order)) if *owner == token => Json(order.clone()).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
pub fn demo_scenario(origin: &str) -> Scenario {
    serde_json::from_value(json!({
        "schema_version":1,"id":"demo-checkout","name":"订单全链路 · 会话与动态变量","description":"真实本地 HTTP 请求：登录提取 token → 创建订单提取 ID → 查询并验证金额。每次运行使用独立 cookie 会话。","base_url":origin,"timeout_ms":5000,
        "variables":{"quantity":2},"steps":[
            {"id":"login","name":"登录并建立会话","method":"POST","path":"/lab/api/login","body":{"username":"demo"},"extract":{"token":"/token"},"expect":{"status":200,"assertions":[{"pointer":"/token","op":"type","value":"string"}]}},
            {"id":"create","name":"创建订单并提取 ID","method":"POST","path":"/lab/api/orders","headers":{"authorization":"Bearer {{token}}"},"body":{"quantity":"{{quantity}}"},"extract":{"order_id":"/id"},"expect":{"status":201,"json":{"id":"<dynamic>","quantity":2,"total":180,"currency":"CNY","state":"created"},"ignore_json":["/id"]}},
            {"id":"verify","name":"查询订单并验证业务结果","method":"GET","path":"/lab/api/orders/{{order_id}}","headers":{"authorization":"Bearer {{token}}"},"expect":{"status":200,"json":{"id":"{{order_id}}","quantity":2,"total":180,"currency":"CNY","state":"created"},"max_latency_ms":1000}}
        ]
    })).unwrap()
}
