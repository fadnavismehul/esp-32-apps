pub(crate) fn u16_at(p: &[u8], n: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        p.get(n..n.checked_add(2)?)?.try_into().ok()?,
    ))
}
// Decode a possibly compressed name, bounding both expansion and pointer traversal.
pub(crate) fn name(p: &[u8], start: usize, out: &mut [u8; 255]) -> Option<(usize, usize)> {
    let mut pos = start;
    let mut end = None;
    let mut n = 0;
    for _ in 0..256 {
        let b = *p.get(pos)?;
        if b & 0xc0 == 0xc0 {
            let lo = *p.get(pos + 1)?;
            let dest = (((b & 63) as usize) << 8) | lo as usize;
            if dest >= pos {
                return None;
            }
            if end.is_none() {
                end = Some(pos + 2)
            }
            pos = dest;
            continue;
        }
        if b & 0xc0 != 0 {
            return None;
        }
        pos += 1;
        if b == 0 {
            return Some((end.unwrap_or(pos), n));
        }
        let len = b as usize;
        let label = p.get(pos..pos.checked_add(len)?)?;
        if n != 0 {
            *out.get_mut(n)? = b'.';
            n += 1;
        }
        let next = n.checked_add(len)?;
        out.get_mut(n..next)?.copy_from_slice(label);
        n = next;
        pos += len;
    }
    None
}
pub fn captive_reply(query: &[u8], ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
    let flags = u16_at(query, 2)?;
    if flags & 0xf800 != 0
        || u16_at(query, 4)? != 1
        || u16_at(query, 6)? != 0
        || u16_at(query, 8)? != 0
    {
        return None;
    }
    u16_at(query, 10)?;
    let (end, _) = name(query, 12, &mut [0; 255])?;
    let kind = u16_at(query, end)?;
    let class = u16_at(query, end + 2)?;
    let qend = end.checked_add(4)?;
    let answer = class == 1 && (kind == 1 || kind == 255);
    let len = qend + if answer { 16 } else { 0 };
    let dst = out.get_mut(..len)?;
    dst.fill(0);
    dst[..2].copy_from_slice(query.get(..2)?);
    dst[2..4].copy_from_slice(&(0x8400 | (flags & 0x0100)).to_be_bytes());
    dst[5] = 1;
    dst[7] = u8::from(answer);
    dst[12..qend].copy_from_slice(query.get(12..qend)?);
    if answer {
        dst[qend..qend + 12].copy_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        dst[qend + 12..].copy_from_slice(&ip)
    }
    Some(len)
}
