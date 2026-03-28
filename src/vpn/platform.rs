use crate::{
    platform::system_proxy::{set_system_proxy, unset_system_proxy},
    vpn::control::{VpnDoctorReport, VpnSpec, VpnStatus},
};
use anyhow::{Context, Result, bail};
use std::process::Command;

#[derive(Debug, Clone, Copy)]
pub enum PlatformKind {
    MacOs,
    Linux,
    Windows,
    Other,
}

pub fn detect_platform() -> PlatformKind {
    if cfg!(target_os = "macos") {
        PlatformKind::MacOs
    } else if cfg!(target_os = "linux") {
        PlatformKind::Linux
    } else if cfg!(target_os = "windows") {
        PlatformKind::Windows
    } else {
        PlatformKind::Other
    }
}

pub fn platform_name(kind: PlatformKind) -> &'static str {
    match kind {
        PlatformKind::MacOs => "macos",
        PlatformKind::Linux => "linux",
        PlatformKind::Windows => "windows",
        PlatformKind::Other => "other",
    }
}

pub fn up(spec: &VpnSpec) -> Result<()> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let services = list_services()?;
            if services.iter().any(|s| s == &spec.service_name) {
                run_cmd("scutil", &["--nc", "start", &spec.service_name])?;
                return Ok(());
            }

            let (host, port) = parse_host_port(&spec.local_http_proxy)?;
            set_system_proxy(&spec.network_service, &host, port)
                .with_context(|| "fallback to OmniProxy managed system-proxy mode")?;
            Ok(())
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => {
            bail!("vpn up is not implemented on this platform yet (adapter boundary is ready)")
        }
    }
}

pub fn down(spec: &VpnSpec) -> Result<()> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let services = list_services()?;
            if services.iter().any(|s| s == &spec.service_name) {
                run_cmd("scutil", &["--nc", "stop", &spec.service_name])?;
                return Ok(());
            }
            unset_system_proxy(&spec.network_service)
                .with_context(|| "fallback to OmniProxy managed system-proxy reset")?;
            Ok(())
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => {
            bail!("vpn down is not implemented on this platform yet (adapter boundary is ready)")
        }
    }
}

pub fn status(spec: &VpnSpec) -> Result<VpnStatus> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let services = list_services()?;
            if services.iter().any(|s| s == &spec.service_name) {
                return macos_status_service(spec);
            }
            macos_status_managed_proxy(spec)
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => Ok(VpnStatus {
            platform: platform_name(detect_platform()).to_string(),
            service_name: spec.service_name.clone(),
            connected: false,
            raw_status: "status not implemented on this platform yet".into(),
        }),
    }
}

pub fn list_services() -> Result<Vec<String>> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let raw = run_cmd_capture("scutil", &["--nc", "list"])?;
            Ok(parse_macos_nc_list(&raw))
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => Ok(Vec::new()),
    }
}

pub fn doctor(spec: &VpnSpec) -> Result<VpnDoctorReport> {
    match detect_platform() {
        PlatformKind::MacOs => {
            let services = list_services()?;
            let exists = services.iter().any(|s| s == &spec.service_name);
            let status_info = status(spec).ok();
            let connected = status_info.as_ref().is_some_and(|s| s.connected);

            let mut notes = vec![
                "macOS adapter supports two modes: scutil VPN service or OmniProxy-managed system-proxy".to_string(),
                "Use OmniProxy-managed mode for immediate self-hosted capture without third-party VPN names".to_string(),
            ];
            if exists {
                notes.push(format!(
                    "service `{}` exists and will be controlled via scutil --nc",
                    spec.service_name
                ));
            } else {
                notes.push(format!(
                    "service `{}` not found; fallback mode will set system HTTP/HTTPS proxy on `{}` to {}",
                    spec.service_name, spec.network_service, spec.local_http_proxy
                ));
            }

            Ok(VpnDoctorReport {
                platform: "macos".to_string(),
                adapter_ready: true,
                service_name: spec.service_name.clone(),
                service_exists: exists,
                connected,
                notes,
            })
        }
        PlatformKind::Linux | PlatformKind::Windows | PlatformKind::Other => {
            let platform = platform_name(detect_platform()).to_string();
            Ok(VpnDoctorReport {
                platform,
                adapter_ready: false,
                service_name: spec.service_name.clone(),
                service_exists: false,
                connected: false,
                notes: vec![
                    "Platform adapter is planned but not implemented yet".to_string(),
                    "Current stable path is explicit proxy / global system proxy mode".to_string(),
                ],
            })
        }
    }
}

fn macos_status_service(spec: &VpnSpec) -> Result<VpnStatus> {
    let out = Command::new("scutil")
        .args(["--nc", "status", &spec.service_name])
        .output()
        .with_context(|| format!("run scutil status {}", spec.service_name))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let raw = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    let connected = is_connected_status(&raw);
    Ok(VpnStatus {
        platform: "macos".into(),
        service_name: spec.service_name.clone(),
        connected,
        raw_status: format!("mode=scutil\n{}", raw.trim()),
    })
}

fn macos_status_managed_proxy(spec: &VpnSpec) -> Result<VpnStatus> {
    let web = run_cmd_capture("networksetup", &["-getwebproxy", &spec.network_service])
        .unwrap_or_else(|e| format!("error: {}", e));
    let secure = run_cmd_capture(
        "networksetup",
        &["-getsecurewebproxy", &spec.network_service],
    )
    .unwrap_or_else(|e| format!("error: {}", e));
    let enabled = web.contains("Enabled: Yes") && secure.contains("Enabled: Yes");
    let (expect_host, expect_port) = parse_host_port(&spec.local_http_proxy)?;
    let host_ok = web.contains(&format!("Server: {}", expect_host))
        && secure.contains(&format!("Server: {}", expect_host));
    let port_ok = web.contains(&format!("Port: {}", expect_port))
        && secure.contains(&format!("Port: {}", expect_port));
    let connected = enabled && host_ok && port_ok;

    Ok(VpnStatus {
        platform: "macos".into(),
        service_name: spec.service_name.clone(),
        connected,
        raw_status: format!(
            "mode=managed-system-proxy\nnetwork_service={}\nexpected_proxy={}\nweb_proxy=\n{}\nsecure_web_proxy=\n{}",
            spec.network_service,
            spec.local_http_proxy,
            web.trim(),
            secure.trim()
        ),
    })
}

fn parse_host_port(raw: &str) -> Result<(String, u16)> {
    if let Some((host, port)) = raw.rsplit_once(':') {
        let p = port
            .parse::<u16>()
            .with_context(|| format!("invalid port in {}", raw))?;
        let h = host.trim();
        if h.is_empty() {
            bail!("empty host in {}", raw);
        }
        return Ok((h.to_string(), p));
    }
    bail!("invalid host:port format: {}", raw)
}

fn is_connected_status(raw: &str) -> bool {
    raw.contains("Connected") || raw.contains("connecting") || raw.contains("Connecting")
}

fn parse_macos_nc_list(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(extract_double_quoted)
        .collect::<Vec<_>>()
}

fn extract_double_quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rem = &line[start + 1..];
    let end = rem.find('"')?;
    let value = rem[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("execute {} {}", cmd, args.join(" ")))?;
    if !status.success() {
        bail!("command failed: {} {}", cmd, args.join(" "));
    }
    Ok(())
}

fn run_cmd_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("execute {} {}", cmd, args.join(" ")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!(
            "command failed: {} {}: {}",
            cmd,
            args.join(" "),
            stderr.trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::{is_connected_status, parse_host_port, parse_macos_nc_list};

    #[test]
    fn parse_nc_list_extracts_names() {
        let raw = r#"Available network connection services in the current set (*=enabled):
* (Disconnected)   "OmniProxy VPN"   [PPP]
* (Connected)      "Corp VPN"        [IPSec]
"#;
        let names = parse_macos_nc_list(raw);
        assert_eq!(
            names,
            vec!["OmniProxy VPN".to_string(), "Corp VPN".to_string()]
        );
    }

    #[test]
    fn status_connected_parser() {
        assert!(is_connected_status("Connected"));
        assert!(is_connected_status("Connecting"));
        assert!(!is_connected_status("Disconnected"));
    }

    #[test]
    fn host_port_parser() {
        let got = parse_host_port("127.0.0.1:9090").expect("parse host:port");
        assert_eq!(got.0, "127.0.0.1");
        assert_eq!(got.1, 9090);
    }
}
