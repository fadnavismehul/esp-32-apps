#[derive(Clone, Copy)]
struct Lease {
    mac: [u8; 6],
    used: bool,
}
/// Eight leases in most-recently-assigned order; no clock is required.
pub struct DhcpServer {
    server_ip: [u8; 4],
    leases: [Lease; 8],
    order: [usize; 8],
}
impl DhcpServer {
    pub const fn new(server_ip: [u8; 4]) -> Self {
        Self {
            server_ip,
            order: [0, 1, 2, 3, 4, 5, 6, 7],
            leases: [Lease {
                mac: [0; 6],
                used: false,
            }; 8],
        }
    }
    pub fn handle(&mut self, request: &[u8], out: &mut [u8]) -> Option<usize> {
        let fixed = request.get(..240)?;
        if fixed[0] != 1 || fixed[1] != 1 || fixed[2] != 6 || fixed[236..240] != [99, 130, 83, 99] {
            return None;
        }
        let mut msg = None;
        let mut requested = None;
        let mut server = None;
        let mut pos = 240;
        let mut ended = false;
        while let Some(&code) = request.get(pos) {
            pos += 1;
            if code == 255 {
                ended = true;
                break;
            }
            if code == 0 {
                continue;
            }
            let len = *request.get(pos)? as usize;
            pos += 1;
            let data = request.get(pos..pos.checked_add(len)?)?;
            pos += len;
            match code {
                53 => {
                    if len != 1 || msg.is_some() {
                        return None;
                    }
                    msg = Some(data[0]);
                }
                50 => {
                    if len != 4 || requested.is_some() {
                        return None;
                    }
                    requested = Some(<[u8; 4]>::try_from(data).ok()?);
                }
                54 => {
                    if len != 4 || server.is_some() {
                        return None;
                    }
                    server = Some(<[u8; 4]>::try_from(data).ok()?);
                }
                _ => {}
            }
        }
        if !ended || !matches!(msg, Some(1 | 3)) || server.is_some_and(|s| s != self.server_ip) {
            return None;
        }
        let mac: [u8; 6] = fixed[28..34].try_into().ok()?;
        if requested.is_none() && fixed[12..16] != [0; 4] {
            requested = Some(fixed[12..16].try_into().ok()?)
        }
        let invalid = msg == Some(3)
            && requested.is_some_and(|a| a[..3] != self.server_ip[..3] || !(2..=9).contains(&a[3]));
        let dst = out.get_mut(..274)?; // Fixed BOOTP header and 34 bytes of options.
        let mut ip = [0; 4];
        if !invalid {
            let existing = self.leases.iter().position(|l| l.used && l.mac == mac);
            let index = if let Some(a) = requested.filter(|_| msg == Some(3)) {
                (a[3] - 2) as usize
            } else {
                existing.unwrap_or_else(|| {
                    self.leases
                        .iter()
                        .position(|l| !l.used)
                        .unwrap_or_else(|| self.order[0])
                })
            };
            // Move the assigned slot to the newest end of a fixed permutation.
            if let Some(position) = self.order.iter().position(|&slot| slot == index) {
                for p in position..7 {
                    self.order[p] = self.order[p + 1];
                }
                self.order[7] = index;
            }
            if let Some(old) = existing {
                self.leases[old].used = false;
            }
            ip = self.server_ip;
            ip[3] = index as u8 + 2;
            self.leases[index] = Lease { mac, used: true };
        }
        dst.fill(0);
        dst[0] = 2;
        dst[1] = 1;
        dst[2] = 6;
        dst[4..8].copy_from_slice(&fixed[4..8]);
        dst[10..12].copy_from_slice(&fixed[10..12]);
        dst[16..20].copy_from_slice(&ip);
        dst[20..24].copy_from_slice(&self.server_ip);
        dst[28..44].copy_from_slice(&fixed[28..44]);
        dst[236..240].copy_from_slice(&[99, 130, 83, 99]);
        let kind = if invalid {
            6
        } else if msg == Some(1) {
            2
        } else {
            5
        };
        dst[240..243].copy_from_slice(&[53, 1, kind]);
        dst[243..249].copy_from_slice(&[
            54,
            4,
            self.server_ip[0],
            self.server_ip[1],
            self.server_ip[2],
            self.server_ip[3],
        ]);
        dst[249..255].copy_from_slice(&[51, 4, 0, 0, 14, 16]);
        dst[255..261].copy_from_slice(&[1, 4, 255, 255, 255, 0]);
        dst[261..267].copy_from_slice(&[
            3,
            4,
            self.server_ip[0],
            self.server_ip[1],
            self.server_ip[2],
            self.server_ip[3],
        ]);
        dst[267..273].copy_from_slice(&[
            6,
            4,
            self.server_ip[0],
            self.server_ip[1],
            self.server_ip[2],
            self.server_ip[3],
        ]);
        dst[273] = 255;
        Some(274)
    }
}
