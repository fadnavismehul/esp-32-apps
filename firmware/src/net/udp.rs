//! Small UDP services. Packet logic lives in `netproto` (host-tested); these
//! tasks only move bytes.

use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::dns::DnsQueryType;
use embassy_net::{IpAddress, IpEndpoint, Ipv4Address, Stack};
use embassy_time::{Duration, Timer, with_timeout};
use log::{info, warn};
use netproto::{dhcp::DhcpServer, dns, mdns, sntp};

use super::{HOSTNAME, SETUP_IP};
use crate::state;

const MDNS_ADDR: Ipv4Address = Ipv4Address::new(224, 0, 0, 251);

macro_rules! socket {
    ($stack:expr, $rx:literal, $tx:literal) => {{
        static mut RX_META: [PacketMetadata; 4] = [PacketMetadata::EMPTY; 4];
        static mut TX_META: [PacketMetadata; 4] = [PacketMetadata::EMPTY; 4];
        static mut RX: [u8; $rx] = [0; $rx];
        static mut TX: [u8; $tx] = [0; $tx];
        // Each task runs once, so these buffers are never shared.
        #[allow(static_mut_refs)]
        unsafe {
            UdpSocket::new($stack, &mut RX_META, &mut RX, &mut TX_META, &mut TX)
        }
    }};
}

#[embassy_executor::task]
pub async fn dhcp_server(stack: Stack<'static>) {
    let mut socket = socket!(stack, 1024, 1024);
    socket.bind(67).unwrap();
    let mut server = DhcpServer::new(SETUP_IP);
    let mut req = [0u8; 600];
    let mut reply = [0u8; 600];
    loop {
        let Ok((n, _)) = socket.recv_from(&mut req).await else { continue };
        if let Some(len) = server.handle(&req[..n], &mut reply) {
            let to = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::BROADCAST), 68);
            if let Err(e) = socket.send_to(&reply[..len], to).await {
                warn!("dhcp: send failed: {e:?}");
            }
        }
    }
}

#[embassy_executor::task]
pub async fn captive_dns(stack: Stack<'static>) {
    let mut socket = socket!(stack, 1024, 1024);
    socket.bind(53).unwrap();
    let mut query = [0u8; 512];
    let mut reply = [0u8; 512];
    loop {
        let Ok((n, from)) = socket.recv_from(&mut query).await else { continue };
        if let Some(len) = dns::captive_reply(&query[..n], SETUP_IP, &mut reply) {
            let _ = socket.send_to(&reply[..len], from.endpoint).await;
        }
    }
}

#[embassy_executor::task]
pub async fn mdns(stack: Stack<'static>) {
    stack.wait_config_up().await;
    if let Err(e) = stack.join_multicast_group(MDNS_ADDR) {
        warn!("mdns: join failed: {e:?}");
        return;
    }
    let mut socket = socket!(stack, 1536, 512);
    socket.bind(5353).unwrap();
    let group = IpEndpoint::new(IpAddress::Ipv4(MDNS_ADDR), 5353);
    let mut packet = [0u8; 1500];
    let mut reply = [0u8; 256];

    let ip = || state::with(|s| s.ip);
    if let Some(ip) = ip() {
        // Announce twice so caches pick up the new address.
        for _ in 0..2 {
            if let Some(len) = mdns::announce(HOSTNAME, ip, &mut reply) {
                let _ = socket.send_to(&reply[..len], group).await;
            }
            Timer::after(Duration::from_secs(1)).await;
        }
        info!("mdns: answering {HOSTNAME}.local");
    }

    loop {
        let Ok((n, _)) = socket.recv_from(&mut packet).await else { continue };
        let Some(ip) = ip() else { continue };
        if let Some(len) = mdns::reply(&packet[..n], HOSTNAME, ip, &mut reply) {
            let _ = socket.send_to(&reply[..len], group).await;
        }
    }
}

const NTP_HOST: &str = "pool.ntp.org";
const RESYNC: Duration = Duration::from_secs(3600);
const RETRY: Duration = Duration::from_secs(15);

/// Set the wall clock from SNTP once the network is up, then hourly.
#[embassy_executor::task]
pub async fn sntp(stack: Stack<'static>) {
    stack.wait_config_up().await;
    let mut socket = socket!(stack, 128, 128);
    socket.bind(0).unwrap();
    let mut reply = [0u8; 128];
    loop {
        let server = match stack.dns_query(NTP_HOST, DnsQueryType::A).await {
            Ok(addrs) if !addrs.is_empty() => addrs[0],
            other => {
                warn!("sntp: resolving {NTP_HOST} failed: {other:?}");
                Timer::after(RETRY).await;
                continue;
            }
        };
        let to = IpEndpoint::new(server, sntp::PORT);
        let synced = async {
            socket.send_to(&sntp::request(), to).await.ok()?;
            loop {
                let (n, from) = socket.recv_from(&mut reply).await.ok()?;
                if from.endpoint == to {
                    return sntp::parse_reply(&reply[..n]);
                }
            }
        };
        match with_timeout(Duration::from_secs(5), synced).await {
            Ok(Some(unix)) => {
                state::with(|s| s.synced = Some((unix, state::now_ms())));
                info!("sntp: synced, unix={unix}");
                Timer::after(RESYNC).await;
            }
            _ => {
                warn!("sntp: no reply from {server}");
                Timer::after(RETRY).await;
            }
        }
    }
}
