use crate::query::{EvalContext, Expr, parse};
use anyhow::Result;
use std::{fs, path::Path};

#[derive(Debug, Clone, Default)]
pub struct RuleEngine {
    deny_rules: Vec<Expr>,
}

impl RuleEngine {
    pub fn load(rule_file: &Path) -> Result<Self> {
        if !rule_file.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(rule_file)?;
        let mut deny_rules = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            deny_rules.push(parse(line)?);
        }
        Ok(Self { deny_rules })
    }

    pub fn should_deny(&self, method: &str, status: Option<u16>) -> bool {
        let ctx = EvalContext {
            req_method: Some(method.to_string()),
            res_status: status,
        };
        self.deny_rules.iter().any(|expr| expr.eval(&ctx))
    }

    pub fn count(&self) -> usize {
        self.deny_rules.len()
    }
}
