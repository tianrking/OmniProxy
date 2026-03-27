use anyhow::Result;
use clap::Parser;
use omni_proxy::cert::diagnose_ca;
use omni_proxy::config::Cli;
use omni_proxy::rules::RuleEngine;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .with_target(false)
        .compact()
        .init();

    let app = omni_proxy::config::AppConfig::from_cli(cli.clone())?;

    if cli.check_rules {
        let rules = RuleEngine::load(&app.rule_file_path)?;
        let s = rules.stats();
        println!(
            "rule_file={}\nrule_count={}\ndeny={}\nreq_set_header={}\nres_set_header={}\nres_set_status={}\nres_replace_body={}",
            app.rule_file_path.display(),
            rules.count(),
            s.deny_rules,
            s.req_header_rules,
            s.res_header_rules,
            s.res_status_rules,
            s.res_body_rules
        );
        return Ok(());
    }

    if cli.diagnose_ca {
        let d = diagnose_ca(&app.ca_cert_path, &app.ca_key_path).await?;
        println!(
            "ca_cert={}\nca_key={}\ncert_exists={}\nkey_exists={}\ncert_size={}\nkey_size={}\npair_parse_ok={}\nmessage={}",
            app.ca_cert_path.display(),
            app.ca_key_path.display(),
            d.cert_exists,
            d.key_exists,
            d.cert_size
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".into()),
            d.key_size
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".into()),
            d.pair_parse_ok,
            d.message
        );
        return Ok(());
    }

    info!(listen = %app.listen_addr, "starting OmniProxy core");

    omni_proxy::proxy::run(app).await
}
