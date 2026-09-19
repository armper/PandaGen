//! Minimal server-side HTTP/1.1: request parsing and response headers.
//!
//! No allocation. The caller owns every buffer, so this runs unchanged in
//! the kernel. The parser is deliberately strict about framing (the part
//! clients actually depend on) and deliberately incurious about headers it
//! does not need.

/// Longest request head this server will accept before answering 431.
pub const MAX_HEAD_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    /// Any other token: a well-formed request this server will not serve.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request<'a> {
    pub method: Method,
    /// Request target, with any query string still attached.
    pub target: &'a str,
    /// Minor version of HTTP/1.x. A 1.0 client gets an explicit close.
    pub minor_version: u8,
    /// Bytes occupied by the request head, including the blank line.
    pub head_len: usize,
    /// True when the client asked to keep the connection open.
    pub keep_alive: bool,
}

impl Request<'_> {
    /// Path with the query string removed.
    pub fn path(&self) -> &str {
        match self.target.split_once('?') {
            Some((path, _)) => path,
            None => self.target,
        }
    }

    /// Query string, if any.
    pub fn query(&self) -> Option<&str> {
        self.target.split_once('?').map(|(_, q)| q)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parse<'a> {
    /// The head is not terminated yet; read more bytes.
    Incomplete,
    Complete(Request<'a>),
    /// Unparseable: answer 400 and close.
    Malformed,
}

/// Find the end of the request head (the blank line after the headers).
fn head_end(buf: &[u8]) -> Option<usize> {
    // Accept bare LF line endings as well as CRLF; some clients and most
    // hand-typed requests use them, and Linux servers accept both.
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' {
            if i + 1 < buf.len() && buf[i + 1] == b'\n' {
                return Some(i + 2);
            }
            if i + 2 < buf.len() && buf[i + 1] == b'\r' && buf[i + 2] == b'\n' {
                return Some(i + 3);
            }
        }
        i += 1;
    }
    None
}

/// Parse a request head. Returns `Incomplete` until the blank line arrives.
pub fn parse(buf: &[u8]) -> Parse<'_> {
    let Some(head_len) = head_end(buf) else {
        return Parse::Incomplete;
    };
    let head = &buf[..head_len];
    let Ok(text) = core::str::from_utf8(head) else {
        return Parse::Malformed;
    };
    let mut lines = text.split('\n');
    let Some(request_line) = lines.next() else {
        return Parse::Malformed;
    };
    let request_line = request_line.trim_end_matches('\r');
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Parse::Malformed;
    };
    if parts.next().is_some() || target.is_empty() {
        return Parse::Malformed;
    }
    let minor_version = match version {
        "HTTP/1.1" => 1,
        "HTTP/1.0" => 0,
        _ => return Parse::Malformed,
    };
    let method = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "" => return Parse::Malformed,
        _ => Method::Other,
    };
    // Connection defaults to keep-alive on 1.1 and close on 1.0.
    let mut keep_alive = minor_version >= 1;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Parse::Malformed;
        };
        if name.eq_ignore_ascii_case("connection") {
            let value = value.trim();
            if value.eq_ignore_ascii_case("close") {
                keep_alive = false;
            } else if value.eq_ignore_ascii_case("keep-alive") {
                keep_alive = true;
            }
        }
    }
    Parse::Complete(Request {
        method,
        target,
        minor_version,
        head_len,
        keep_alive,
    })
}

/// Write a decimal number; returns the bytes used.
fn write_usize(out: &mut [u8], mut value: usize) -> Option<usize> {
    let mut digits = [0u8; 20];
    let mut len = 0;
    loop {
        digits[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    if out.len() < len {
        return None;
    }
    for (i, slot) in out.iter_mut().take(len).enumerate() {
        *slot = digits[len - 1 - i];
    }
    Some(len)
}

/// Reason phrase for the statuses this server produces.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        _ => "Unknown",
    }
}

/// Write response headers for a body of `content_length` bytes. The body is
/// sent separately, so a large one can be streamed.
pub fn write_headers(
    out: &mut [u8],
    status: u16,
    content_type: &str,
    content_length: usize,
    keep_alive: bool,
) -> Option<usize> {
    let mut n = 0;
    let mut put = |bytes: &[u8], n: &mut usize| -> Option<()> {
        if *n + bytes.len() > out.len() {
            return None;
        }
        out[*n..*n + bytes.len()].copy_from_slice(bytes);
        *n += bytes.len();
        Some(())
    };
    put(b"HTTP/1.1 ", &mut n)?;
    let mut status_buf = [0u8; 8];
    let len = write_usize(&mut status_buf, status as usize)?;
    put(&status_buf[..len], &mut n)?;
    put(b" ", &mut n)?;
    put(reason(status).as_bytes(), &mut n)?;
    put(b"\r\nContent-Type: ", &mut n)?;
    put(content_type.as_bytes(), &mut n)?;
    put(b"\r\nContent-Length: ", &mut n)?;
    let mut len_buf = [0u8; 20];
    let len = write_usize(&mut len_buf, content_length)?;
    put(&len_buf[..len], &mut n)?;
    put(b"\r\nServer: PandaGen\r\nConnection: ", &mut n)?;
    put(if keep_alive { b"keep-alive" } else { b"close" }, &mut n)?;
    put(b"\r\n\r\n", &mut n)?;
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_curl_request() {
        let raw = b"GET /status?q=1 HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nUser-Agent: curl/8.7.1\r\nAccept: */*\r\n\r\n";
        match parse(raw) {
            Parse::Complete(request) => {
                assert_eq!(request.method, Method::Get);
                assert_eq!(request.target, "/status?q=1");
                assert_eq!(request.path(), "/status");
                assert_eq!(request.query(), Some("q=1"));
                assert_eq!(request.minor_version, 1);
                assert_eq!(request.head_len, raw.len());
                assert!(request.keep_alive, "HTTP/1.1 defaults to keep-alive");
            }
            other => panic!("expected complete, got {other:?}"),
        }
    }

    #[test]
    fn waits_for_the_blank_line() {
        let full = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        for split in 0..full.len() {
            let partial = &full[..split];
            assert_eq!(
                parse(partial),
                Parse::Incomplete,
                "byte prefix of length {split} must not parse as complete"
            );
        }
        assert!(matches!(parse(full), Parse::Complete(_)));
    }

    #[test]
    fn head_len_stops_at_the_blank_line_even_with_a_body() {
        let raw = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nBODYBYTES";
        match parse(raw) {
            Parse::Complete(request) => {
                assert_eq!(request.head_len, raw.len() - b"BODYBYTES".len());
            }
            other => panic!("expected complete, got {other:?}"),
        }
    }

    #[test]
    fn accepts_bare_lf_and_http_1_0() {
        match parse(b"GET /x HTTP/1.0\nHost: y\n\n") {
            Parse::Complete(request) => {
                assert_eq!(request.minor_version, 0);
                assert_eq!(request.target, "/x");
                assert!(!request.keep_alive, "HTTP/1.0 defaults to close");
            }
            other => panic!("expected complete, got {other:?}"),
        }
    }

    #[test]
    fn connection_header_overrides_the_version_default() {
        let close_11 = b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n";
        let keep_10 = b"GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n";
        match (parse(close_11), parse(keep_10)) {
            (Parse::Complete(a), Parse::Complete(b)) => {
                assert!(!a.keep_alive);
                assert!(b.keep_alive, "matching is case-insensitive");
            }
            other => panic!("expected two complete parses, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_request_lines() {
        for raw in [
            &b"GET\r\n\r\n"[..],
            &b"GET /\r\n\r\n"[..],
            &b"GET / HTTP/2.0\r\n\r\n"[..],
            &b"GET / HTTP/1.1 extra\r\n\r\n"[..],
            &b"GET  HTTP/1.1\r\n\r\n"[..],
            &b"GET / HTTP/1.1\r\nnot-a-header\r\n\r\n"[..],
            &[0xFF, 0xFE, b'\r', b'\n', b'\r', b'\n'][..],
        ] {
            assert_eq!(parse(raw), Parse::Malformed, "should reject {raw:?}");
        }
    }

    #[test]
    fn unknown_methods_parse_but_are_marked_other() {
        match parse(b"POST / HTTP/1.1\r\n\r\n") {
            Parse::Complete(request) => assert_eq!(request.method, Method::Other),
            other => panic!("expected complete, got {other:?}"),
        }
    }

    #[test]
    fn headers_are_well_formed_and_report_the_exact_length() {
        let mut out = [0u8; 256];
        let n = write_headers(&mut out, 200, "text/plain", 12345, false).unwrap();
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Type: text/plain\r\n"));
        assert!(text.contains("Content-Length: 12345\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
        assert_eq!(
            text.matches("\r\n\r\n").count(),
            1,
            "exactly one blank line"
        );

        let n = write_headers(&mut out, 404, "text/html", 0, true).unwrap();
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(text.contains("Content-Length: 0\r\n"));
        assert!(text.contains("Connection: keep-alive\r\n"));
    }

    #[test]
    fn header_writing_reports_a_buffer_that_is_too_small() {
        let mut tiny = [0u8; 8];
        assert_eq!(write_headers(&mut tiny, 200, "text/plain", 1, false), None);
    }

    #[test]
    fn decimal_conversion_covers_edges() {
        let mut out = [0u8; 24];
        for value in [0usize, 1, 9, 10, 99, 100, 65535, usize::MAX] {
            let n = write_usize(&mut out, value).unwrap();
            let text = core::str::from_utf8(&out[..n]).unwrap();
            assert_eq!(text.parse::<usize>().unwrap(), value);
        }
    }
}
