//! Wi-Fi bring-up and the network tasks.
//!
//! At boot the device picks one mode and keeps it until the next reboot:
//! - Station: join the strongest saved network. Serves the HTTP API,
//!   answers mDNS for `espc6.local` and keeps the clock synced over SNTP.
//! - Setup: an open access point `glyph-setup-XXXX` at 192.168.4.1 with a
//!   DHCP server, a DNS server that points every name at the device (so phones
//!   open the setup page), and the setup form.
//!
//! Wi-Fi can be switched off in settings: the station task then disconnects
//! and waits, without scanning, until it is switched back on.
//!
//! Setup mode starts only when no network is saved, or when asked for (BOOT
//! held 3 s, or `POST /setup`). With saved networks the device keeps looking
//! for them, however long it is out of range, and the apps and reminders keep
//! running meanwhile. Setup mode reboots back to station mode after
//! `SETUP_TIMEOUT`, in case it was entered by mistake.

pub mod http;
mod udp;

use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_net::{Ipv4Address, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4};
use embassy_time::{Duration, Instant, Timer};
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::scan::ScanConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Config, Interface, WifiController};
use log::{info, warn};
use static_cell::StaticCell;

use crate::state::{self, Mode};

pub const HOSTNAME: &str = "espc6";
pub const SETUP_IP: [u8; 4] = [192, 168, 4, 1];
/// Scan every `RETRY_FAST` for this long after losing the network, then
/// every `RETRY_SLOW`.
const RETRY_FAST_FOR: Duration = Duration::from_secs(60);
const RETRY_FAST: Duration = Duration::from_secs(3);
const RETRY_SLOW: Duration = Duration::from_secs(20);
const SETUP_TIMEOUT: Duration = Duration::from_secs(600);

/// Survives a software reset, so station mode can hand over to setup mode.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut BOOT_INTO_SETUP: u32 = 0;
const SETUP_MAGIC: u32 = 0x5E7_0B00;

pub fn request_setup_and_reboot() -> ! {
    unsafe { BOOT_INTO_SETUP = SETUP_MAGIC };
    esp_hal::system::software_reset()
}

fn take_setup_request() -> bool {
    unsafe {
        let set = BOOT_INTO_SETUP == SETUP_MAGIC;
        BOOT_INTO_SETUP = 0;
        set
    }
}

pub fn start(
    spawner: Spawner,
    wifi: esp_hal::peripherals::WIFI<'static>,
    seed: u64,
    mac_suffix: u16,
) {
    static CONTROLLER: StaticCell<WifiController<'static>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<{ http::SERVERS + 5 }>> = StaticCell::new();

    let controller = CONTROLLER.init(
        WifiController::new(wifi, Default::default()).expect("wifi controller"),
    );
    let has_networks = state::config(|c| c.networks().next().is_some());
    let wifi_off = state::config(|c| c.wifi_off());
    let setup = take_setup_request() || (!has_networks && !wifi_off);

    if setup {
        state::with(|s| s.mode = Mode::Setup);
        let mut ssid_buf = [0u8; 32];
        let ssid = setup_ssid(&mut ssid_buf, mac_suffix);
        let ap = AccessPointConfig::default()
            .with_ssid(ssid.try_into().unwrap())
            .with_channel(6)
            .with_authentication(AuthenticationMethodConfig::Open);
        controller.set_config(&Config::AccessPoint(ap)).expect("ap config");
        state::with(|s| {
            s.set_ssid(ssid);
            s.ip = Some(SETUP_IP);
        });
        info!("wifi: setup mode, join '{ssid}' and open http://192.168.4.1");

        let net = embassy_net::Config::ipv4_static(StaticConfigV4 {
            address: Ipv4Cidr::new(Ipv4Address::from(SETUP_IP), 24),
            gateway: None,
            dns_servers: Default::default(),
        });
        let (stack, runner) = embassy_net::new(
            Interface::access_point(),
            net,
            RESOURCES.init(StackResources::new()),
            seed,
        );
        spawner.spawn(net_runner(runner).unwrap());
        spawner.spawn(udp::dhcp_server(stack).unwrap());
        spawner.spawn(udp::captive_dns(stack).unwrap());
        spawner.spawn(setup_timeout().unwrap());
        http::spawn(spawner, stack, true);
    } else {
        state::with(|s| s.mode = Mode::Connecting);
        let (stack, runner) = embassy_net::new(
            Interface::station(),
            embassy_net::Config::dhcpv4(Default::default()),
            RESOURCES.init(StackResources::new()),
            seed,
        );
        spawner.spawn(net_runner(runner).unwrap());
        spawner.spawn(station(controller, stack).unwrap());
        spawner.spawn(udp::mdns(stack).unwrap());
        spawner.spawn(udp::sntp(stack).unwrap());
        http::spawn(spawner, stack, false);
    }
}

fn setup_ssid(buf: &mut [u8; 32], suffix: u16) -> &str {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let prefix = b"glyph-setup-";
    buf[..prefix.len()].copy_from_slice(prefix);
    for i in 0..4 {
        buf[prefix.len() + i] = HEX[((suffix >> (12 - 4 * i)) & 0xF) as usize];
    }
    core::str::from_utf8(&buf[..prefix.len() + 4]).unwrap()
}

#[embassy_executor::task]
async fn net_runner(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn setup_timeout() {
    Timer::after(SETUP_TIMEOUT).await;
    info!("wifi: setup mode timed out, rebooting to try saved networks");
    esp_hal::system::software_reset();
}

/// Join the strongest saved network, then stay connected.
#[embassy_executor::task]
async fn station(controller: &'static mut WifiController<'static>, stack: Stack<'static>) {
    let mut offline_since = Instant::now();
    controller
        .set_config(&Config::Station(StationConfig::default()))
        .expect("station config");

    loop {
        if wifi_off() {
            info!("wifi: off");
            state::with(|s| {
                s.mode = Mode::Off;
                s.ip = None;
                s.rssi = None;
            });
            while wifi_off() {
                state::WIFI_CHANGED.wait().await;
            }
            info!("wifi: on");
            state::with(|s| s.mode = Mode::Connecting);
            offline_since = Instant::now();
        }
        match join_best(controller).await {
            Some(ssid) => {
                state::with(|s| s.set_ssid(&ssid));
                stack.wait_config_up().await;
                if let Some(cfg) = stack.config_v4() {
                    let ip = cfg.address.address().octets();
                    info!("ip={}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
                    state::with(|s| {
                        s.mode = Mode::Station;
                        s.ip = Some(ip);
                        s.show_ip_until_ms = state::now_ms() + 8000;
                    });
                }
                // Stay here until the link drops, refreshing RSSI.
                loop {
                    let rssi = controller.rssi().ok();
                    state::with(|s| s.rssi = rssi);
                    if !controller.is_connected() || wifi_off() {
                        break;
                    }
                    select(Timer::after(Duration::from_secs(5)), state::WIFI_CHANGED.wait()).await;
                }
                if wifi_off() {
                    let _ = controller.disconnect_async().await;
                } else {
                    warn!("wifi: disconnected");
                }
                offline_since = Instant::now();
                state::with(|s| {
                    s.mode = Mode::Connecting;
                    s.ip = None;
                    s.rssi = None;
                });
            }
            None => {
                let fast = Instant::now() - offline_since < RETRY_FAST_FOR;
                let wait = Timer::after(if fast { RETRY_FAST } else { RETRY_SLOW });
                select(wait, state::WIFI_CHANGED.wait()).await;
            }
        }
    }
}

fn wifi_off() -> bool {
    state::config(|c| c.wifi_off())
}

async fn join_best(controller: &mut WifiController<'static>) -> Option<alloc::string::String> {
    let seen = match controller.scan_async(&ScanConfig::default().with_max(20)).await {
        Ok(seen) => seen,
        Err(e) => {
            warn!("wifi: scan failed: {e:?}");
            return None;
        }
    };
    // Saved networks that are in range, strongest first.
    let mut best: Option<(i8, alloc::string::String, alloc::string::String)> = None;
    state::config(|c| {
        for net in c.networks() {
            if let Some(ap) = seen.iter().find(|ap| ap.ssid.as_str() == net.ssid()) {
                if best.as_ref().is_none_or(|(rssi, _, _)| ap.signal_strength > *rssi) {
                    best = Some((ap.signal_strength, net.ssid().into(), net.password().into()));
                }
            }
        }
    });
    let Some((rssi, ssid, pass)) = best else {
        info!("wifi: no saved network in range");
        return None;
    };

    let auth = if pass.is_empty() {
        AuthenticationMethodConfig::Open
    } else {
        AuthenticationMethodConfig::Wpa2Personal(pass.as_str().try_into().ok()?)
    };
    let sta = StationConfig::default()
        .with_ssid(ssid.as_str().try_into().ok()?)
        .with_authentication(auth);
    controller.set_config(&Config::Station(sta)).ok()?;
    info!("wifi: joining '{ssid}' ({rssi} dBm)");
    match controller.connect_async().await {
        Ok(_) => Some(ssid),
        Err(e) => {
            warn!("wifi: join '{ssid}' failed: {e:?}");
            None
        }
    }
}
