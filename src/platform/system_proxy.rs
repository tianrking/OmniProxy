use anyhow::{Context, Result, bail};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKind {
    MacOs,
    Windows,
    Linux,
    Other,
}

pub fn current_platform() -> PlatformKind {
    if cfg!(target_os = "macos") {
        PlatformKind::MacOs
    } else if cfg!(target_os = "windows") {
        PlatformKind::Windows
    } else if cfg!(target_os = "linux") {
        PlatformKind::Linux
    } else {
        PlatformKind::Other
    }
}

pub fn platform_name() -> &'static str {
    match current_platform() {
        PlatformKind::MacOs => "macos",
        PlatformKind::Windows => "windows",
        PlatformKind::Linux => "linux",
        PlatformKind::Other => "other",
    }
}

pub fn set_system_proxy(network_service: &str, host: &str, port: u16) -> Result<()> {
    match current_platform() {
        PlatformKind::MacOs => {
            run_cmd(
                "networksetup",
                &["-setwebproxy", network_service, host, &port.to_string()],
            )?;
            run_cmd(
                "networksetup",
                &[
                    "-setsecurewebproxy",
                    network_service,
                    host,
                    &port.to_string(),
                ],
            )?;
            run_cmd(
                "networksetup",
                &["-setwebproxystate", network_service, "on"],
            )?;
            run_cmd(
                "networksetup",
                &["-setsecurewebproxystate", network_service, "on"],
            )?;
            Ok(())
        }
        PlatformKind::Windows | PlatformKind::Linux | PlatformKind::Other => bail!(
            "auto system proxy not implemented for this platform yet. See emitted *_hint lines."
        ),
    }
}

pub fn unset_system_proxy(network_service: &str) -> Result<()> {
    match current_platform() {
        PlatformKind::MacOs => {
            run_cmd(
                "networksetup",
                &["-setwebproxystate", network_service, "off"],
            )?;
            run_cmd(
                "networksetup",
                &["-setsecurewebproxystate", network_service, "off"],
            )?;
            Ok(())
        }
        PlatformKind::Windows | PlatformKind::Linux | PlatformKind::Other => bail!(
            "auto system proxy reset not implemented for this platform yet. See emitted *_hint lines."
        ),
    }
}

pub fn set_proxy_hint(host: &str, port: u16, network_service: &str) -> String {
    match current_platform() {
        PlatformKind::MacOs => format!(
            "networksetup -setwebproxy \"{svc}\" {host} {port} && networksetup -setsecurewebproxy \"{svc}\" {host} {port} && networksetup -setwebproxystate \"{svc}\" on && networksetup -setsecurewebproxystate \"{svc}\" on",
            svc = network_service
        ),
        PlatformKind::Windows => format!(
            "powershell -Command \"netsh winhttp set proxy proxy-server=\\\"http={host}:{port};https={host}:{port}\\\"\"",
        ),
        PlatformKind::Linux => {
            format!("export HTTP_PROXY=http://{host}:{port} HTTPS_PROXY=http://{host}:{port}",)
        }
        PlatformKind::Other => {
            format!("set HTTP(S) proxy manually to {host}:{port} in your OS/network settings")
        }
    }
}

pub fn unset_proxy_hint(network_service: &str) -> String {
    match current_platform() {
        PlatformKind::MacOs => format!(
            "networksetup -setwebproxystate \"{svc}\" off && networksetup -setsecurewebproxystate \"{svc}\" off",
            svc = network_service
        ),
        PlatformKind::Windows => "powershell -Command \"netsh winhttp reset proxy\"".to_string(),
        PlatformKind::Linux => "unset HTTP_PROXY HTTPS_PROXY".to_string(),
        PlatformKind::Other => "clear system proxy manually in OS settings".to_string(),
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("failed to execute {} {}", cmd, args.join(" ")))?;
    if !status.success() {
        bail!("command failed: {} {}", cmd, args.join(" "));
    }
    Ok(())
}
