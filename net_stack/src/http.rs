//! Minimal HTTP/1.1: the server's request parsing and response headers,
//! and the client's URL, request, response head and chunked body (NET-032).
//!
//! No allocation. The caller owns every buffer, so this runs unchanged in
//! the kernel. The parser is deliberately strict about framing (the part
//! clients actually depend on) and deliberately incurious about headers it
//! does not need.

/// Longest request head this server will accept before answering 431.
pub const MAX_HEAD_BYTES: usize = 2048;

// The 431 path can only fire when the whole head fits in the receive buffer:
// the caller sees `Incomplete` with `buffered.len() >= MAX_HEAD_BYTES`, and
// `buffered` is that buffer. Raise this above `tcp::BUFFER_BYTES` and the
// branch becomes unreachable -- an oversized head fills the buffer, the
// advertised window goes to zero, nothing is ever drained, and the
// connection wedges with no response at all until the reaper.
const _: () = assert!(MAX_HEAD_BYTES <= crate::tcp::BUFFER_BYTES);

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
    /// Bytes of body the client says follow the head.
    ///
    /// This was not parsed at all, and the caller drained exactly `head_len`
    /// -- so a declared body stayed in the receive buffer and was parsed as
    /// the next request on a keep-alive connection. One request produced two
    /// responses, and a partial body fused with the next real request into
    /// something that parsed as a different method entirely. Behind any
    /// proxy that is a textbook request smuggle.
    pub body_len: usize,
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
    /// Framing this server does not implement: answer 501 and close.
    ///
    /// `Transfer-Encoding` was ignored entirely, so a chunked request's
    /// framing bytes were read as the next request. Refusing is the only
    /// honest answer from a server that does not decode it.
    Unsupported,
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
    // RFC 9112 2.2: a server should ignore at least one empty line before
    // the request line. Clients and proxies emit a stray CRLF after a body,
    // and answering 400 to it is a needless failure.
    let mut request_line = "";
    for line in lines.by_ref() {
        request_line = line.trim_end_matches('\r');
        if !request_line.is_empty() {
            break;
        }
    }
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
    let mut body_len = 0usize;
    let mut seen_length = false;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Parse::Malformed;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("connection") {
            if value.eq_ignore_ascii_case("close") {
                keep_alive = false;
            } else if value.eq_ignore_ascii_case("keep-alive") {
                keep_alive = true;
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            let Ok(declared) = value.parse::<usize>() else {
                return Parse::Malformed;
            };
            // Two different lengths is the classic smuggling primitive.
            if seen_length && declared != body_len {
                return Parse::Malformed;
            }
            seen_length = true;
            body_len = declared;
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            // Not decoded here, so it cannot be accepted. A request that
            // carries both is a smuggling attempt by construction.
            return Parse::Unsupported;
        }
    }
    Parse::Complete(Request {
        method,
        target,
        minor_version,
        head_len,
        keep_alive,
        body_len,
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

// ---- The client side (NET-032): a request out, a response read back. ----

/// An `http://` URL's parts. `https` is refused: there is no TLS here, and
/// quietly speaking plain HTTP to a TLS port would only ever fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Url<'a> {
    pub host: &'a str,
    pub port: u16,
    /// Path and query, starting with `/`.
    pub path: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlError {
    /// `https://` or another scheme.
    NotHttp,
    /// No host, a bad port, or characters that do not belong.
    Malformed,
}

/// Parse `http://host[:port][/path]`; a bare `host/path` is taken as
/// `http://` too, since that is how people type it.
pub fn parse_url(url: &str) -> Result<Url<'_>, UrlError> {
    let rest = match url.split_once("://") {
        Some(("http", rest)) | Some(("HTTP", rest)) => rest,
        Some(_) => return Err(UrlError::NotHttp),
        None => url,
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(at) if rest.as_bytes()[at] == b'/' => (&rest[..at], &rest[at..]),
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    // A path that starts with `?` still needs its slash; the caller writes
    // what we return, so the one case is handled where it is written.
    if authority.contains('@') {
        // user:pass@host -- not something to send in the clear by accident.
        return Err(UrlError::Malformed);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| UrlError::Malformed)?),
        None => (authority, 80),
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !host_ok || port == 0 || path.bytes().any(|b| b <= b' ' || b == 0x7F) {
        return Err(UrlError::Malformed);
    }
    Ok(Url { host, port, path })
}

/// Write the GET request for `url` into `out`; returns its length. The
/// connection is asked to close after the response, which is how the
/// response's end is known when it declares no length.
pub fn write_request(url: &Url<'_>, out: &mut [u8]) -> Option<usize> {
    let mut w = Writer { out, len: 0 };
    w.put(b"GET ")?;
    if url.path.starts_with('?') {
        w.put(b"/")?;
    }
    w.put(url.path.as_bytes())?;
    w.put(b" HTTP/1.1\r\nHost: ")?;
    w.put(url.host.as_bytes())?;
    if url.port != 80 {
        w.put(b":")?;
        let mut digits = [0u8; 5];
        let mut n = url.port;
        let mut i = digits.len();
        while n > 0 {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        w.put(&digits[i..])?;
    }
    w.put(b"\r\nUser-Agent: PandaGen\r\nAccept: */*\r\nConnection: close\r\n\r\n")?;
    Some(w.len)
}

struct Writer<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.len.checked_add(bytes.len())?;
        self.out.get_mut(self.len..end)?.copy_from_slice(bytes);
        self.len = end;
        Some(())
    }
}

/// A response's head, as far as a client needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response<'a> {
    pub status: u16,
    pub reason: &'a str,
    /// Bytes of head, including the blank line.
    pub head_len: usize,
    /// The declared body length, if any.
    pub content_length: Option<usize>,
    /// The body is chunked: decode it with `dechunk`.
    pub chunked: bool,
    /// Where a redirect points, if one does.
    pub location: Option<&'a str>,
    /// What the body is (`text/html; charset=utf-8`), if said.
    pub content_type: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseParse<'a> {
    Incomplete,
    Complete(Response<'a>),
    Malformed,
}

/// Parse a response head from the start of `buf`.
pub fn parse_response(buf: &[u8]) -> ResponseParse<'_> {
    let Some(head_len) = head_end(buf) else {
        return ResponseParse::Incomplete;
    };
    let Ok(text) = core::str::from_utf8(&buf[..head_len]) else {
        return ResponseParse::Malformed;
    };
    let mut lines = text.split('\n').map(|l| l.trim_end_matches('\r'));
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    let status = parts.next().and_then(|s| s.parse::<u16>().ok());
    let reason = parts.next().unwrap_or("");
    let Some(status) = status.filter(|s| (100..=999).contains(s)) else {
        return ResponseParse::Malformed;
    };
    if !version.starts_with("HTTP/1.") {
        return ResponseParse::Malformed;
    }
    let mut response = Response {
        status,
        reason,
        head_len,
        content_length: None,
        chunked: false,
        location: None,
        content_type: None,
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            match value.parse::<usize>() {
                Ok(n) => response.content_length = Some(n),
                Err(_) => return ResponseParse::Malformed,
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            response.chunked = value
                .rsplit(',')
                .next()
                .is_some_and(|last| last.trim().eq_ignore_ascii_case("chunked"));
        } else if name.eq_ignore_ascii_case("location") {
            response.location = Some(value);
        } else if name.eq_ignore_ascii_case("content-type") {
            response.content_type = Some(value);
        }
    }
    ResponseParse::Complete(response)
}

/// Decode a chunked body (everything after the head) into `out`. `Some`
/// with the decoded length once the last chunk has arrived; `None` while
/// it has not, or if the framing is broken or `out` is too small.
pub fn dechunk(body: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut at = 0;
    let mut written = 0;
    loop {
        let line_end = body[at..].iter().position(|&b| b == b'\n')? + at;
        let line = core::str::from_utf8(&body[at..line_end]).ok()?;
        // Chunk extensions (`;name=value`) are allowed and ignored.
        let size_text = line.trim_end_matches('\r').split(';').next()?.trim();
        let size = usize::from_str_radix(size_text, 16).ok()?;
        at = line_end + 1;
        if size == 0 {
            return Some(written);
        }
        let data = body.get(at..at.checked_add(size)?)?;
        out.get_mut(written..written + size)?.copy_from_slice(data);
        written += size;
        at += size;
        // The chunk's own CRLF.
        match body.get(at..) {
            Some([b'\r', b'\n', ..]) => at += 2,
            Some([b'\n', ..]) => at += 1,
            _ => return None,
        }
    }
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

#[cfg(test)]
mod framing_tests {
    use super::*;

    fn parse_str(text: &str) -> Parse<'_> {
        parse(text.as_bytes())
    }

    #[test]
    fn a_declared_body_is_reported_so_the_caller_can_skip_it() {
        // `Content-Length` was not parsed, and the caller drained exactly
        // the head -- so the body stayed in the receive buffer and was
        // parsed as the next request. One request produced two responses.
        let raw = "GET / HTTP/1.1\r\nHost: x\r\nContent-Length: 33\r\n\r\n\
                   GET /health HTTP/1.1\r\nHost: x\r\n\r\n";
        let Parse::Complete(request) = parse_str(raw) else {
            panic!("must parse");
        };
        assert_eq!(request.body_len, 33, "the declared body must be reported");
        assert!(request.keep_alive);
    }

    #[test]
    fn two_disagreeing_content_lengths_are_refused() {
        let raw = "GET / HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 9\r\n\r\n";
        assert_eq!(parse_str(raw), Parse::Malformed);
        // The same value twice is redundant but not ambiguous.
        let raw = "GET / HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\n";
        assert!(matches!(parse_str(raw), Parse::Complete(_)));
    }

    #[test]
    fn transfer_encoding_this_server_cannot_decode_is_refused() {
        // Ignoring it meant the chunked framing bytes were read as the next
        // request. A server that does not decode chunked must say so.
        let raw = "GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";
        assert_eq!(parse_str(raw), Parse::Unsupported);
    }

    #[test]
    fn a_stray_blank_line_before_the_request_is_tolerated() {
        // Clients and proxies emit a CRLF after a body; answering 400 to it
        // is a needless failure. RFC 9112 2.2.
        for raw in [
            "\r\nGET /health HTTP/1.1\r\nHost: x\r\n\r\n",
            "\nGET /health HTTP/1.1\r\nHost: x\r\n\r\n",
        ] {
            let Parse::Complete(request) = parse_str(raw) else {
                panic!("must parse: {raw:?}");
            };
            assert_eq!(request.target, "/health");
            assert_eq!(request.head_len, raw.len());
        }
    }
}

#[cfg(test)]
mod client_tests {
    use super::*;

    #[test]
    fn urls_are_parsed_as_people_type_them() {
        assert_eq!(
            parse_url("http://10.0.2.2:8080/a?b=1"),
            Ok(Url {
                host: "10.0.2.2",
                port: 8080,
                path: "/a?b=1"
            })
        );
        assert_eq!(
            parse_url("example.com"),
            Ok(Url {
                host: "example.com",
                port: 80,
                path: "/"
            })
        );
        assert_eq!(
            parse_url("http://example.com?q"),
            Ok(Url {
                host: "example.com",
                port: 80,
                path: "?q"
            })
        );
        assert_eq!(parse_url("https://example.com/"), Err(UrlError::NotHttp));
        assert_eq!(parse_url("http://:80/"), Err(UrlError::Malformed));
        assert_eq!(
            parse_url("http://a:b@example.com/"),
            Err(UrlError::Malformed)
        );
        assert_eq!(
            parse_url("http://example.com:99999/"),
            Err(UrlError::Malformed)
        );
        assert_eq!(parse_url("http://exa mple.com/"), Err(UrlError::Malformed));
        assert_eq!(
            parse_url("http://example.com/a b"),
            Err(UrlError::Malformed)
        );
    }

    #[test]
    fn the_request_names_the_host_and_asks_to_close() {
        let mut out = [0u8; 256];
        let url = parse_url("http://example.com:8080?x").unwrap();
        let n = write_request(&url, &mut out).unwrap();
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(
            text.starts_with("GET /?x HTTP/1.1\r\nHost: example.com:8080\r\n"),
            "{text}"
        );
        assert!(text.ends_with("Connection: close\r\n\r\n"));
        let url = parse_url("example.com/index.html").unwrap();
        let n = write_request(&url, &mut out).unwrap();
        assert!(core::str::from_utf8(&out[..n])
            .unwrap()
            .contains("Host: example.com\r\n"));
        assert_eq!(write_request(&url, &mut out[..10]), None);
    }

    #[test]
    fn a_response_head_gives_status_length_chunking_and_redirects() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nhello";
        match parse_response(raw) {
            ResponseParse::Complete(r) => {
                assert_eq!((r.status, r.reason), (200, "OK"));
                assert_eq!(r.content_length, Some(5));
                assert!(!r.chunked);
                assert_eq!(&raw[r.head_len..], b"hello");
            }
            other => panic!("{other:?}"),
        }
        let raw = b"HTTP/1.1 301 Moved\r\nLocation: http://example.org/\r\nTransfer-Encoding: gzip, chunked\r\n\r\n";
        match parse_response(raw) {
            ResponseParse::Complete(r) => {
                assert_eq!(r.location, Some("http://example.org/"));
                assert!(r.chunked);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\n"),
            ResponseParse::Incomplete
        );
        assert_eq!(
            parse_response(b"SSH-2.0-x\r\n\r\n"),
            ResponseParse::Malformed
        );
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: x\r\n\r\n"),
            ResponseParse::Malformed
        );
    }

    #[test]
    fn a_chunked_body_is_decoded_once_it_is_all_there() {
        let body = b"4\r\nWiki\r\n6;ext=1\r\npedia \r\nE\r\nin \r\n\r\nchunks.\r\n0\r\n\r\n";
        let mut out = [0u8; 64];
        let n = dechunk(body, &mut out).unwrap();
        assert_eq!(&out[..n], b"Wikipedia in \r\n\r\nchunks.");
        assert_eq!(dechunk(&body[..20], &mut out), None, "not all there");
        assert_eq!(dechunk(body, &mut out[..8]), None, "no room");
        assert_eq!(dechunk(b"zz\r\n", &mut out), None, "not hex");
    }
}
