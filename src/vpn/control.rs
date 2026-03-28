use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnSpec {
    pub service_name: String,
    pub local_socks5: String,
    pub local_http_proxy: String,
    pub local_dns: String,
}

impl Default for VpnSpec {
    fn default() -> Self {
        Self {
            service_name: "OmniProxy VPN".to_string(),
            local_socks5: "127.0.0.1:1080".to_string(),
            local_http_proxy: "127.0.0.1:9090".to_string(),
            local_dns: "127.0.0.1:5353".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnStatus {
    pub platform: String,
    pub service_name: String,
    pub connected: bool,
    pub raw_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnDoctorReport {
    pub platform: String,
    pub adapter_ready: bool,
    pub service_name: String,
    pub service_exists: bool,
    pub connected: bool,
    pub notes: Vec<String>,
}
