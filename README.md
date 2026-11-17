# esp-32-apps

Rust firmware and small apps for the Waveshare ESP32-C6 Touch AMOLED 2.16.

One base firmware owns the hardware, Wi-Fi and an HTTP API. Each app is a
small `no_std` crate that only draws pixels and reacts to touch, tilt and
the KEY button. The same apps run in a simulator on a computer, which writes
PNG frames, so they can be checked before they go on the board.

Apps:

- `visuals`: three generative visuals (glyph field, halftone blobs, flow field).
- `sand`: falling sand that follows tilt.
- `clock`: a dot matrix clock.
- `pikachu`: a small desk pet drawn from a sprite sheet.
- `city`: a beach city that follows the local time of day.

The `espctl` tool flashes the board over USB or Wi-Fi, takes screenshots,
fakes input and sets reminders. `AGENTS.md` describes the layout, the build
loop and the device in full. `docs/api.md` describes the HTTP API.

## Quick start

```bash
cargo test
cargo run -q -p sim -- city --time 18:15 --frames 1 --out out/city
cargo run -q -p espctl -- flash --usb
```

Put your Wi-Fi networks in a `.env` file at the repo root before flashing
(`WIFI_SSID`, `WIFI_PASS`, and `_2` to `_4` for more). The file is git-ignored.
Without it, the board opens a `glyph-setup-XXXX` access point where you can
pick a network.
