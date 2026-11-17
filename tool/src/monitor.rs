#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ansi_and_monitor_fields() {
        let line = "\x1b[32mboot token=0123456789aBCDef0123456789abcdef ip=192.168.1.42\x1b[0m\r\n";
        assert_eq!(
            strip_ansi(line),
            "boot token=0123456789aBCDef0123456789abcdef ip=192.168.1.42\r\n"
        );
        assert_eq!(
            fields(line),
            (
                Some("0123456789aBCDef0123456789abcdef".into()),
                Some("192.168.1.42".into())
            )
        );
        for line in [
            "token=abc",
            "token=0123456789abcdef0123456789abcdef0",
            "token=gggggggggggggggggggggggggggggggg ip=999.2.3.4",
        ] {
            assert_eq!(fields(line), (None, None));
        }
        assert_eq!(strip_ansi("a\x1b]0;title\x07b"), "ab");
    }
}

use crate::device::Result;
use std::{
    io::{BufRead, BufReader},
    net::Ipv4Addr,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub fn strip_ansi(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut escape = false;
                for c in chars.by_ref() {
                    if c == '\x07' || (escape && c == '\\') {
                        break;
                    }
                    escape = c == '\x1b';
                }
            }
            _ => {}
        }
    }
    out
}
pub fn fields(line: &str) -> (Option<String>, Option<String>) {
    let line = strip_ansi(line);
    let field = |key: &str| -> Option<String> {
        for (pos, _) in line.match_indices(key) {
            if pos > 0
                && line[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let tail = &line[pos + key.len()..];
            let s = tail
                .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ']' | ')'))
                .next()
                .unwrap_or("");
            if !s.is_empty() {
                return Some(s.into());
            }
        }
        None
    };
    let token =
        field("token=").filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let ip = field("ip=").filter(|s| s.parse::<Ipv4Addr>().is_ok());
    (token, ip)
}
pub fn pair(port: Option<&str>) -> Result<(String, Option<String>)> {
    let mut cmd = Command::new("espflash");
    cmd.args(["monitor", "--chip", "esp32c6", "--non-interactive"]);
    if let Some(port) = port {
        cmd.args(["--port", port]);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("monitor stdout unavailable")?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut token = None;
    let mut ip = None;
    let result = loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break Ok(());
        };
        match rx.recv_timeout(remaining) {
            Ok(Ok(line)) => {
                let (t, i) = fields(&line);
                if t.is_some() {
                    token = t;
                }
                if i.is_some() {
                    ip = i;
                }
                if token.is_some() && ip.is_some() {
                    break Ok(());
                }
            }
            Ok(Err(e)) => break Err(Box::new(e) as Box<dyn std::error::Error>),
            Err(_) => break Ok(()),
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    result?;
    Ok((
        token.ok_or("no token found in espflash monitor output within 20 seconds")?,
        ip,
    ))
}
