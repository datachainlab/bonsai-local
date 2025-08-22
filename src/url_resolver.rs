use axum::http::HeaderMap;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone)]
pub struct ServerUrlResolver {
    fixed_url: Option<Url>,
}

impl ServerUrlResolver {
    pub fn new(fixed_url: Option<Url>) -> Self {
        Self { fixed_url }
    }

    /// Resolves the server URL based on the following priority order:
    /// 1. Fixed URL (if provided via --server_url option) - always takes precedence
    /// 2. Forwarded header (RFC 7239) - parses "proto" and "host" directives from the FIRST entry
    /// 3. X-Forwarded-* headers - uses FIRST values from X-Forwarded-Proto, X-Forwarded-Host, and optionally X-Forwarded-Port
    /// 4. Host header - direct connection fallback, infers HTTPS for port 443, otherwise defaults to HTTP
    ///
    /// When multiple proxy entries exist (comma-separated), we use the FIRST (leftmost) values
    /// as they represent the original client request URL.
    ///
    /// Returns ServerUrlError::UnableToResolve if no URL can be determined from any source.
    pub fn resolve(&self, headers: &HeaderMap) -> Result<Url, ServerUrlError> {
        if let Some(ref url) = self.fixed_url {
            return Ok(url.clone());
        }

        if let Some(url) = self.extract_from_forwarded_header(headers) {
            return Ok(url);
        }

        if let Some(url) = self.extract_from_x_forwarded_headers(headers) {
            return Ok(url);
        }

        if let Some(url) = self.extract_from_host_header(headers) {
            return Ok(url);
        }

        Err(ServerUrlError::UnableToResolve)
    }

    fn extract_from_forwarded_header(&self, headers: &HeaderMap) -> Option<Url> {
        headers.get("forwarded").and_then(|value| {
            value.to_str().ok().and_then(|s| {
                // RFC 7239: Each proxy appends its own entry, separated by commas
                // Example: "proto=https;host=original.com, proto=http;host=proxy1.com, proto=https;host=proxy2.com"
                //          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                //          Client's original request (what we want to extract)

                // Split by comma to get individual proxy entries and take the first one
                if let Some(first_entry) = s.split(',').next() {
                    let mut proto = None;
                    let mut host = None;

                    // Parse each directive in the first entry
                    for directive in first_entry.split(';') {
                        let trimmed = directive.trim();

                        if let Some(p) = trimmed.strip_prefix("proto=") {
                            proto = Some(p.trim_matches('"'));
                        } else if let Some(h) = trimmed.strip_prefix("host=") {
                            host = Some(h.trim_matches('"'));
                        }
                    }

                    // Only build URL if we have both proto and host from the same proxy entry
                    // This ensures consistency - both values come from the same proxy
                    if let (Some(proto), Some(host)) = (proto, host) {
                        return Url::parse(&format!("{}://{}", proto, host)).ok();
                    }
                }
                None
            })
        })
    }

    fn extract_from_x_forwarded_headers(&self, headers: &HeaderMap) -> Option<Url> {
        // X-Forwarded-* headers: Each proxy appends its value, creating comma-separated lists
        // Example: X-Forwarded-Host: "original.com, proxy1.com, proxy2.com"
        //                             ^^^^^^^^^^^^
        //                             Client's original host (what we extract)
        // We take the FIRST value (leftmost) from each header

        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim())
            .unwrap_or("http");

        let host = headers
            .get("x-forwarded-host")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim())?;

        let port = headers
            .get("x-forwarded-port")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim());

        let url_string = if let Some(port) = port {
            format!("{}://{}:{}", proto, host, port)
        } else {
            format!("{}://{}", proto, host)
        };

        Url::parse(&url_string).ok()
    }

    fn extract_from_host_header(&self, headers: &HeaderMap) -> Option<Url> {
        headers.get("host").and_then(|value| {
            value.to_str().ok().and_then(|host| {
                // Infer scheme for direct connections (no proxy headers):
                // - Port 443 implies HTTPS
                // - Check X-Forwarded-Proto as a hint (though this is unusual for direct connections)
                // - Default to HTTP for all other cases
                let scheme = if host.ends_with(":443") {
                    "https"
                } else if headers
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.split(',').next())
                    .map(|s| s.trim())
                    == Some("https")
                {
                    "https"
                } else {
                    "http"
                };

                Url::parse(&format!("{}://{}", scheme, host)).ok()
            })
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerUrlError {
    #[error("Unable to resolve server URL from headers")]
    UnableToResolve,
}

pub type SharedUrlResolver = Arc<ServerUrlResolver>;
