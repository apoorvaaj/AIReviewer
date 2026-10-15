//! THE ONLY MODULE THAT CAN OPEN A NETWORK CONNECTION.
//!
//! tests/audit.rs fails the build if any other source file mentions an HTTP
//! client or socket type. Everything above this layer talks to the `Transport`
//! trait, which lets tests substitute a recording mock and prove that no call
//! happens before the student confirms the payload.
//!
//! There is no telemetry, analytics, update check or crash reporting anywhere.

use std::time::Duration;

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait Transport: Send + Sync {
    /// POST a JSON body. `headers` never include document text.
    fn post_json(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<HttpResponse, String>;
}

/// Real HTTPS transport (rustls). HTTPS-only: plain http:// URLs are refused.
pub struct HttpsTransport {
    client: reqwest::blocking::Client,
}

impl HttpsTransport {
    pub fn new(timeout_secs: u64) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .https_only(true)
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent(concat!("ib-feedback-coach/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("could not create HTTPS client: {e}"))?;
        Ok(HttpsTransport { client })
    }
}

impl Transport for HttpsTransport {
    fn post_json(&self, url: &str, headers: &[(&str, String)], body: &str) -> Result<HttpResponse, String> {
        let mut req = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_owned());
        for (name, value) in headers {
            req = req.header(*name, value);
        }
        let resp = req.send().map_err(|e| format!("network error: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp.text().map_err(|e| format!("network error reading response: {e}"))?;
        Ok(HttpResponse { status, body })
    }
}
