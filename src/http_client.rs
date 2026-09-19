#![forbid(unsafe_code)]

//! A small HTTP client for talking to the worker on localhost.
//!
//! The worker is a local, plaintext HTTP service, so this speaks HTTP/1.0 over
//! a TCP socket and reads the body to end-of-stream. That avoids both a TLS
//! stack and chunked-transfer handling, and keeps the CLI free of a runtime it
//! would otherwise pull in for a handful of requests.
//!
//! It is deliberately not a general-purpose client: anything that needs TLS
//! (GitHub, Cloudflare) is delegated to `gh` and `cloudflared`, which already
//! hold the right credentials.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::CliError;

const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Split `http://host:port` into a socket address.
pub fn authority_of(base: &str) -> Result<String, CliError> {
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| CliError::Config(format!("only http:// bases are supported, got {base}")))?;
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(CliError::Config(format!("no host in base URL {base}")));
    }
    if authority.contains(':') {
        Ok(authority.to_string())
    } else {
        Ok(format!("{authority}:80"))
    }
}

/// Split a raw response into its status code and body.
pub fn parse_response(raw: &[u8]) -> Result<Response, CliError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .ok_or_else(|| CliError::Command("response had no header terminator".into()))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| CliError::Command("response had no status line".into()))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| CliError::Command(format!("unparsable status line: {status_line}")))?;
    Ok(Response {
        status,
        body: body.to_string(),
    })
}

/// Build the request bytes. Separated from the socket so it can be tested.
pub fn request_bytes(
    method: &str,
    path: &str,
    authority: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Vec<u8> {
    let mut request = format!("{method} {path} HTTP/1.0\r\nHost: {authority}\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        request.push_str("content-type: application/json\r\n");
        request.push_str(&format!("content-length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    let mut bytes = request.into_bytes();
    if let Some(body) = body {
        bytes.extend_from_slice(body.as_bytes());
    }
    bytes
}

pub fn request(
    base: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Result<Response, CliError> {
    let authority = authority_of(base)?;
    let mut stream = TcpStream::connect(&authority).map_err(|error| {
        CliError::Command(format!(
            "cannot reach the worker at {authority}: {error}. Is it running? Try: giw worker up"
        ))
    })?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();
    stream
        .write_all(&request_bytes(method, path, &authority, headers, body))
        .map_err(|error| CliError::Command(format!("failed to send request: {error}")))?;
    stream.flush().ok();
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| CliError::Command(format!("failed to read response: {error}")))?;
    parse_response(&raw)
}

/// Pull one string field out of a JSON object without a full parse.
pub fn json_field(body: &str, field: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get(field).map(|found| match found {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_defaults_to_port_80_but_keeps_an_explicit_one() {
        assert_eq!(
            authority_of("http://127.0.0.1:18095").unwrap(),
            "127.0.0.1:18095"
        );
        assert_eq!(
            authority_of("http://example.test").unwrap(),
            "example.test:80"
        );
        assert_eq!(
            authority_of("http://127.0.0.1:18095/builds").unwrap(),
            "127.0.0.1:18095"
        );
        // https would silently send plaintext to a TLS port, so refuse it.
        assert!(authority_of("https://example.test").is_err());
    }

    #[test]
    fn responses_split_on_the_header_terminator() {
        let raw =
            b"HTTP/1.1 202 Accepted\r\ncontent-type: application/json\r\n\r\n{\"id\":\"build-1\"}";
        let response = parse_response(raw).expect("parses");
        assert_eq!(response.status, 202);
        assert_eq!(response.body, "{\"id\":\"build-1\"}");
        assert!(response.is_success());
    }

    #[test]
    fn a_body_containing_blank_lines_survives() {
        let raw = b"HTTP/1.1 200 OK\r\n\r\nfirst\n\nsecond";
        let response = parse_response(raw).expect("parses");
        // Only the first terminator separates head from body; log output that
        // contains blank lines must not be truncated.
        assert_eq!(response.body, "first\n\nsecond");
    }

    #[test]
    fn failure_statuses_are_reported_not_hidden() {
        let raw = b"HTTP/1.1 401 Unauthorized\r\n\r\n{\"error\":\"unauthorized\"}";
        let response = parse_response(raw).expect("parses");
        assert_eq!(response.status, 401);
        assert!(!response.is_success());
    }

    #[test]
    fn requests_carry_the_auth_header_and_a_content_length() {
        let bytes = request_bytes(
            "POST",
            "/builds",
            "127.0.0.1:18095",
            &[("x-server-auth", "secret")],
            Some("{\"a\":1}"),
        );
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.starts_with("POST /builds HTTP/1.0\r\n"));
        assert!(text.contains("Host: 127.0.0.1:18095\r\n"));
        assert!(text.contains("x-server-auth: secret\r\n"));
        assert!(text.contains("content-length: 7\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"a\":1}"));
    }

    #[test]
    fn a_get_has_no_body_headers() {
        let text = String::from_utf8(request_bytes(
            "GET",
            "/healthz",
            "127.0.0.1:18095",
            &[],
            None,
        ))
        .expect("utf8");
        assert!(!text.contains("content-length"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn json_field_reads_strings_and_numbers() {
        assert_eq!(
            json_field("{\"id\":\"build-1\",\"n\":3}", "id"),
            Some("build-1".to_string())
        );
        assert_eq!(json_field("{\"n\":3}", "n"), Some("3".to_string()));
        assert_eq!(json_field("{}", "missing"), None);
        assert_eq!(json_field("not json", "id"), None);
    }
}
