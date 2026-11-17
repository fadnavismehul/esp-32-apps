#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use app_sdk::{App, Input, Output, Sound, Tilt, Touch};
use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::dma::DmaTxStreamBuf;
use esp_hal::{dma_tx_buffer, dma_tx_stream_buffer};
use esp_hal::gpio::{Input as Pin, InputConfig, Level, Output as Out, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::i2s::master::{Channels, DataFormat, I2s, I2sTx, TdmConfig};
use esp_hal::rng::Rng;
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::{MwdtStage, TimerGroup};
use esp_hal::{Async, efuse};
use esp_storage::FlashStorage;
use firmware::apps::ActiveApp;
use firmware::board::{audio, display, imu, pmu, touch};
use firmware::state::{self, Mode};
use firmware::{alert, logger, net, overlay, persist, remind, settings};
use log::{info, warn};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

/// A press shorter than this counts as a tap.
const TAP_MS: u64 = 300;
const FRAME: Duration = Duration::from_millis(33);
/// Hold KEY this long to open settings; shorter is a press for the app.
const KEY_HOLD: Duration = Duration::from_millis(600);
/// Hold BOOT this long to enter setup mode.
const SETUP_HOLD: Duration = Duration::from_secs(3);
/// The main loop must come round this often or the watchdog resets the board.
/// A panic halts the CPU, so this is also what reboots after a panic.
const WATCHDOG: Duration = Duration::from_secs(5);
/// A new OTA image must have Wi-Fi and a running app by then, or we roll back.
/// Longer than the station join timeout, so a slow join still counts.
const CONFIRM_BY: Duration = Duration::from_secs(60);
/// Frames the app must render before the image counts as working.
const CONFIRM_FRAMES: u32 = 90;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    logger::init();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let p = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);
    esp_alloc::heap_allocator!(size: 48 * 1024);

    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0, p.FROM_CPU_INTR0);
    let mut wdt = TimerGroup::new(p.TIMG1).wdt;
    wdt.set_timeout(MwdtStage::Stage0, esp_hal::time::Duration::from_millis(WATCHDOG.as_millis()));
    wdt.enable();

    // Settings first: they decide the app, the volume and the Wi-Fi mode.
    let mut flash = FlashStorage::new(p.FLASH);
    firmware::check_boot(&mut flash);
    let (mut cfg, reminders, utc_offset_min) = persist::load(&mut flash);
    state::set_utc_offset_min(utc_offset_min);
    info!("tz: offset {} min", state::utc_offset_min());
    let rng = Rng::new();
    let fresh_token = cfg.token().is_empty();
    if fresh_token {
        let mut random = [0u8; 16];
        for chunk in random.chunks_exact_mut(4) {
            chunk.copy_from_slice(&rng.random().to_le_bytes());
        }
        cfg.set_token_from_random(random);
    }
    info!("fw {} slot {}", env!("CARGO_PKG_VERSION"), firmware::ota_slot_name_from(&mut flash));
    info!("token={}", cfg.token());
    let (app_name, volume) = (ActiveApp::canonical(cfg.app()).unwrap_or("visuals"), cfg.volume());
    state::CONFIG.lock(|c| *c.borrow_mut() = cfg);
    info!("reminders: {} saved", reminders.iter().count());
    state::reminders(|r| *r = reminders);
    *persist::FLASH.lock().await = Some(flash);
    if fresh_token {
        persist::save().await;
    }

    let mut i2c = I2c::new(p.I2C0, I2cConfig::default().with_frequency(Rate::from_khz(400)))
        .unwrap()
        .with_sda(p.GPIO8)
        .with_scl(p.GPIO7);

    match pmu::init(&mut i2c) {
        Ok(()) => info!("pmu: rails up"),
        Err(e) => warn!("pmu: {e:?}"),
    }
    match imu::init(&mut i2c) {
        Ok(id) => info!("imu: who_am_i={id:#04x}"),
        Err(e) => warn!("imu: {e:?}"),
    }
    let mut touch_rst = Out::new(p.GPIO11, Level::High, OutputConfig::default());
    touch::reset(&mut touch_rst);

    // Audio: I2S must be configured (MCLK running) before the codec.
    let i2s = I2s::new(
        p.I2S0,
        p.DMA_CH1,
        TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(audio::SAMPLE_RATE))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::STEREO),
    )
    .unwrap()
    .with_mclk(p.GPIO19)
    .into_async();
    let i2s_tx = i2s.i2s_tx.with_bclk(p.GPIO20).with_ws(p.GPIO22).with_dout(p.GPIO23).build();
    match audio::init(&mut i2c, volume) {
        Ok(()) => info!("audio: codec up, volume {volume}"),
        Err(e) => warn!("audio: {e:?}"),
    }
    spawner.spawn(sound_task(i2s_tx, dma_tx_stream_buffer!(audio::BUFFER_BYTES, audio::CHUNK)).unwrap());

    let mut spi = Spi::new(
        p.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(SpiMode::_0),
    )
    .unwrap()
    .with_sck(p.GPIO0)
    .with_sio0(p.GPIO1)
    .with_sio1(p.GPIO2)
    .with_sio2(p.GPIO3)
    .with_sio3(p.GPIO4)
    .with_cs(p.GPIO15);
    display::init(&mut spi, state::config(|c| c.brightness()));
    info!("display: init done");
    let mut lcd = display::Display::new(
        spi.with_dma(p.DMA_CH0),
        dma_tx_buffer!(display::STRIP_BYTES).unwrap(),
        dma_tx_buffer!(display::STRIP_BYTES).unwrap(),
    );

    state::APP.lock().await.switch_to(app_name);
    info!("app: {app_name}");

    let mac = efuse::base_mac_address();
    let mac = mac.as_bytes();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    net::start(spawner, p.WIFI, seed, u16::from_be_bytes([mac[4], mac[5]]));

    // KEY (GPIO10): a short press goes to the app, holding it opens settings.
    let key = Pin::new(p.GPIO10, InputConfig::default().with_pull(Pull::Up));
    let mut key_since: Option<Instant> = None;
    let mut key_held = false;
    let boot_btn = Pin::new(p.GPIO9, InputConfig::default().with_pull(Pull::Up));
    spawner.spawn(button_task(boot_btn).unwrap());

    let boot = Instant::now();
    let mut last = boot;
    let mut press_start: Option<Instant> = None;
    let mut last_touch = (0, 0);
    let mut frames = 0u32;
    let mut stats_at = boot;
    let mut drawn = 0u32;

    loop {
        wdt.feed();
        let now = Instant::now();
        if state::with(|s| s.busy) {
            // OTA in progress: leave the CPU to the network. The upload holds
            // the flash lock, so nothing below that saves may run.
            Timer::after(Duration::from_millis(200)).await;
            last = Instant::now();
            continue;
        }
        if firmware::image_pending() {
            // Switched off on purpose counts as working, or it would roll back.
            let online = state::with(|s| s.ip.is_some() || s.mode == Mode::Off);
            if online && drawn >= CONFIRM_FRAMES {
                firmware::confirm_image().await;
            } else if now - boot > CONFIRM_BY {
                firmware::roll_back_now().await;
            }
        }
        let dt_ms = (now - last).as_millis() as u32;
        last = now;

        if let Some(v) = state::VOLUME.try_take() {
            let _ = audio::set_volume(&mut i2c, v);
        }
        let tick = remind::tick(now.as_millis(), state::local_time_s());
        if tick.save {
            persist::save().await;
        }
        if let Some(sound) = tick.sound {
            state::SOUND.signal(sound);
        }
        let now_ms = now.as_millis();
        let mut fx = settings::tick(now_ms);
        // Settings open as soon as the hold is long enough; a press released
        // before that is a short one.
        let mut key_short = false;
        match (key.is_low(), key_since) {
            (true, None) => key_since = Some(now),
            (true, Some(since)) if !key_held && now - since >= KEY_HOLD => {
                key_held = true;
                fx = settings::toggle(now_ms);
            }
            (false, Some(_)) => {
                key_short = !key_held;
                (key_since, key_held) = (None, false);
            }
            _ => {}
        }
        match state::KEY.try_take() {
            Some(true) => fx = settings::toggle(now_ms),
            Some(false) => key_short = true,
            None => {}
        }
        if key_short && settings::is_open() {
            fx = settings::close();
            key_short = false;
        }
        let pressed = pmu::take_short_press(&mut i2c);
        if remind::current().is_some() && settings::is_open() {
            // A reminder takes the screen.
            fx = settings::close();
        }
        if pressed && remind::snooze(now_ms) {
            state::SOUND.signal(Sound::Click);
        } else if pressed && settings::is_open() {
            fx = settings::close();
            state::SOUND.signal(Sound::Click);
        } else if pressed {
            let next = state::APP.lock().await.next_name();
            info!("button: next app {next}");
            state::SWITCH_APP.signal(next);
            state::config(|c| {
                let _ = c.set_app(next);
            });
            persist::save().await;
            state::SOUND.signal(Sound::Click);
        }
        if let Some(name) = state::SWITCH_APP.try_take() {
            if state::APP.lock().await.switch_to(name) {
                info!("app: switched to {name}");
            }
        }

        let (mut x_mg, mut y_mg, mut z_mg) = imu::read_mg(&mut i2c).unwrap_or((0, 0, 1000));
        let mut point = touch::read(&mut i2c);
        let ovr = state::with(|s| {
            let o = s.ovr;
            s.ovr.tap = false;
            o
        });
        let overriding = now.as_millis() < ovr.until_ms;
        if overriding {
            if let Some(t) = ovr.tilt {
                (x_mg, y_mg, z_mg) = (t.x_mg, t.y_mg, t.z_mg);
            }
            if ovr.touch.is_some() {
                point = ovr.touch;
            }
        }
        let tapped = ovr.tap
            || match (point, press_start) {
                (Some(_), None) => {
                    press_start = Some(now);
                    false
                }
                (None, Some(start)) => {
                    press_start = None;
                    (now - start).as_millis() < TAP_MS
                }
                _ => false,
            };
        if let Some(p) = point {
            last_touch = p;
        }
        // A tap on a reminder dismisses it and does not reach the app.
        let tapped = tapped && !remind::dismiss();
        let tapped = if tapped && settings::is_open() {
            fx = settings::tap(last_touch.0, last_touch.1, now_ms);
            false
        } else {
            tapped
        };
        if let Some(b) = fx.brightness {
            lcd.set_brightness(b);
        }
        if let Some(v) = fx.volume {
            let _ = audio::set_volume(&mut i2c, v);
        }
        if let Some(sound) = fx.sound {
            state::SOUND.signal(sound);
        }
        if fx.save {
            persist::save().await;
        }

        let input = Input {
            tilt: Tilt { x_mg, y_mg, z_mg },
            touch: Touch { down: point.is_some(), x: last_touch.0, y: last_touch.1, tapped },
            time_ms: (now - boot).as_millis() as u32,
            local_time_s: state::local_time_s(),
            key: key_short && remind::current().is_none(),
        };

        let lines = status_lines(now.as_millis());
        let lines: alloc::vec::Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut out = Output::default();
        let showing = remind::current();
        let in_settings = settings::is_open();
        {
            // The app pauses while a reminder or settings covers the screen.
            let mut app = state::APP.lock().await;
            if showing.is_none() && !in_settings {
                app.update(&input, dt_ms, &mut out);
            }
            lcd.frame(|rows, buf| {
                match &showing {
                    Some((a, shown)) => alert::render(rows.clone(), buf, a, *shown, now_ms),
                    None if in_settings => settings::render(rows.clone(), buf),
                    None => app.render(rows.clone(), buf),
                }
                overlay::draw(rows, buf, &lines);
            });
        }
        if let Some(sound) = out.sound {
            state::SOUND.signal(sound);
        }

        frames += 1;
        drawn = drawn.saturating_add(1);
        if (now - stats_at).as_secs() >= 5 {
            let ms = (now - stats_at).as_millis() as u32;
            let fps = frames * 1000 / ms;
            state::with(|s| s.fps = fps);
            info!(
                "fps={fps} tilt=({x_mg},{y_mg},{z_mg}) touch={point:?} frame={}ms heap_free={}",
                (Instant::now() - now).as_millis(),
                esp_alloc::HEAP.free()
            );
            frames = 0;
            stats_at = now;
        }

        let spent = Instant::now() - now;
        Timer::after(FRAME.checked_sub(spent).unwrap_or(Duration::from_millis(1))).await;
    }
}

/// Text to draw over the app this frame, if any.
fn status_lines(now_ms: u64) -> alloc::vec::Vec<alloc::string::String> {
    use alloc::format;
    state::with(|s| match s.mode {
        Mode::Station if now_ms < s.show_ip_until_ms => match s.ip {
            Some(ip) => alloc::vec![
                format!("{}", s.ssid().unwrap_or("")),
                format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
                "espc6.local".into(),
            ],
            None => alloc::vec![],
        },
        _ => alloc::vec![],
    })
}

#[embassy_executor::task]
async fn sound_task(mut tx: I2sTx<'static, Async>, mut buf: DmaTxStreamBuf) {
    loop {
        let sound: Sound = state::SOUND.wait().await;
        let mut voice = audio::Voice::new(sound);
        buf.push_with(|b| voice.fill(b));
        let mut transfer = match tx.write(buf) {
            Ok(t) => t,
            Err((e, t, b)) => {
                warn!("audio: {e:?}");
                (tx, buf) = (t, fresh(b));
                continue;
            }
        };
        // Top the ring up as chunks drain, by polling: waiting on the DMA's
        // "chunk done" interrupt clears all its flags, so if the ring ran dry
        // (a flash erase can stall the CPU that long) the final "all done"
        // would be lost and the task would wait forever. A stall now just
        // cuts the sound short.
        while !voice.done() && !transfer.is_done() {
            if transfer.available_bytes() > 0 {
                transfer.push_with(|b| voice.fill(b));
            } else {
                Timer::after(Duration::from_millis(5)).await;
            }
        }
        if !voice.done() {
            warn!("audio: ran dry, sound cut short");
        } else if sound == Sound::Chime {
            info!("audio: chime played");
        }
        let (res, t, b) = transfer.wait_async().await;
        if let Err(e) = res {
            warn!("audio: {e:?}");
        }
        (tx, buf) = (t, fresh(b));
    }
}

/// A stream buffer remembers how much was pushed before its first transfer;
/// rebuild it so the next sound starts clean.
fn fresh(buf: DmaTxStreamBuf) -> DmaTxStreamBuf {
    let (descriptors, buffer) = buf.split();
    DmaTxStreamBuf::new(descriptors, buffer).unwrap()
}

#[embassy_executor::task]
async fn button_task(btn: Pin<'static>) {
    loop {
        if btn.is_low() {
            let start = Instant::now();
            while btn.is_low() {
                if Instant::now() - start >= SETUP_HOLD {
                    info!("button: held, entering setup mode");
                    net::request_setup_and_reboot();
                }
                Timer::after(Duration::from_millis(50)).await;
            }
        }
        Timer::after(Duration::from_millis(50)).await;
    }
}
