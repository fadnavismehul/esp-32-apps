# espctl

Run `cargo run -p espctl -- help` for all commands. Global flags are
`--host HOST[:PORT]` and `--json`; put them before `sim` so simulator flags
can pass through unchanged.

```sh
cargo run -p espctl -- pair
cargo run -p espctl -- --json status
cargo run -p espctl -- wifi add 'Home Wi-Fi' --pass-stdin
cargo run -p espctl -- input --tilt-deg 30 --tilt-dir 90 --touch 240,240 --tap
cargo run -p espctl -- screenshot /tmp/screen.png
cargo run -p espctl -- flash --usb
cargo run -p espctl -- flash --ota
cargo run -p espctl -- sim glyphfield --frames 3 --tap-at 1 --out /tmp/glyphfield
```

Host precedence: `--host`, `ESPCTL_HOST` environment variable, `ESPCTL_HOST`
in the workspace `.env`, then `espc6.local`. Token precedence: `ESPCTL_TOKEN`
environment variable, then the workspace `.env`. Pairing updates that file
without removing unrelated entries and prints only the token's first four
characters. `--pass-stdin` reads one line, removing its newline.

JSON commands emit one object. The streaming exception is
`--json logs --follow`, which emits one object per new chunk as newline-delimited
JSON (`{"logs":"..."}`). Plain logs follow prints just the new text. Both
handle overlap when the device's log tail rolls over. Stop following with Ctrl-C.
In JSON mode, build/flash child progress goes to stderr so stdout contains only
the result object. Simulator stdout is already JSON.

HTTP uses ureq 3.5 with a five-second connect timeout and a thirty-second
request/read budget (120 seconds for OTA). Requests go directly to the device,
without proxies or redirects. OTA waits two seconds before its first status
poll, then retries failures every two seconds for up to ninety seconds.

Tests use local HTTP servers and stand-in cargo/espflash executables; they do
not build firmware, access USB, change the workspace `.env`, or flash a board.
