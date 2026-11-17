#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "espctl-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("firmware/target")).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        fs::write(root.join("firmware/partitions.csv"), "").unwrap();
        Self {
            root: root.canonicalize().unwrap(),
        }
    }
    fn script(&self, name: &str, body: &str) {
        let path = self.root.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_espctl"))
            .args(args)
            .env("PATH", self.root.join("bin"))
            .env_remove("ESPCTL_HOST")
            .env_remove("ESPCTL_TOKEN")
            .current_dir(self.root.join("firmware"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn usb_build_arguments_and_json_stdout() {
    let fixture = Fixture::new();
    fixture.script(
        "cargo",
        "printf '%s\\n' \"$PWD\" \"$@\" > ../cargo-args\nprintf 'building\\n'",
    );
    fixture.script(
        "espflash",
        "printf '%s\\n' \"$@\" > ../flash-args\nprintf 'flashing\\n'",
    );
    let output = fixture.run(&["--json", "flash", "--usb", "--port", "tty-test"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["transport"], "usb");
    let cargo = fs::read_to_string(fixture.root.join("cargo-args")).unwrap();
    assert_eq!(
        cargo,
        format!(
            "{}\nbuild\n--release\n",
            fixture.root.join("firmware").display()
        )
    );
    let flash = fs::read_to_string(fixture.root.join("flash-args")).unwrap();
    assert_eq!(flash,format!("flash\n--chip\nesp32c6\n--partition-table\n{}\n--erase-parts\notadata\n--non-interactive\n--port\ntty-test\n{}\n",fixture.root.join("firmware/partitions.csv").display(),fixture.root.join("firmware/target/riscv32imac-unknown-none-elf/release/firmware").display()));
}
#[test]
fn pairing_saves_token_without_printing_secret() {
    let fixture = Fixture::new();
    fixture.script(
        "espflash",
        "printf '\\033[32mtoken=0123456789abcdef0123456789abcdef\\033[0m\\nip=192.168.2.3\\n'",
    );
    fs::write(fixture.root.join(".env"), "OTHER=keep\nESPCTL_TOKEN=old\n").unwrap();
    let output = fixture.run(&["--json", "pair", "--port", "test"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["host"], "192.168.2.3");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("0123456789abcdef"));
    assert_eq!(
        fs::read_to_string(fixture.root.join(".env")).unwrap(),
        "OTHER=keep\nESPCTL_TOKEN=0123456789abcdef0123456789abcdef\nESPCTL_HOST=192.168.2.3\n"
    );
}
#[test]
fn failed_build_stops_flash() {
    let fixture = Fixture::new();
    fixture.script("cargo", "exit 7");
    fixture.script("espflash", "touch ../unexpected");
    assert_eq!(fixture.run(&["flash", "--usb"]).status.code(), Some(1));
    assert!(!fixture.root.join("unexpected").exists());
}

#[test]
fn ota_image_upload_and_new_status() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    let fixture = Fixture::new();
    fixture.script("cargo", "exit 0");
    fixture.script("espflash", "printf '%s\\n' \"$@\" > ../image-args\nfor last do :; done\nprintf 'app-image' > \"$last\"");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut header = Vec::new();
            let mut b = [0; 1];
            while !header.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut b).unwrap();
                header.push(b[0]);
            }
            let header = String::from_utf8(header).unwrap();
            if index == 0 {
                assert!(header.starts_with("POST /ota HTTP/1.1"));
                assert!(header
                    .to_ascii_lowercase()
                    .contains("content-length: 9\r\n"));
                let mut body = [0; 9];
                stream.read_exact(&mut body).unwrap();
                assert_eq!(&body, b"app-image");
                stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            } else {
                assert!(header.starts_with("GET /status HTTP/1.1"));
                let body = br#"{"app":"hello","fw":"new","uptime_s":1}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
            requests.push(header);
        }
        requests
    });
    let output = fixture.run(&["--json", "--host", &address.to_string(), "flash", "--ota"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["fw"], "new");
    assert_eq!(server.join().unwrap().len(), 2);
    let args = fs::read_to_string(fixture.root.join("image-args")).unwrap();
    assert_eq!(
        args,
        format!(
            "save-image\n--chip\nesp32c6\n--flash-size\n16mb\n--partition-table\n{}\n{}\n{}\n",
            fixture.root.join("firmware/partitions.csv").display(),
            fixture
                .root
                .join("firmware/target/riscv32imac-unknown-none-elf/release/firmware")
                .display(),
            fixture.root.join("firmware/target/firmware.bin").display()
        )
    );
}
