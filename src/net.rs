//! Hex's own HTTPS: model downloads, update checks, and transformation
//! scripts' `fetch`, through one blocking client (ureq) that works on managed
//! networks (#894).
//!
//! - TLS through the platform (`native-tls`: SChannel, Security.framework,
//!   or OpenSSL) and its trusted roots, so a company's own root certificate,
//!   as TLS-inspecting firewalls need, is trusted like it is in a browser.
//! - The proxy the computer uses (`proxy`): `HTTPS_PROXY`, `HTTP_PROXY`,
//!   `ALL_PROXY`, and `NO_PROXY` on every OS, then Windows' proxy settings
//!   (including a PAC file or WPAD) and macOS' network proxy settings.
//! - Failures named for a person: "Couldn't reach huggingface.co: …".

mod download;
mod proxy;

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub use download::{Download, download};
pub use proxy::proxy_for;

/// What a caller adds to [`agent`]'s settings.
pub type Config = ureq::config::ConfigBuilder<ureq::typestate::AgentScope>;

/// An agent for requests to `url`: platform TLS and roots, and the proxy the
/// computer uses for it.
pub fn agent(url: &str, configure: impl FnOnce(Config) -> Config) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .proxy(proxy_for(url))
        .timeout_connect(Some(Duration::from_secs(15)));
    configure(config).build().into()
}

/// The host of `url`, for messages and logs (never the path or query).
pub fn host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "the server".into())
}

/// Why a request failed, in a person's words, without the URL.
pub fn reason(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(407) => "the proxy needs a sign-in Hex can't give".into(),
        ureq::Error::StatusCode(code) => format!("the server answered HTTP {code}"),
        ureq::Error::HostNotFound => "the name couldn't be looked up (DNS)".into(),
        ureq::Error::ConnectProxyFailed(detail) => format!("the proxy refused ({detail})"),
        ureq::Error::Tls(detail) => format!("certificate or TLS error ({detail})"),
        ureq::Error::NativeTls(detail) => format!("certificate or TLS error ({detail})"),
        ureq::Error::Timeout(_) => "the connection timed out".into(),
        ureq::Error::ConnectionFailed => "the connection failed".into(),
        ureq::Error::Io(detail) => format!("the connection failed ({detail})"),
        error => error.to_string(),
    }
}

/// "Couldn't reach `host`: `reason`", naming the proxy when one was used.
pub fn unreachable(url: &str, error: &ureq::Error) -> String {
    let via = proxy_for(url)
        .map(|proxy| format!(" through the proxy {}:{}", proxy.host(), proxy.port()))
        .unwrap_or_default();
    format!("Couldn't reach {}{via}: {}", host(url), reason(error))
}
