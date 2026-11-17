use super::*;
#[test]
fn http_requests() {
    assert!(http::parse_request(b"GET / HTTP/1.1\r\n")
        .unwrap()
        .is_none());
    let b=b"POST /wifi?a=1 HTTP/1.1\r\ncOnTeNt-LeNgTh: 3\r\nAUTHORIZATION: Bearer abc\r\nContent-Type: text/plain\r\n\r\nxyz";
    let r = http::parse_request(b).unwrap().unwrap();
    assert_eq!((r.method, r.path, r.query), ("POST", "/wifi", "a=1"));
    assert_eq!(r.content_length, Some(3));
    assert_eq!(r.bearer, Some("abc"));
    assert_eq!(r.content_type, Some("text/plain"));
    assert_eq!(&b[r.header_len..], b"xyz");
    for b in [
        b"bad\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
        b"GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
    ] {
        assert!(http::parse_request(b).is_err());
    }
    assert_eq!(http::param("a=1&b=two=2", "b"), Some("two=2"));
    assert_eq!(http::param("abc=1", "a"), None);
    let mut out = [0; 128];
    assert_eq!(http::url_decode("a+%C3%A9", &mut out), Some("a é"));
    for s in ["%", "%GG", "%FF"] {
        assert!(http::url_decode(s, &mut out).is_none());
    }
    assert!(http::url_decode("abc", &mut [0; 2]).is_none());
    let mut out = [0; 256];
    let n = http::write_response(&mut out, 200, "text/plain", 123);
    let s = core::str::from_utf8(&out[..n]).unwrap();
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(s.contains("Content-Length: 123\r\n"));
    assert!(s.ends_with("\r\n\r\n"));
    assert_eq!(http::write_response(&mut [0; 2], 200, "x", 0), 0);
}
fn question(kind: u16) -> std::vec::Vec<u8> {
    let mut q = std::vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    q.extend_from_slice(b"\x06espc6x\x05local\0");
    q.extend_from_slice(&kind.to_be_bytes());
    q.extend_from_slice(&[0, 1]);
    q
}
#[test]
fn dns_and_mdns() {
    let mut out = [0; 512];
    let q = question(1);
    let n = dns::captive_reply(&q, [1, 2, 3, 4], &mut out).unwrap();
    assert_eq!(&out[n - 4..n], &[1, 2, 3, 4]);
    assert_eq!(out[7], 1);
    assert_eq!(&out[12..q.len()], &q[12..]);
    dns::captive_reply(&question(28), [0; 4], &mut out).unwrap();
    assert_eq!(out[7], 0);
    assert!(mdns::reply(&q, "ESPC6X", [1; 4], &mut out).is_some());
    assert!(mdns::reply(&q, "other", [1; 4], &mut out).is_none());
    let mut compressed = q.clone();
    compressed[5] = 2;
    compressed.extend_from_slice(&[0xc0, 12, 0, 1, 0x80, 1]);
    assert!(mdns::reply(&compressed, "espc6x", [1; 4], &mut out).is_some());
    assert!(mdns::announce("espc6x", [2; 4], &mut out).is_some());
    assert_eq!(&out[..4], &[0, 0, 0x84, 0]);
    assert!(mdns::announce("", [0; 4], &mut out).is_none());
    for len in 0..q.len() {
        assert!(dns::captive_reply(&q[..len], [0; 4], &mut out).is_none());
        assert!(mdns::reply(&q[..len], "espc6x", [0; 4], &mut out).is_none());
    }
    let mut cycle = question(1);
    cycle[12] = 0xc0;
    cycle[13] = 12;
    assert!(dns::captive_reply(&cycle, [0; 4], &mut out).is_none());
}
#[test]
fn dhcp_packets() {
    let mut s = dhcp::DhcpServer::new([192, 168, 4, 1]);
    let mut q = [0; 250];
    q[0] = 1;
    q[1] = 1;
    q[2] = 6;
    q[4..8].copy_from_slice(&[1, 2, 3, 4]);
    q[10] = 0x80;
    q[28..34].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    q[236..240].copy_from_slice(&[99, 130, 83, 99]);
    q[240..244].copy_from_slice(&[53, 1, 1, 255]);
    let mut out = [0; 512];
    let n = s.handle(&q, &mut out).unwrap();
    assert!(n > 240);
    assert_eq!(out[0], 2);
    assert_eq!(&out[4..8], &q[4..8]);
    assert_eq!(&out[28..34], &q[28..34]);
    assert_eq!(&out[16..20], &[192, 168, 4, 2]);
    assert_eq!(&out[240..243], &[53, 1, 2]);
    assert_eq!(&out[243..249], &[54, 4, 192, 168, 4, 1]);
    assert_eq!(&out[249..255], &[51, 4, 0, 0, 14, 16]);
    q[242] = 3;
    q[243..250].copy_from_slice(&[50, 4, 10, 0, 0, 1, 255]);
    s.handle(&q, &mut out).unwrap();
    assert_eq!(out[242], 6);
    q[245..249].copy_from_slice(&[192, 168, 4, 3]);
    s.handle(&q, &mut out).unwrap();
    assert_eq!(out[242], 5);
    assert_eq!(&out[16..20], &[192, 168, 4, 3]);
    assert!(s.handle(&q, &mut [0; 20]).is_none());
    for len in 0..240 {
        assert!(s.handle(&q[..len], &mut out).is_none());
    }
}
#[test]
fn config_storage() {
    let mut c = config::Config::new();
    assert_eq!(c.app(), "glyphfield");
    assert_eq!(c.volume(), 60);
    assert_eq!(c.token(), "");
    for s in ["a", "b", "c", "d", "e"] {
        c.add_network(s, "pass").unwrap();
    }
    assert_eq!(
        c.networks().map(|n| n.ssid()).collect::<std::vec::Vec<_>>(),
        ["e", "d", "c", "b"]
    );
    c.add_network("c", "new").unwrap();
    assert_eq!(c.networks().next().unwrap().password(), "new");
    assert!(c.forget("d"));
    assert!(!c.forget("d"));
    assert!(c.add_network("", "x").is_err());
    c.set_token_from_random([0xab; 16]);
    assert_eq!(c.token(), "abababababababababababababababab");
    c.set_app("hello").unwrap();
    c.set_volume(255);
    assert_eq!(c.volume(), 100);
    let mut b = [0; config::ENCODED_LEN];
    c.encode(&mut b);
    let d = config::Config::decode(&b).unwrap();
    assert_eq!(d.app(), "hello");
    assert_eq!(d.token(), c.token());
    assert_eq!(d.networks().count(), 3);
    b[10] ^= 1;
    assert!(config::Config::decode(&b).is_none());
    assert!(config::Config::decode(&[255; 4096]).is_none());
}
#[test]
fn dhcp_oldest_and_reuse() {
    let mut s = dhcp::DhcpServer::new([192, 168, 4, 1]);
    let mut q = [0; 244];
    q[0] = 1;
    q[1] = 1;
    q[2] = 6;
    q[236..240].copy_from_slice(&[99, 130, 83, 99]);
    q[240..].copy_from_slice(&[53, 1, 1, 255]);
    let mut out = [0; 512];
    for mac in 1..=8 {
        q[28] = mac;
        s.handle(&q, &mut out).unwrap();
        assert_eq!(out[19], mac + 1);
    }
    q[28] = 1;
    s.handle(&q, &mut out).unwrap();
    assert_eq!(out[19], 2);
    q[28] = 9;
    s.handle(&q, &mut out).unwrap();
    assert_eq!(out[19], 3);
    q[28] = 10;
    s.handle(&q, &mut out).unwrap();
    assert_eq!(out[19], 4);
}
#[test]
fn malformed_and_output_boundaries() {
    let mut out = [0; 1024];
    assert!(matches!(
        http::parse_request(&[b'x'; 8192]),
        Err(http::ParseError::TooLarge)
    ));
    for bad in [
        b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        b"GET / HTTP/1.1\r\nx: \xff\r\n\r\n",
    ] {
        assert!(http::parse_request(bad).is_err());
    }
    let q = question(255);
    let n = dns::captive_reply(&q, [1; 4], &mut out).unwrap();
    for size in 0..n {
        assert!(dns::captive_reply(&q, [1; 4], &mut out[..size]).is_none());
    }
    let n = mdns::announce("espc6x", [1; 4], &mut out).unwrap();
    for size in 0..n {
        assert!(mdns::announce("espc6x", [1; 4], &mut out[..size]).is_none());
    }
    let mut q = question(1);
    q[2] |= 0x80;
    assert!(dns::captive_reply(&q, [0; 4], &mut out).is_none());
    assert!(mdns::reply(&q, "espc6x", [0; 4], &mut out).is_none());
    // The only matching question is compressed, referring to a preceding question.
    let mut q = question(28);
    q[5] = 2;
    q.extend_from_slice(&[0xc0, 12, 0, 1, 0x80, 1]);
    assert!(mdns::reply(&q, "espc6x", [0; 4], &mut out).is_some());
    let c = config::Config::new();
    let mut b = [0; config::ENCODED_LEN];
    c.encode(&mut b);
    for size in 0..b.len() {
        assert!(config::Config::decode(&b[..size]).is_none());
    }
}
#[test]
fn config_limits_and_unicode() {
    let mut c = config::Config::new();
    assert!(c.add_network(&"a".repeat(33), "").is_err());
    assert!(c.add_network("a", &"b".repeat(65)).is_err());
    assert!(c.set_app(&"a".repeat(17)).is_err());
    assert!(c.set_app("").is_err());
    c.add_network(&"é".repeat(16), &"é".repeat(32)).unwrap();
    let mut b = [0; config::ENCODED_LEN];
    c.encode(&mut b);
    let d = config::Config::decode(&b).unwrap();
    assert_eq!(d.networks().next().unwrap().ssid(), "é".repeat(16));
    assert_eq!(d.networks().next().unwrap().password(), "é".repeat(32));
    assert!(c.forget(&"é".repeat(16)));
    assert_eq!(c.networks().count(), 0);
}

#[test]
fn arbitrary_input_is_panic_free() {
    let mut state = 0x12345678u32;
    let mut data = [0u8; 512];
    let mut output = [0u8; 512];
    let mut server = dhcp::DhcpServer::new([192, 168, 4, 1]);
    for len in 0..512 {
        for byte in &mut data {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        let input = &data[..len];
        let _ = http::parse_request(input);
        let _ = dns::captive_reply(input, [1; 4], &mut output);
        let _ = mdns::reply(input, "espc6x", [1; 4], &mut output);
        let _ = server.handle(input, &mut output);
        let _ = config::Config::decode(input);
        let _ = tz::decode(input);
        let _ = core::str::from_utf8(input).ok().and_then(tz::parse);
    }
}

#[test]
fn sntp_request_and_reply() {
    use crate::sntp;
    let req = sntp::request();
    assert_eq!(req[0], 0x23);
    assert!(req[1..].iter().all(|b| *b == 0));

    let mut reply = [0u8; sntp::LEN];
    reply[0] = 0x24; // leap 0, version 4, mode 4
    reply[1] = 2;
    // 2026-10-26 00:00:00 UTC = 1792972800 Unix = 4001961600 NTP.
    reply[40..44].copy_from_slice(&4_001_961_600u32.to_be_bytes());
    assert_eq!(sntp::parse_reply(&reply), Some(1_792_972_800));
    // After the 2036 NTP era rollover, small NTP seconds still map forward.
    reply[40..44].copy_from_slice(&100u32.to_be_bytes());
    assert_eq!(sntp::parse_reply(&reply), Some(2_085_978_596));

    for (byte, value) in [(0, 0x23), (0, 0xE4), (1, 0), (1, 16)] {
        let mut bad = reply;
        bad[byte] = value;
        assert_eq!(sntp::parse_reply(&bad), None, "byte {byte} = {value:#x}");
    }
    let mut zero = reply;
    zero[40..44].fill(0);
    assert_eq!(sntp::parse_reply(&zero), None);
    assert_eq!(sntp::parse_reply(&reply[..47]), None);
}
