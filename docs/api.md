# Device HTTP API

The base firmware serves this on port 80. `espctl` wraps it. Plain HTTP/1.1,
one request per connection (`Connection: close`), no chunked request bodies.

## Finding the device

- On a network it joined (station mode): `http://espc6.local` over mDNS, or its
  IP, which it prints on the serial log and shows on screen for a few seconds
  after connecting.
- In setup mode (access point mode): join the open Wi-Fi network
  `glyph-setup-XXXX` and open `http://192.168.4.1`. Phones usually pop the
  page up on their own because every DNS name resolves to the device.

## Auth

Every request except `GET /status` and the setup-mode pages needs
`Authorization: Bearer <token>`. The device makes a random 32-hex-character
token on first boot, keeps it in flash, and prints `token=<token>` on the USB
serial log at every boot. `espctl pair` reads it from there and saves it in
`.env`.

Wrong or missing token: `401`. Unknown path: `404`. Bad parameters: `400`
with a one-line plain-text reason.

Parameters go in the query string (`?a=1&b=2`), URL-encoded. The only bodies
are the OTA image and the setup form.

## Endpoints

### `GET /status`

```json
{
  "fw": "0.2.0",
  "mode": "sta",
  "ssid": "HomeNet",
  "ip": "192.168.1.42",
  "rssi": -54,
  "app": "visuals",
  "apps": ["visuals", "sand", "clock", "pikachu"],
  "fps": 30,
  "uptime_s": 1234,
  "heap_free": 41234,
  "volume": 60,
  "utc_offset_min": 480,
  "ota_slot": "ota_0"
}
```

`mode` is `"sta"`, `"ap"`, or `"off"` (Wi-Fi switched off in settings). `ssid`, `ip` and `rssi` are `null` when not
connected. `utc_offset_min` is the time zone offset in minutes from UTC (e.g. `480` for Singapore +8:00, `330` for India +5:30).

### `GET /logs`

The last ~4 KB of the device log as `text/plain`, oldest first.

### `GET /screenshot`

The current frame as raw RGB565, big-endian, 480x480 row-major
(460800 bytes, `application/octet-stream`). The device re-renders the frame
strip by strip for this, so it matches what the panel shows.

### `POST /input`

Overrides the sensors for a while, so a test can drive the app with no one
holding the board. All parameters are optional.

- `tilt=x,y,z` in milli-g (flat is `0,0,1000`)
- `touch=x,y` holds a finger at that point
- `tap=1` sends one tap (at `touch` if given)
- `hold_ms=N` how long the override lasts, default 3000, max 60000

Returns `204`.

### `POST /app?name=<name>`

Switches the running app: `visuals`, `sand`, `clock` or `pikachu`, or `glyphfield`,
`blobs` or `flow` to open `visuals` on that one. Returns `204`, or `404` for an unknown name. The
choice is saved and survives a reboot.

### `POST /volume?level=<0..100>`

Speaker volume, saved. `0` mutes. Returns `204`.

### `GET /timezone`

Returns the current time zone offset in minutes: `{"utc_offset_min": 480}`.

### `POST /timezone`

Sets the time zone offset, saved to flash. The zone is the plain body, or
`?zone=`: `sgt`, `ist`, or `[+|-]H:MM` (for example `+5:30` or `-3:00`; write
`+` as `%2B` in a query). Returns `200` with
`{"utc_offset_min": <minutes>}`, or `400` if invalid.

### `POST /sound?kind=tick|click|chime`

Plays a sound once, for testing the speaker. Returns `204`.

### `GET /reminders`

```json
{
  "now": "2026-10-26 14:01",
  "showing": { "id": 3, "text": "Stand up" },
  "waiting": 1,
  "reminders": [
    { "id": 2, "text": "Stretch", "repeat": "weekly", "time": "18:30",
      "days": "weekdays", "next": "2026-10-26 18:30" },
    { "id": 3, "text": "Stand up", "repeat": "interval", "every_min": 90,
      "from": "09:00", "to": "17:00", "days": "daily", "next": "2026-10-26 15:00" },
    { "id": 4, "text": "Call the bank", "repeat": "once",
      "at": "2026-11-02 08:00", "next": "2026-11-02 08:00" }
  ]
}
```

Times are the device's local time (UTC+8). `now` and every `next` are `null`
until the clock has synced. `showing` is the reminder on screen, or `null`;
`waiting` counts snoozed or queued ones. `days` is `daily`, `weekdays`,
`weekends` or a list like `mon,wed`.

### `POST /reminders?time=HH:MM&text=<text>[&...]`

Adds a reminder and returns `201` with `{"id": 5, "next": "2026-10-26 18:30"}`.
`text` is 1 to 60 printable ASCII characters. The other parameters pick the
kind:

- nothing: once, the next time the clock reads `HH:MM` (`503` if the clock
  has not synced yet)
- `date=YYYY-MM-DD`: once, on that day (`400` if that time has passed)
- `days=...`: every week on those days
- `every=N` minutes (1..1440), with optional `until=HH:MM` (default 23:59)
  and `days` (default daily): from `time` to `until`, every N minutes

At most 16 are kept (`409` when full). A one-off deletes itself when it
fires; one that came due while the device was off fires once the clock syncs.

### `POST /reminders/delete?id=<id>`

Returns `204`, or `404`.

### `POST /reminders/test?id=<id>`

Shows that reminder now, as if it had fired. Returns `204`, or `404`.

### `POST /reminders/dismiss`, `POST /reminders/snooze`

Act on the reminder on screen, like a tap or the PWR button. Snooze brings it
back in 10 minutes (snoozes are lost on reboot). Returns `204`, or `404` when
nothing is showing.

### `GET /wifi`

```json
{ "networks": ["HomeNet", "Office"] }
```

Saved networks, most recently added first. Passwords are never returned.

### `POST /wifi?ssid=<ssid>&pass=<password>`

Saves a network (up to 4; the oldest is dropped). An existing entry with the
same SSID is replaced. Returns `204`. It takes effect at the next reboot or
reconnect.

### `POST /wifi/forget?ssid=<ssid>`

Returns `204`, or `404` if not saved.

### `POST /ota`

Body: an ESP-IDF app image, as produced by
`espflash save-image --chip esp32c6 <elf> <bin>`. Needs a `Content-Length`.
The device writes it to the inactive OTA slot, marks that slot to boot next,
replies `204`, and reboots about a second later. If the upload is cut short
or the image is too large, it replies `400` and keeps running the current
image. The new image must get an IP and draw 90 frames within 60 s, and
within 3 boots, or the device goes back to the previous one.

### `POST /key[?long=1]`

Presses the KEY button. A short press goes to the app (in `visuals` it moves
to the next visual) or closes settings; `long=1` is a hold, which opens or
closes settings. Returns `204`.

### `POST /reboot`

Returns `204`, then reboots.

### `POST /setup`

Returns `204`, then reboots into setup mode (access point). The same thing
happens when the BOOT button is held for 3 seconds.

## Setup mode pages (no token)

- `GET /` a small form with SSID and password fields.
- `POST /` with an `application/x-www-form-urlencoded` body
  `ssid=...&pass=...`. Saves the network, shows a "Saved, rebooting" page and
  reboots into station mode.

Setup mode ends after 10 minutes without a saved network, and the device
reboots to try its saved networks again.
