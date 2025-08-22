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

    pub(crate) fn extract_from_forwarded_header(&self, headers: &HeaderMap) -> Option<Url> {
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

    pub(crate) fn extract_from_x_forwarded_headers(&self, headers: &HeaderMap) -> Option<Url> {
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

    pub(crate) fn extract_from_host_header(&self, headers: &HeaderMap) -> Option<Url> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn test_extract_from_forwarded_header_single_entry() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=https;host=example.com"),
        );

        let url = resolver.extract_from_forwarded_header(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_extract_from_forwarded_header_multiple_entries() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        // Multiple proxy entries - should use the first (leftmost) one
        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=https;host=original.com, proto=http;host=proxy1.com, proto=https;host=proxy2.com"),
        );

        let url = resolver.extract_from_forwarded_header(&headers).unwrap();
        assert_eq!(url.as_str(), "https://original.com/");
    }

    #[test]
    fn test_extract_from_forwarded_header_with_quotes() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=\"https\";host=\"example.com\""),
        );

        let url = resolver.extract_from_forwarded_header(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_extract_from_forwarded_header_missing_proto() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("forwarded", HeaderValue::from_static("host=example.com"));

        assert!(resolver.extract_from_forwarded_header(&headers).is_none());
    }

    #[test]
    fn test_extract_from_forwarded_header_missing_host() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("forwarded", HeaderValue::from_static("proto=https"));

        assert!(resolver.extract_from_forwarded_header(&headers).is_none());
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_basic() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));

        let url = resolver.extract_from_x_forwarded_headers(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_with_port() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("8443"));

        let url = resolver.extract_from_x_forwarded_headers(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com:8443/");
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_multiple_values() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        // Multiple values - should use the first (leftmost) one
        headers.insert(
            "x-forwarded-proto",
            HeaderValue::from_static("https, http, https"),
        );
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("original.com, proxy1.com, proxy2.com"),
        );
        headers.insert(
            "x-forwarded-port",
            HeaderValue::from_static("443, 80, 8080"),
        );

        let url = resolver.extract_from_x_forwarded_headers(&headers).unwrap();
        // Port 443 is the default for https, so it gets normalized away
        assert_eq!(url.as_str(), "https://original.com/");
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_default_proto() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        // No x-forwarded-proto header - should default to http
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));

        let url = resolver.extract_from_x_forwarded_headers(&headers).unwrap();
        assert_eq!(url.as_str(), "http://example.com/");
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_missing_host() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));

        assert!(resolver
            .extract_from_x_forwarded_headers(&headers)
            .is_none());
    }

    #[test]
    fn test_extract_from_host_header_basic() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("example.com"));

        let url = resolver.extract_from_host_header(&headers).unwrap();
        assert_eq!(url.as_str(), "http://example.com/");
    }

    #[test]
    fn test_extract_from_host_header_with_port() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("example.com:8080"));

        let url = resolver.extract_from_host_header(&headers).unwrap();
        assert_eq!(url.as_str(), "http://example.com:8080/");
    }

    #[test]
    fn test_extract_from_host_header_https_port_443() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("example.com:443"));

        let url = resolver.extract_from_host_header(&headers).unwrap();
        // Port 443 is the default for https, so it gets normalized away
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_extract_from_host_header_with_x_forwarded_proto_hint() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));

        let url = resolver.extract_from_host_header(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_extract_from_host_header_missing() {
        let resolver = ServerUrlResolver::new(None);
        let headers = HeaderMap::new();

        assert!(resolver.extract_from_host_header(&headers).is_none());
    }

    #[test]
    fn test_resolve_with_fixed_url() {
        let fixed_url = Url::parse("https://fixed.example.com").unwrap();
        let resolver = ServerUrlResolver::new(Some(fixed_url.clone()));
        let mut headers = HeaderMap::new();

        // Add various headers that would normally be used
        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=http;host=forwarded.com"),
        );
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("xforwarded.com"),
        );
        headers.insert("host", HeaderValue::from_static("host.com"));

        // Fixed URL should always take precedence
        let url = resolver.resolve(&headers).unwrap();
        assert_eq!(url, fixed_url);
    }

    #[test]
    fn test_resolve_priority_forwarded_over_x_forwarded() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=https;host=forwarded.com"),
        );
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("xforwarded.com"),
        );
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        headers.insert("host", HeaderValue::from_static("host.com"));

        // Forwarded header should take precedence
        let url = resolver.resolve(&headers).unwrap();
        assert_eq!(url.as_str(), "https://forwarded.com/");
    }

    #[test]
    fn test_resolve_priority_x_forwarded_over_host() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("xforwarded.com"),
        );
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("host", HeaderValue::from_static("host.com"));

        // X-Forwarded headers should take precedence over Host
        let url = resolver.resolve(&headers).unwrap();
        assert_eq!(url.as_str(), "https://xforwarded.com/");
    }

    #[test]
    fn test_resolve_fallback_to_host() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("host.com"));

        // Should fall back to Host header
        let url = resolver.resolve(&headers).unwrap();
        assert_eq!(url.as_str(), "http://host.com/");
    }

    #[test]
    fn test_resolve_no_headers_returns_error() {
        let resolver = ServerUrlResolver::new(None);
        let headers = HeaderMap::new();

        let result = resolver.resolve(&headers);
        assert!(matches!(result, Err(ServerUrlError::UnableToResolve)));
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_with_spaces() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        // Test with spaces around values
        headers.insert(
            "x-forwarded-proto",
            HeaderValue::from_static(" https , http"),
        );
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static(" example.com , other.com"),
        );
        headers.insert("x-forwarded-port", HeaderValue::from_static(" 8443 , 80"));

        let url = resolver.extract_from_x_forwarded_headers(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com:8443/");
    }

    #[test]
    fn test_extract_from_host_header_ipv4() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("192.168.1.1:8080"));

        let url = resolver.extract_from_host_header(&headers).unwrap();
        assert_eq!(url.as_str(), "http://192.168.1.1:8080/");
    }

    #[test]
    fn test_extract_from_host_header_ipv6() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("host", HeaderValue::from_static("[::1]:8080"));

        let url = resolver.extract_from_host_header(&headers).unwrap();
        assert_eq!(url.as_str(), "http://[::1]:8080/");
    }

    #[test]
    fn test_extract_from_forwarded_header_with_port() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=https;host=example.com:8443"),
        );

        let url = resolver.extract_from_forwarded_header(&headers).unwrap();
        assert_eq!(url.as_str(), "https://example.com:8443/");
    }

    #[test]
    fn test_extract_from_x_forwarded_headers_http_port_80() {
        let resolver = ServerUrlResolver::new(None);
        let mut headers = HeaderMap::new();

        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("80"));

        let url = resolver.extract_from_x_forwarded_headers(&headers).unwrap();
        // Port 80 is the default for http, so it gets normalized away
        assert_eq!(url.as_str(), "http://example.com/");
    }
}
