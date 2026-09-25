//! Plain HTTP GETs for the catalog and artwork: a timeout, a size cap and
//! conditional requests. No login, no cookies, nothing about the user sent.

use std::time::Duration;

pub enum Fetched {
    /// The server says our copy is current (`304`).
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
    /// The server doesn't have it (`404`, `410`).
    Missing,
}

pub fn get(url: &str, etag: Option<&str>, limit: u64, timeout: Duration) -> Result<Fetched, String> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into();
    let mut request = agent.get(url).header("User-Agent", concat!("SaveScummer/", env!("CARGO_PKG_VERSION")));
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    let mut response = request.call().map_err(|e| format!("{url}: {e}"))?;
    match response.status().as_u16() {
        304 => Ok(Fetched::NotModified),
        404 | 410 => Ok(Fetched::Missing),
        200 => {
            let etag = response.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
            let bytes =
                response.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("{url}: {e}"))?;
            Ok(Fetched::Body { bytes, etag })
        }
        status => Err(format!("{url}: HTTP {status}")),
    }
}
