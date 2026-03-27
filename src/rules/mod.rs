use crate::query::{EvalContext, Expr, parse};
use anyhow::{Result, bail};
use std::{fs, path::Path};

#[derive(Debug, Clone)]
enum RuleAction {
    Deny {
        when: Expr,
    },
    SetReqHeader {
        name: String,
        value: String,
        when: Expr,
    },
    SetResHeader {
        name: String,
        value: String,
        when: Expr,
    },
    SetResStatus {
        status: u16,
        when: Expr,
    },
    ReplaceResBody {
        body: String,
        when: Expr,
    },
}

#[derive(Debug, Clone, Default)]
pub struct RuleEngine {
    actions: Vec<RuleAction>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    pub method: String,
    pub uri: String,
    pub host: String,
}

#[derive(Debug, Clone, Default)]
pub struct RequestOutcome {
    pub denied: bool,
    pub add_headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct ResponseOutcome {
    pub add_headers: Vec<(String, String)>,
    pub override_status: Option<u16>,
    pub replace_body: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleStats {
    pub deny_rules: usize,
    pub req_header_rules: usize,
    pub res_header_rules: usize,
    pub res_status_rules: usize,
    pub res_body_rules: usize,
}

impl RuleEngine {
    pub fn load(rule_file: &Path) -> Result<Self> {
        if !rule_file.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(rule_file)?;
        let mut actions = Vec::new();

        for (ln, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let action = parse_rule_line(line).map_err(|e| {
                anyhow::anyhow!("rule parse error at line {}: {} | {}", ln + 1, e, line)
            })?;
            actions.push(action);
        }

        Ok(Self { actions })
    }

    pub fn eval_request(&self, req: &RequestMeta) -> RequestOutcome {
        let mut out = RequestOutcome::default();
        let ctx = EvalContext {
            req_method: Some(req.method.clone()),
            req_uri: Some(req.uri.clone()),
            req_host: Some(req.host.clone()),
            res_status: None,
        };

        for action in &self.actions {
            match action {
                RuleAction::Deny { when } => {
                    if when.eval(&ctx) {
                        out.denied = true;
                    }
                }
                RuleAction::SetReqHeader { name, value, when } => {
                    if when.eval(&ctx) {
                        out.add_headers.push((name.clone(), value.clone()));
                    }
                }
                RuleAction::SetResHeader { .. }
                | RuleAction::SetResStatus { .. }
                | RuleAction::ReplaceResBody { .. } => {}
            }
        }

        out
    }

    pub fn eval_response(&self, req: &RequestMeta, status: u16) -> ResponseOutcome {
        let mut out = ResponseOutcome::default();
        let ctx = EvalContext {
            req_method: Some(req.method.clone()),
            req_uri: Some(req.uri.clone()),
            req_host: Some(req.host.clone()),
            res_status: Some(status),
        };

        for action in &self.actions {
            match action {
                RuleAction::SetResHeader { name, value, when } => {
                    if when.eval(&ctx) {
                        out.add_headers.push((name.clone(), value.clone()));
                    }
                }
                RuleAction::SetResStatus { status, when } => {
                    if when.eval(&ctx) {
                        out.override_status = Some(*status);
                    }
                }
                RuleAction::ReplaceResBody { body, when } => {
                    if when.eval(&ctx) {
                        out.replace_body = Some(body.clone());
                    }
                }
                RuleAction::Deny { .. } | RuleAction::SetReqHeader { .. } => {}
            }
        }

        out
    }

    pub fn stats(&self) -> RuleStats {
        let mut s = RuleStats::default();
        for action in &self.actions {
            match action {
                RuleAction::Deny { .. } => s.deny_rules += 1,
                RuleAction::SetReqHeader { .. } => s.req_header_rules += 1,
                RuleAction::SetResHeader { .. } => s.res_header_rules += 1,
                RuleAction::SetResStatus { .. } => s.res_status_rules += 1,
                RuleAction::ReplaceResBody { .. } => s.res_body_rules += 1,
            }
        }
        s
    }

    pub fn count(&self) -> usize {
        self.actions.len()
    }
}

fn parse_rule_line(line: &str) -> Result<RuleAction> {
    if let Some(expr) = line.strip_prefix("deny ") {
        return Ok(RuleAction::Deny { when: parse(expr)? });
    }

    if let Some(rest) = line.strip_prefix("req.set_header ") {
        let (name, value, when) = parse_set_header(rest)?;
        return Ok(RuleAction::SetReqHeader {
            name,
            value,
            when: parse(when)?,
        });
    }

    if let Some(rest) = line.strip_prefix("res.set_header ") {
        let (name, value, when) = parse_set_header(rest)?;
        return Ok(RuleAction::SetResHeader {
            name,
            value,
            when: parse(when)?,
        });
    }

    if let Some(rest) = line.strip_prefix("res.set_status ") {
        let (raw_status, when) = rest
            .split_once(" if ")
            .ok_or_else(|| anyhow::anyhow!("res.set_status requires ' if <expr>'"))?;
        let status: u16 = raw_status.trim().parse()?;
        return Ok(RuleAction::SetResStatus {
            status,
            when: parse(when.trim())?,
        });
    }

    if let Some(rest) = line.strip_prefix("res.replace_body ") {
        let (raw_body, when) = rest
            .split_once(" if ")
            .ok_or_else(|| anyhow::anyhow!("res.replace_body requires ' if <expr>'"))?;
        let body = raw_body.trim().trim_matches('"').to_string();
        return Ok(RuleAction::ReplaceResBody {
            body,
            when: parse(when.trim())?,
        });
    }

    // Backward compatibility: bare expression means deny.
    Ok(RuleAction::Deny { when: parse(line)? })
}

fn parse_set_header(input: &str) -> Result<(String, String, &str)> {
    let (left, when) = input
        .split_once(" if ")
        .ok_or_else(|| anyhow::anyhow!("set_header rule requires ' if <expr>'"))?;
    let (name, value) = left
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("set_header rule requires 'Header: Value'"))?;

    let name = name.trim();
    let value = value.trim();
    if name.is_empty() {
        bail!("header name is empty");
    }
    if when.trim().is_empty() {
        bail!("condition expression is empty");
    }

    Ok((name.to_string(), value.to_string(), when.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_rule_file(content: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid time")
            .as_nanos();
        p.push(format!("omni_rules_{ts}.txt"));
        fs::write(&p, content).expect("write rule file");
        p
    }

    #[test]
    fn request_deny_and_req_header_rules_work() {
        let path = write_rule_file(
            r#"
deny req.method == POST
req.set_header x-test: yes if req.host == "api.example.com"
"#,
        );

        let rules = RuleEngine::load(&path).expect("load rules");
        let _ = fs::remove_file(&path);
        let req = RequestMeta {
            method: "POST".into(),
            uri: "https://api.example.com/v1/items".into(),
            host: "api.example.com".into(),
        };
        let outcome = rules.eval_request(&req);
        assert!(outcome.denied);
        assert_eq!(outcome.add_headers, vec![("x-test".into(), "yes".into())]);
    }

    #[test]
    fn response_rules_apply_status_header_and_body() {
        let path = write_rule_file(
            r#"
res.set_header x-policy: hit if res.status >= 400
res.set_status 418 if req.uri ~= "/teapot"
res.replace_body "rewritten" if req.method == GET
"#,
        );
        let rules = RuleEngine::load(&path).expect("load rules");
        let _ = fs::remove_file(&path);
        let req = RequestMeta {
            method: "GET".into(),
            uri: "https://svc.local/teapot".into(),
            host: "svc.local".into(),
        };

        let out = rules.eval_response(&req, 500);
        assert_eq!(out.add_headers, vec![("x-policy".into(), "hit".into())]);
        assert_eq!(out.override_status, Some(418));
        assert_eq!(out.replace_body, Some("rewritten".into()));
    }

    #[test]
    fn invalid_rule_returns_line_number() {
        let path = write_rule_file(
            r#"
res.set_status abc if req.method == GET
"#,
        );
        let err = RuleEngine::load(&path).expect_err("should fail");
        let _ = fs::remove_file(&path);
        let msg = err.to_string();
        assert!(msg.contains("line 2"), "unexpected error: {msg}");
    }
}
