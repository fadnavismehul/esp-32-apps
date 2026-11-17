# ESP32-C6 glyph device: agent guide

A Waveshare ESP32-C6 Touch AMOLED 2.16 running our own Rust firmware. One
base firmware owns the hardware, Wi-Fi and an HTTP API. Apps are small crates
that only draw pixels and react to input, so they can be built and checked on
the Mac before they go near the board.

Device API: `docs/api.md`.

## Layout

| Path | What it is | Builds for |
|---|---|---|
| `crates/app-sdk/` | The `App` trait, `Input`, `Output`, `Sound`. The contract every app implements. | host + device |
| `crates/netproto/` | DHCP, DNS, mDNS, HTTP parsing and the settings format. Pure functions with tests. | host + device |
| `apps/<name>/` | One crate per app: `visuals` (wraps `glyphfield`, `blobs` halftone metaballs and `flow` particle flow field; KEY steps through them), `sand` (falling sand), `clock` (dot clock), `pikachu` (a desk pet drawn from a sprite sheet), `city` (a beach city that follows the local clock, in a poster look and a halftone dot look; KEY switches looks). | host + device |
| `sim/` | Runs any app on the Mac and writes PNG frames. | host |
| `tool/` | `espctl`, the CLI for everything else. | host |
| `firmware/` | The base firmware: drivers, Wi-Fi, HTTP API, OTA, app host. Its own Cargo workspace. | device only |

## The loop for changing an app

1. Edit `apps/<name>/src/lib.rs`.
2. `cargo test -p <name>`
3. `cargo run -q -p sim -- <name> --tilt 40 --touch 240,240 --frames 30 --out out/<name>`
   and look at the PNGs in `out/<name>/`. This is the main way to check visuals.
4. `cargo run -q -p espctl -- flash --ota` (or `--usb` with the board plugged in).
5. `cargo run -q -p espctl -- status`, then
   `cargo run -q -p espctl -- input --tilt-deg 45 --hold-ms 5000` and
   `cargo run -q -p espctl -- screenshot out/device.png` to check it on the device.

## Adding an app

1. `cargo new --lib apps/<name>`, make it `#![no_std]`, depend only on `app-sdk`
   (copy `apps/clock/Cargo.toml`). Add it to `members` in `Cargo.toml`.
2. Implement `App`. Give the struct a `const fn new()`.
3. Register it in `sim/src/main.rs` and in `firmware/src/apps.rs` (variant,
   name, constructor), and add the dependency to `firmware/Cargo.toml`.
4. Switch to it with `espctl app <name>`.

## Pikachu's art

`apps/pikachu` draws its creature from a sprite sheet that its `build.rs`
converts at build time (`apps/pikachu/build/convert.rs`). The committed art in
`art/default/` is a stand-in drawn by `make_sheet.py`. To use other sprites,
put two files in `apps/pikachu/art/custom/`, which is git-ignored so the art
stays out of the repo:

- `sheet.png`: a grid of equal frames, transparent background (alpha below
  128 counts as transparent), at most 255 colours.
- `sheet.toml`: copy `art/default/sheet.toml` and edit it.
  - `frame = [w, h]` sets the frame size, at most 240x240.
  - `faces = "left"` or `"right"` says which way the art looks.
  - The optional `scale` is screen pixels per art pixel. The default draws it
    about 240 px tall.
  - Each animation is a table, `[idle]`, `[blink]`, `[walk]`, `[happy]`,
    `[dizzy]` or `[sleep]`, with `row` (the grid row, from 0), `frames` and an
    optional `ms` per frame.
  - Only `idle` is required; a missing animation plays `idle`.
  - The creature's feet are the lowest opaque row of the first idle frame.

Then `cargo test -p pikachu` (it does not check custom art, but the build
does, with a plain error), `cargo run -q -p sim -- pikachu --frames 1 --out
out/pikachu` to look at it, and `espctl flash --ota`. Delete the two files to
go back to the stand-in.

Behaviour: it wanders and blinks, hops with sparks when tapped, walks to a
tap anywhere else, slides when tilted, gets dizzy when shaken, and sleeps from
23:00 to 07:00 (a tap half-wakes it). KEY turns the ambient cheek sparks on
and off.

## City Window's art

`apps/city` shows a beach city framed by palm trees, looking out over
the water toward the skyline and the open sky.

Behaviour: it sweeps smoothly through the day according to the local clock,
from dawn pastel skies to daytime cyan, a vivid sunset, dusk and deep night.
At night the city stands almost black against a low burnt orange haze under
an orange moon, with a few amber stair lights, one teal crown, teal roof
beacons and their light on the water. Before the clock syncs,
it shows the sunset look. Waves roll, clouds drift, palms sway in a light
breeze and stars twinkle. Tilting the device shifts
the layers with parallax, moving the front palms the most and the sun or moon
the least. Shaking the device swings the palms strongly before they settle.
Touching a palm nudges that tree away from your finger so it swings and comes
back to rest. One small event runs every minute or two, or starts right away
on a tap anywhere else on the screen: tail lights on the coast road, a plane
passing overhead with a blinking light, or the teal tower lights flickering.
Only one event runs at a time, and events run in the poster look only. A
small clock in the lower left corner shows the time over the water, drifting
slightly every three minutes to prevent burn in. KEY switches between the
poster look and a halftone dot look.

All art is plain data in `apps/city/src/art.rs`:

- Layout: `HORIZON` sets where the sky meets the sea; `SKY_BANDS`, `SKY_EDGES`
  and `SEA_EDGES` set band boundaries; `BAND_FADE` sets the stripe dither
  between bands; `STAR_ROWS` and `STAR_EVERY` control star placement.
- Day and night colours: `NIGHT`, `BEFORE_DAWN`, `DAWN`, `MORNING`, `DAY`,
  `GOLDEN`, `SUNSET` and `DUSK` define the palettes; `KEYS` schedules them
  across the day and sets orb position, radius, moon phase and night factor;
  `BEFORE_SYNC_MIN` is the fallback time before the clock syncs.
- Sun and moon: `SUN_SLITS` and `SLITS_FROM_R` cut horizontal stripes into
  the sun; `MOON_SHADOW` positions the crescent moon shadow.
- City, lights and clouds: `BUILDINGS` and `FAR_BUILDINGS` define the main and
  background skyline blocks and roofs; `WINDOW_PITCH` and `WINDOW_SIZE`
  space the bands of glass on hotels by day; `HAZE_FAR` and `HAZE_NEAR` set
  how the night haze fades the feet of the buildings; `STAIRS` places the
  amber stair lights; `CROWN` and `BEACONS` pick the towers with teal
  lights; `AMBER`, `AMBER_DIM`, `TEAL`, `TEAL_DIM` and `TEAL_HALO` colour
  them; `FLICKER` sets how the teal
  lights stutter; `TAIL_LIGHT`, `HEAD_LIGHT`
  and `PLANE_LIGHT` colour event vehicles; `CLOCK_NIGHT` and `CLOCK_STRENGTH`
  style the small time display; `CLOUDS` sets cloud streaks.
- Water: it mirrors the sky and city. `MIRROR_EDGES`, `FRESNEL` and
  `MIRROR_SEA` set how strongly each strip mirrors and which sea colour it
  blends toward; `RIPPLE_ROWS_PER_PX` and `WOBBLE_ROWS_PER_PX` set how much
  ripples bend it; `MOON_PATH_SHIFT` places the moon's path under the lit
  part of the crescent; `GLINT` sets how bright the tower light glints are.
- Palms: `PALMS` defines the trunk curves, widths and fronds for each tree.

To check a change, run `cargo test -p city`, and `cargo run -q -p sim -- city
--time 18:15 --frames 1 --out out/city` to look at the frame. In `sim`, pass
`--time HH:MM[:SS]` to view any time of day, `--tilt DEG` to test parallax,
`--tap-at FRAME` to trigger an event or touch, `--key-at FRAME` to toggle the
dot look, and `--frames N` to inspect motion over multiple frames.

All art must stay original. Do not copy characters, logos or artwork from
other games or media.

## Rules for app code

- App crates build at `opt-level = 3` (see `firmware/Cargo.toml`); add a
  line there for a new app with heavy per-frame work.
- No floats in per-frame code. The C6 has no FPU. Use integer or fixed-point
  math and lookup tables.
- No heap and no blocking. Apps are `no_std` without `alloc`.
- `render(rows, buf)` must depend only on state. It is called for 12-row
  strips on the device, and for 2-row strips when taking a screenshot.
- Keep state small. The whole device has about 512 KB of RAM, shared with
  Wi-Fi and display buffers. `glyphfield` is about 5 KB, `blobs` 3 KB, `flow`
  12 KB, `sand` 14 KB, `city` about 11 KB.
- Pixels are RGB565 (`app_sdk::rgb565(r, g, b)`). Black is free on AMOLED.
- Sounds: set `out.sound = Some(Sound::Tick { pitch })` or `Sound::Click`.
  One per frame.

## Device facts

- Screen 480x480, touch 480x480 in the same coordinates.
- Tilt is gravity in milli-g along the screen axes: `x_mg` toward the right
  edge (USB side), `y_mg` toward the bottom. The buttons are on the left edge.
  Standing upright with the clock reading normally is about `(0, 1000, 0)`;
  flat is `(0, 0, ±1000)`. The chip's axes are rotated; `board/imu.rs` maps
  them.
- The firmware renders at 30 fps; a glyphfield frame takes about 25 ms.
- Buttons (on the left edge; PWR is the middle one): BOOT (GPIO9) held 3 s reboots into Wi-Fi setup
  mode; PWR (AXP2101, read over I2C) short press switches to the next app in
  `NAMES` order (on battery, holding it about 6 s powers off); KEY (GPIO10)
  held 0.6 s opens the settings screen (`firmware/src/settings.rs`: Wi-Fi
  on/off, volume, brightness, time zone; saved on close, closes itself after 30 s). A
  short press reaches the app as `Input::key`, a modifier each app may use
  (`visuals` steps to the next visual, `city` switches looks); it also closes
  settings. `espctl key [--long]` presses it remotely; drive settings with
  `espctl input --tap`. In `sim`, `--key-at FRAME` presses it.
- Wall clock: SNTP from `pool.ntp.org` on joining Wi-Fi, then hourly. Apps get
  `Input::local_time_s` (shifted by the saved time zone, UTC+8 / 480 min by default,
  `state::utc_offset_min()`), `None` until synced. In `sim`, pass `--time HH:MM[:SS]`.
- Reminders belong to the base firmware, not an app, so they fire over
  whatever is running. `firmware/src/remind.rs` checks the saved list
  (`netproto::reminders`, stored after the settings in the `cfg` sector) each
  minute and queues alerts; `firmware/src/alert.rs` draws the full-screen
  card instead of the app, which pauses. It chimes every 10 s for the first
  minute. A tap dismisses it; PWR snoozes it 10 minutes instead of switching
  apps. Snoozes live in RAM only.
- Sounds stream through a small DMA ring (`audio::Voice`), so they can be
  any length; each is padded to a 1 KB chunk.
- Wi-Fi: out of range, the device keeps scanning for its saved networks
  (every 3 s for a minute, then every 20 s) and the apps keep running. It
  only opens the `glyph-setup-XXXX` access point when no network is saved,
  or when asked (BOOT held 3 s, `espctl setup`). Switched off in settings,
  it stops scanning until switched back on there; `status` then says
  `"mode":"off"` (when you can reach it at all).
- USB serial: `espflash monitor --chip esp32c6 --non-interactive`. The board
  prints `token=...` and `ip=...` at boot.

## espctl

`cargo run -q -p espctl -- <command>`; add `--json` for machine output.

| Command | Does |
|---|---|
| `pair` | Reads the token (and IP) over USB and saves them to `.env`. Run once per board. |
| `status`, `logs [--follow]`, `screenshot <png>` | Look at the device. |
| `input --tilt x,y,z \| --tilt-deg D --touch x,y --tap --hold-ms N` | Fake sensor input. |
| `app <name>`, `volume <0-100>`, `timezone [<sgt|ist|+H:MM>]`, `sound tick\|click\|chime` | Control it. |
| `remind list`, `remind add 18:30 Take meds --days weekdays`, `remind delete <id>` | Reminders. `add` also takes `--date YYYY-MM-DD`, or `--every 90m --until 17:00`; with no option it fires once, next time it is that time. |
| `remind test <id>`, `remind dismiss`, `remind snooze` | Show one now, or act on the one on screen. |
| `wifi list`, `wifi add <ssid> --pass-stdin`, `wifi forget <ssid>` | Saved networks. |
| `wifi sync --usb` | Writes `WIFI_SSID`/`WIFI_PASS` (and `_2`..`_4`) from `.env` into the `cfg` partition over USB. `flash --usb` does this too. |
| `flash --usb`, `flash --ota`, `build` | Ship firmware. |
| `key [--long]` | Press KEY (the app's modifier) or hold it (settings). |
| `reboot`, `setup` | Restart, or restart into Wi-Fi setup mode. |
| `sim <app> ...` | Same as running `sim`. |

The host comes from `--host`, `ESPCTL_HOST`, `.env`, then `espc6.local`.
`.env` is git-ignored and holds the API token and Wi-Fi networks.
Saved networks live in the `cfg` partition, which flashing never erases.

## Recovering a bad board

Nothing here can brick it: the ROM bootloader always accepts USB flashing.

- A watchdog (TIMG1, 5 s) resets the board if the main loop stops coming
  round. A panic halts the CPU, so the watchdog is also what reboots it.
- A new OTA image is on probation. It confirms itself once it has an IP and
  has drawn 90 frames. If it has not within 60 s it rolls back to the other
  slot, and so it does on its 4th unconfirmed boot (crash loops). The ESP-IDF
  bootloader espflash installs has no rollback of its own; this all lives in
  `firmware/src/lib.rs` (`check_boot`, `confirm_image`). The boot log shows
  `ota: state ..., try N` and `ota: image confirmed`.

If an OTA image still misbehaves, plug in USB and run `espctl flash --usb`. If it
will not even enumerate, hold BOOT while plugging in, then flash.

## Firmware notes

- `firmware/` builds only for `riscv32imac-unknown-none-elf`; run cargo from
  inside it (its `.cargo/config.toml` sets the target).
- Board drivers live in `firmware/src/board/`. The init sequences came from
  Waveshare's BSP and the Clawdmeter project; quirks are
  noted in each file.
- Flash layout: `firmware/partitions.csv` (two 4 MB OTA slots, a 4 KB `cfg`
  sector for settings).
