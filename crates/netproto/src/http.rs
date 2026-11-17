use core::fmt::{self, Write};
#[derive(Debug)]
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub content_length: Option<usize>,
    pub bearer: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub header_len: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Malformed,
    TooLarge,
}
/// Maximum header block size, including the terminating CRLF pair.
pub const MAX_HEADER_LEN: usize = 8192;
pub fn parse_request(buf: &[u8]) -> Result<Option<Request<'_>>, ParseError> {
    let end = match buf.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(p) => p + 4,
        None => {
            return if buf.len() >= MAX_HEADER_LEN {
                Err(ParseError::TooLarge)
            } else {
                Ok(None)
            }
        }
    };
    if end > MAX_HEADER_LEN {
        return Err(ParseError::TooLarge);
    }
    let text = core::str::from_utf8(&buf[..end]).map_err(|_| ParseError::Malformed)?;
    let mut lines = text[..text.len() - 4].split("\r\n");
    let mut parts = lines.next().ok_or(ParseError::Malformed)?.split(' ');
    let method = parts.next().ok_or(ParseError::Malformed)?;
    let target = parts.next().ok_or(ParseError::Malformed)?;
    let version = parts.next().ok_or(ParseError::Malformed)?;
    if method.is_empty()
        || !method.bytes().all(token)
        || !target.starts_with('/')
        || target.bytes().any(|b| b <= 32 || b == 127)
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || parts.next().is_some()
    {
        return Err(ParseError::Malformed);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut r = Request {
        method,
        path,
        query,
        content_length: None,
        bearer: None,
        content_type: None,
        header_len: end,
    };
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(ParseError::Malformed)?;
        if name.is_empty()
            || !name.bytes().all(token)
            || value.bytes().any(|b| (b < 32 && b != 9) || b == 127)
        {
            return Err(ParseError::Malformed);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("content-length") {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ParseError::Malformed);
            }
            let n = value.parse().map_err(|_| ParseError::TooLarge)?;
            if r.content_length.is_some() {
                return Err(ParseError::Malformed);
            }
            r.content_length = Some(n);
        } else if name.eq_ignore_ascii_case("authorization") {
            if let Some((scheme, t)) = value.split_once(' ') {
                let t = t.trim();
                if scheme.eq_ignore_ascii_case("bearer")
                    && !t.is_empty()
                    && !t.bytes().any(|b| b.is_ascii_whitespace())
                {
                    r.bearer = Some(t)
                }
            }
        } else if name.eq_ignore_ascii_case("content-type") {
            r.content_type = Some(value)
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(ParseError::Malformed);
        }
    }
    Ok(Some(r))
}
fn token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}
pub fn param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find_map(|(k, v)| if k == key { Some(v) } else { None })
}
pub fn url_decode<'b>(input: &str, out: &'b mut [u8]) -> Option<&'b str> {
    let mut src = input.bytes();
    let mut n = 0;
    while let Some(b) = src.next() {
        let b = match b {
            b'+' => b' ',
            b'%' => hex(src.next()?)?.checked_mul(16)? + hex(src.next()?)?,
            _ => b,
        };
        *out.get_mut(n)? = b;
        n += 1;
    }
    core::str::from_utf8(out.get(..n)?).ok()
}
fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
/// Returns zero if the output is too small or the content type contains controls.
/// On failure the output may contain a partial header; do not transmit it.
pub fn write_response(
    out: &mut [u8],
    status: u16,
    content_type: &str,
    content_length: usize,
) -> usize {
    if content_type.bytes().any(|b| b < 32 || b == 127) {
        return 0;
    }
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    };
    let mut w = Writer { out, n: 0 };
    if write!(w,"HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n").is_err(){0}else{w.n}
}
struct Writer<'a> {
    out: &'a mut [u8],
    n: usize,
}
impl Write for Writer<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.n.checked_add(s.len()).ok_or(fmt::Error)?;
        self.out
            .get_mut(self.n..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.n = end;
        Ok(())
    }
}
