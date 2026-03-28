use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use omni_proxy::{
    api::ApiEvent,
    query::{EvalContext, Expr, parse},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use reqwest::Method;
use std::{
    collections::{HashMap, VecDeque},
    io,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

#[derive(Debug, Parser)]
#[command(name = "omni-tui", about = "OmniProxy geek-first TUI")]
struct Cli {
    #[arg(long, default_value = "ws://127.0.0.1:9091/ws")]
    api: String,
}

#[derive(Debug, Clone)]
struct Flow {
    client: String,
    request_id: Option<String>,
    method: Option<String>,
    uri: Option<String>,
    status: Option<u16>,
    req_ts_ms: Option<u64>,
    res_ts_ms: Option<u64>,
    req_body_size: Option<usize>,
    res_body_size: Option<usize>,
    req_capture_reason: Option<String>,
    res_capture_reason: Option<String>,
    ws_frames: usize,
    ws_bytes: usize,
    is_connect: bool,
}

#[derive(Debug)]
enum UiEvent {
    Api(ApiEvent),
    WsConnected,
    WsDisconnected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Filter,
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let (tx, mut rx) = mpsc::unbounded_channel::<UiEvent>();
    let ws_url = cli.api.clone();
    tokio::spawn(async move {
        if let Err(err) = ws_reader_task(&ws_url, tx).await {
            eprintln!("[omni-tui] ws reader exited: {err}");
        }
    });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::default();

    let loop_result = run_loop(&mut terminal, &mut app, &mut rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    loop_result
}

async fn ws_reader_task(url: &str, tx: mpsc::UnboundedSender<UiEvent>) -> Result<()> {
    loop {
        match connect_async(url).await {
            Ok((ws, _)) => {
                let _ = tx.send(UiEvent::WsConnected);
                let (_, mut read) = ws.split();

                while let Some(msg) = read.next().await {
                    let msg = msg?;
                    if !msg.is_text() {
                        continue;
                    }
                    let text = msg.into_text()?;
                    if let Ok(event) = serde_json::from_str::<ApiEvent>(&text) {
                        let _ = tx.send(UiEvent::Api(event));
                    }
                }
                let _ = tx.send(UiEvent::WsDisconnected("socket closed".into()));
            }
            Err(err) => {
                let _ = tx.send(UiEvent::WsDisconnected(err.to_string()));
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    rx: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> Result<()> {
    loop {
        while let Ok(event) = rx.try_recv() {
            app.ingest(event);
        }

        terminal.draw(|f| draw_ui(f, app))?;

        if event::poll(Duration::from_millis(80))? {
            let CEvent::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match app.input_mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => app.next(),
                    KeyCode::Up | KeyCode::Char('k') => app.prev(),
                    KeyCode::Char('g') => app.first(),
                    KeyCode::Char('G') => app.last(),
                    KeyCode::Char('r') => {
                        app.replay_selected().await;
                    }
                    KeyCode::Char('x') => app.toggle_hide_connect(),
                    KeyCode::Char('/') => {
                        app.input_mode = InputMode::Filter;
                        app.filter_buffer.clear();
                    }
                    KeyCode::Char('c') => app.clear(),
                    _ => {}
                },
                InputMode::Filter => match key.code {
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.filter_buffer.clear();
                    }
                    KeyCode::Enter => {
                        app.apply_filter();
                        app.input_mode = InputMode::Normal;
                    }
                    KeyCode::Backspace => {
                        app.filter_buffer.pop();
                    }
                    KeyCode::Char(ch) => app.filter_buffer.push(ch),
                    _ => {}
                },
            }
        }
    }
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[0]);

    let indices = app.filtered_indices();
    let items: Vec<ListItem> = indices
        .iter()
        .map(|idx| {
            let flow = &app.flows[*idx];
            let method = flow.method.clone().unwrap_or_else(|| "-".into());
            let uri = flow.uri.clone().unwrap_or_else(|| "-".into());
            let status = flow
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".into());
            let latency = flow
                .req_ts_ms
                .zip(flow.res_ts_ms)
                .map(|(a, b)| b.saturating_sub(a).to_string())
                .unwrap_or_else(|| "-".into());

            ListItem::new(Line::from(vec![
                Span::styled(method, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::raw(status),
                Span::raw("  "),
                Span::raw(format!("{}ms", latency)),
                Span::raw("  "),
                Span::raw(uri),
            ]))
        })
        .collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(app.selected_visible());
    let list = List::new(items)
        .block(
            Block::default()
                .title("Flows (j/k, g/G, /, r replay, x hide CONNECT, c clear, q quit)")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, body[0], &mut state);

    let detail = if let Some(flow) = app.selected_flow() {
        vec![
            Line::from(format!("Client: {}", flow.client)),
            Line::from(format!(
                "Request-Id: {}",
                flow.request_id.as_deref().unwrap_or("-")
            )),
            Line::from(format!("Method: {}", flow.method.as_deref().unwrap_or("-"))),
            Line::from(format!("URI: {}", flow.uri.as_deref().unwrap_or("-"))),
            Line::from(format!(
                "Host: {}",
                flow.uri
                    .as_deref()
                    .and_then(parse_host_from_uri)
                    .unwrap_or("-")
            )),
            Line::from(format!(
                "Status: {}",
                flow.status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".into())
            )),
            Line::from(format!(
                "Latency(ms): {}",
                flow.req_ts_ms
                    .zip(flow.res_ts_ms)
                    .map(|(a, b)| b.saturating_sub(a).to_string())
                    .unwrap_or_else(|| "-".into())
            )),
            Line::from(format!(
                "ReqBody(bytes): {}",
                flow.req_body_size
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            )),
            Line::from(format!(
                "ResBody(bytes): {}",
                flow.res_body_size
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into())
            )),
            Line::from(format!(
                "ReqCapture: {}",
                flow.req_capture_reason.as_deref().unwrap_or("-")
            )),
            Line::from(format!(
                "ResCapture: {}",
                flow.res_capture_reason.as_deref().unwrap_or("-")
            )),
            Line::from(format!("WS Frames/Bytes: {}/{}", flow.ws_frames, flow.ws_bytes)),
        ]
    } else {
        vec![Line::from("No flow selected")]
    };

    frame.render_widget(
        Paragraph::new(detail).block(Block::default().title("Detail").borders(Borders::ALL)),
        body[1],
    );

    let filter_title = match app.input_mode {
        InputMode::Normal => "Filter (press / to edit)",
        InputMode::Filter => "Filter (Enter apply, Esc cancel)",
    };
    frame.render_widget(
        Paragraph::new(app.filter_buffer.as_str())
            .block(Block::default().title(filter_title).borders(Borders::ALL)),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(app.footer_text()).style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

#[derive(Default)]
struct App {
    flows: Vec<Flow>,
    selected: usize,
    input_mode: InputMode,
    filter_buffer: String,
    filter_expr: Option<Expr>,
    status_line: String,
    latest_req_idx_by_client: HashMap<String, usize>,
    pending_req_indexes_by_client: HashMap<String, VecDeque<usize>>,
    req_idx_by_request_id: HashMap<String, usize>,
    ws_connected: bool,
    ws_status: String,
    hide_connect: bool,
    ws_frames_total: usize,
    ws_bytes_total: usize,
}

impl App {
    async fn replay_selected(&mut self) {
        let Some(flow) = self.selected_flow().cloned() else {
            self.status_line = "no flow selected for replay".into();
            return;
        };
        let Some(method_raw) = flow.method else {
            self.status_line = "selected flow has no method".into();
            return;
        };
        let Some(uri) = flow.uri else {
            self.status_line = "selected flow has no uri".into();
            return;
        };
        if !uri.starts_with("http://") && !uri.starts_with("https://") {
            self.status_line = "selected uri is not absolute".into();
            return;
        }
        let method = match Method::from_bytes(method_raw.as_bytes()) {
            Ok(m) => m,
            Err(_) => {
                self.status_line = format!("invalid method for replay: {}", method_raw);
                return;
            }
        };
        let client = reqwest::Client::new();
        match client.request(method, &uri).send().await {
            Ok(resp) => {
                self.status_line = format!("replay ok: {} {}", resp.status().as_u16(), uri);
            }
            Err(err) => {
                self.status_line = format!("replay failed: {}", err);
            }
        }
    }

    fn ingest(&mut self, event: UiEvent) {
        match event {
            UiEvent::WsConnected => {
                self.ws_connected = true;
                self.ws_status = "connected".into();
            }
            UiEvent::WsDisconnected(reason) => {
                self.ws_connected = false;
                self.ws_status = format!("disconnected: {}", reason);
            }
            UiEvent::Api(ApiEvent::HttpRequest {
                timestamp_ms,
                request_id,
                client,
                method,
                uri,
                body_size,
                body_capture_reason,
                ..
            }) => {
                let idx = self.flows.len();
                let is_connect = method.eq_ignore_ascii_case("CONNECT");
                self.flows.push(Flow {
                    client: client.clone(),
                    request_id: request_id.clone(),
                    method: Some(method),
                    uri: Some(uri),
                    status: None,
                    req_ts_ms: Some(timestamp_ms),
                    res_ts_ms: None,
                    req_body_size: body_size,
                    res_body_size: None,
                    req_capture_reason: body_capture_reason,
                    res_capture_reason: None,
                    ws_frames: 0,
                    ws_bytes: 0,
                    is_connect,
                });
                self.latest_req_idx_by_client.insert(client.clone(), idx);
                self.pending_req_indexes_by_client
                    .entry(client)
                    .or_default()
                    .push_back(idx);
                if let Some(req_id) = request_id {
                    self.req_idx_by_request_id.insert(req_id, idx);
                }
            }
            UiEvent::Api(ApiEvent::HttpResponse {
                timestamp_ms,
                request_id,
                client,
                status,
                body_size,
                body_capture_reason,
                ..
            }) => {
                let idx = request_id
                    .as_deref()
                    .and_then(|id| self.req_idx_by_request_id.get(id).copied())
                    .or_else(|| {
                        self.pending_req_indexes_by_client
                            .get_mut(&client)
                            .and_then(|q| q.pop_front())
                    })
                    .or_else(|| self.latest_req_idx_by_client.get(&client).copied());

                if let Some(idx) = idx {
                    if let Some(flow) = self.flows.get_mut(idx) {
                        flow.status = Some(status);
                        flow.res_ts_ms = Some(timestamp_ms);
                        flow.res_body_size = body_size;
                        flow.res_capture_reason = body_capture_reason;
                    }
                    if let Some(req_id) = request_id {
                        self.req_idx_by_request_id.remove(&req_id);
                    }
                    if let Some(q) = self.pending_req_indexes_by_client.get_mut(&client)
                        && let Some(pos) = q.iter().position(|v| *v == idx)
                    {
                        let _ = q.remove(pos);
                    }
                } else {
                    self.flows.push(Flow {
                        client,
                        request_id,
                        method: None,
                        uri: None,
                        status: Some(status),
                        req_ts_ms: None,
                        res_ts_ms: Some(timestamp_ms),
                        req_body_size: None,
                        res_body_size: body_size,
                        req_capture_reason: None,
                        res_capture_reason: body_capture_reason,
                        ws_frames: 0,
                        ws_bytes: 0,
                        is_connect: false,
                    });
                }
            }
            UiEvent::Api(ApiEvent::WebSocketFrame {
                client, payload_len, ..
            }) => {
                self.ws_frames_total = self.ws_frames_total.saturating_add(1);
                self.ws_bytes_total = self.ws_bytes_total.saturating_add(payload_len);
                if let Some(client) = client
                    && let Some(idx) = self.latest_req_idx_by_client.get(&client).copied()
                    && let Some(flow) = self.flows.get_mut(idx)
                {
                    flow.ws_frames = flow.ws_frames.saturating_add(1);
                    flow.ws_bytes = flow.ws_bytes.saturating_add(payload_len);
                }
            }
        }

        if !self.flows.is_empty() {
            self.selected = self.selected.min(self.flows.len() - 1);
        }
    }

    fn apply_filter(&mut self) {
        let s = self.filter_buffer.trim();
        if s.is_empty() {
            self.filter_expr = None;
            self.status_line = "filter cleared".into();
            return;
        }

        match parse(s) {
            Ok(expr) => {
                self.filter_expr = Some(expr);
                self.status_line = format!("filter applied: {}", s);
            }
            Err(err) => {
                self.status_line = format!("invalid filter: {err}");
            }
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        self.flows
            .iter()
            .enumerate()
            .filter(|(_, f)| self.matches(f))
            .map(|(i, _)| i)
            .collect()
    }

    fn matches(&self, flow: &Flow) -> bool {
        if self.hide_connect && flow.is_connect {
            return false;
        }

        let Some(expr) = &self.filter_expr else {
            return true;
        };

        let host = flow
            .uri
            .as_deref()
            .and_then(parse_host_from_uri)
            .map(|s| s.to_string());
        let ctx = EvalContext {
            req_method: flow.method.clone(),
            req_uri: flow.uri.clone(),
            req_host: host,
            res_status: flow.status,
        };
        expr.eval(&ctx)
    }

    fn selected_flow(&self) -> Option<&Flow> {
        let idxs = self.filtered_indices();
        let idx = idxs.get(self.selected)?;
        self.flows.get(*idx)
    }

    fn selected_visible(&self) -> Option<usize> {
        let len = self.filtered_indices().len();
        if len == 0 {
            None
        } else {
            Some(self.selected.min(len.saturating_sub(1)))
        }
    }

    fn next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected + 1).min(len - 1);
    }

    fn prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn first(&mut self) {
        self.selected = 0;
    }

    fn last(&mut self) {
        let len = self.filtered_indices().len();
        self.selected = len.saturating_sub(1);
    }

    fn clear(&mut self) {
        self.flows.clear();
        self.latest_req_idx_by_client.clear();
        self.pending_req_indexes_by_client.clear();
        self.req_idx_by_request_id.clear();
        self.selected = 0;
        self.ws_frames_total = 0;
        self.ws_bytes_total = 0;
        self.status_line = "flows cleared".into();
    }

    fn toggle_hide_connect(&mut self) {
        self.hide_connect = !self.hide_connect;
        self.status_line = format!(
            "hide CONNECT: {}",
            if self.hide_connect { "on" } else { "off" }
        );
    }

    fn footer_text(&self) -> String {
        let total = self.flows.len();
        let visible = self.filtered_indices().len();
        let mode = match self.input_mode {
            InputMode::Normal => "NORMAL",
            InputMode::Filter => "FILTER",
        };
        let ws = if self.ws_connected { "ws=up" } else { "ws=down" };
        let connect = if self.hide_connect {
            "connect=hidden"
        } else {
            "connect=shown"
        };
        let ws_stats = format!(
            "ws_frames={} ws_bytes={}",
            self.ws_frames_total, self.ws_bytes_total
        );
        let ws_status = if self.ws_status.is_empty() {
            "-".to_string()
        } else {
            self.ws_status.clone()
        };

        if self.status_line.is_empty() {
            format!(
                "{} {} {} ws_status={} mode={} visible={}/{}",
                ws, connect, ws_stats, ws_status, mode, visible, total
            )
        } else {
            format!(
                "{} | {} {} {} ws_status={} mode={} visible={}/{}",
                self.status_line, ws, connect, ws_stats, ws_status, mode, visible, total
            )
        }
    }
}

fn parse_host_from_uri(uri: &str) -> Option<&str> {
    let rest = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))?;
    Some(rest.split('/').next()?.split(':').next().unwrap_or(""))
}
