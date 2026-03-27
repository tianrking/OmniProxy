use anyhow::{Context, Result};
use hudsucker::rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::path::Path;
use tokio::fs;

pub async fn load_or_init_issuer(
    cert_path: &Path,
    key_path: &Path,
) -> Result<Issuer<'static, KeyPair>> {
    if cert_path.exists() && key_path.exists() {
        let cert = fs::read_to_string(cert_path)
            .await
            .with_context(|| format!("read cert {}", cert_path.display()))?;
        let key = fs::read_to_string(key_path)
            .await
            .with_context(|| format!("read key {}", key_path.display()))?;

        let key_pair = KeyPair::from_pem(&key).context("parse CA private key pem")?;
        return Issuer::from_ca_cert_pem(&cert, key_pair).context("parse CA certificate pem");
    }

    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create cert dir {}", parent.display()))?;
    }
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create key dir {}", parent.display()))?;
    }

    let (cert_pem, key_pem) = generate_ca_pair()?;

    fs::write(cert_path, cert_pem.as_bytes())
        .await
        .with_context(|| format!("write cert {}", cert_path.display()))?;
    fs::write(key_path, key_pem.as_bytes())
        .await
        .with_context(|| format!("write key {}", key_path.display()))?;

    let key_pair = KeyPair::from_pem(&key_pem).context("re-parse generated CA private key")?;
    Issuer::from_ca_cert_pem(&cert_pem, key_pair).context("re-parse generated CA cert")
}

fn generate_ca_pair() -> Result<(String, String)> {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::CrlSign,
    ];

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "OmniProxy Local CA");
    dn.push(DnType::OrganizationName, "OmniProxy");
    params.distinguished_name = dn;

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    Ok((cert.pem(), key_pair.serialize_pem()))
}
