use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Output},
    thread,
};

fn serve(args: &[&str], status: &str, body: Vec<u8>) -> (Output, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut b = [0; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut b).unwrap();
            request.push(b[0]);
        }
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        String::from_utf8(request).unwrap()
    });
    let output = Command::new(env!("CARGO_BIN_EXE_espctl"))
        .args(["--host", &address.to_string(), "--json"])
        .args(args)
        .env("ESPCTL_TOKEN", "testtoken")
        .output()
        .unwrap();
    (output, server.join().unwrap())
}
#[test]
fn http_status_auth_and_query() {
    let (out, request) = serve(
        &["status"],
        "200 OK",
        br#"{"app":"hello","fw":"test","uptime_s":3}"#.to_vec(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["app"], "hello");
    assert!(request.starts_with("GET /status HTTP/1.1\r\n"));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer testtoken\r\n"));
    let (out, request) = serve(
        &["wifi", "add", "A & B", "--pass", "x+y?#"],
        "204 No Content",
        vec![],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(request.starts_with("POST /wifi?ssid=A%20%26%20B&pass=x%2By%3F%23 HTTP/1.1\r\n"));
    assert!(request
        .to_ascii_lowercase()
        .contains("content-length: 0\r\n"));
}
#[test]
fn runtime_and_usage_exit_codes() {
    let (out, _) = serve(&["status"], "401 Unauthorized", b"unauthorized".to_vec());
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("HTTP 401"));
    let out = Command::new(env!("CARGO_BIN_EXE_espctl"))
        .args(["volume", "101"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
#[test]
fn screenshot_png_and_bad_length() {
    let file = std::env::temp_dir().join(format!("espctl-shot-{}.png", std::process::id()));
    let body = [0xf8, 0].repeat(480 * 480);
    let (out, _) = serve(&["screenshot", file.to_str().unwrap()], "200 OK", body);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut reader = png::Decoder::new(std::fs::File::open(&file).unwrap())
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (480, 480));
    assert!(pixels.chunks_exact(3).all(|p| p == [255, 0, 0]));
    std::fs::remove_file(&file).unwrap();
    let (out, _) = serve(
        &["screenshot", file.to_str().unwrap()],
        "200 OK",
        vec![0; 2],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(!file.exists());
}
