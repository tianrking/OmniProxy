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
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleStats {
    pub deny_rules: usize,
    pub req_header_rules: usize,
    pub res_header_rules: usize,
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
                RuleAction::SetResHeader { .. } => {}
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
            if let RuleAction::SetResHeader { name, value, when } = action
                && when.eval(&ctx)
            {
                out.add_headers.push((name.clone(), value.clone()));
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
