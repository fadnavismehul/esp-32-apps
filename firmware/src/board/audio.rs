//! ES8311 codec on I2C 0x18, fed over I2S (MCLK 19, BCLK 20, WS 22,
//! DOUT 23). The speaker amp has no enable pin on this board.
//!
//! I2S runs at 16 kHz, 16-bit stereo, MCLK = 256 x fs = 4.096 MHz. Register
//! values follow Espressif's es8311 driver for that clock (coefficient row
//! `{4096000, 16000, ...}`).

use app_sdk::Sound;

use super::Bus;

const ADDR: u8 = 0x18;
pub const SAMPLE_RATE: u32 = 16_000;

pub fn init(i2c: &mut Bus, volume: u8) -> Result<(), esp_hal::i2c::master::Error> {
    let regs: &[(u8, u8)] = &[
        (0x00, 0x1F), // reset
        (0x00, 0x00),
        (0x00, 0x80), // power on, slave serial port
        (0x01, 0x3F), // clocks from the MCLK pin, all on
        (0x02, 0x00), // pre_div 1, pre_multi 1x
        (0x03, 0x10), // single speed, adc osr
        (0x04, 0x10), // dac osr
        (0x05, 0x00), // adc/dac dividers 1
        (0x06, 0x03), // bclk divider 4, not inverted
        (0x07, 0x00), // lrck divider high
        (0x08, 0xFF), // lrck divider low
        (0x09, 0x0C), // DAC serial port: I2S, 16 bit
        (0x0A, 0x0C), // ADC serial port: I2S, 16 bit
        (0x0D, 0x01), // power up analog
        (0x0E, 0x02), // PGA and ADC modulator
        (0x12, 0x00), // power up DAC
        (0x13, 0x10), // output to the speaker driver
        (0x1C, 0x6A), // ADC EQ bypass, DC offset cancel
        (0x37, 0x08), // DAC EQ bypass
        (0x31, 0x00), // unmute
    ];
    for &(reg, val) in regs {
        i2c.write(ADDR, &[reg, val])?;
    }
    set_volume(i2c, volume)
}

pub fn set_volume(i2c: &mut Bus, volume: u8) -> Result<(), esp_hal::i2c::master::Error> {
    let v = volume.min(100) as u32;
    let reg = if v == 0 { 0 } else { (v * 256 / 100 - 1) as u8 };
    i2c.write(ADDR, &[0x32, reg])
}

/// Leading silence lets the codec's clocks settle before the sound starts,
/// since I2S only runs while a sound is playing.
const LEAD_MS: u32 = 8;
/// Sounds stream through a ring of DMA chunks, so a long chime needs no big
/// buffer. Only whole chunks are sent, so every sound is padded to a chunk.
pub const CHUNK: usize = 1024;
/// 128 ms of audio: plenty of slack for a frame that keeps the CPU busy.
pub const BUFFER_BYTES: usize = 8 * CHUNK;

/// 64-entry quarter-free sine, amplitude 127.
const SIN64: [i8; 64] = {
    let mut t = [0i8; 64];
    // Parabolic approximation, close enough for a click.
    let mut i = 0;
    while i < 64 {
        let x = (i as i32 % 32) * 2 - 31; // -31..31 over half a period
        let y = 127 - 127 * x * x / (31 * 31);
        t[i] = if i < 32 { y as i8 } else { -y as i8 };
        i += 1;
    }
    t
};

const fn frames(ms: u32) -> u32 {
    SAMPLE_RATE * ms / 1000
}

/// Phase step for `hz`, 8 fractional bits.
const fn step(hz: u32) -> u32 {
    hz * 64 * 256 / SAMPLE_RATE
}

/// 256-entry sine, amplitude 32767, from Bhaskara's approximation: much
/// purer than `SIN64`, which matters for soft sustained tones.
const SIN256: [i16; 256] = {
    let mut t = [0i16; 256];
    let mut i = 0;
    while i < 256 {
        // x in 0..128 is half a period; Bhaskara: 16x(P-x) / (5P^2 - 4x(P-x)), P = 128.
        let x = (i % 128) as i64;
        let p = 128i64;
        let y = 32767 * 16 * x * (p - x) / (5 * p * p - 4 * x * (p - x));
        t[i] = if i < 128 { y as i16 } else { -y as i16 };
        i += 1;
    }
    t
};

/// Phase step for `hz` on a 32-bit phase accumulator.
const fn step32(hz: u32) -> u32 {
    ((hz as u64) << 32).div_euclid(SAMPLE_RATE as u64) as u32
}

/// One sine note: starts at `at`, swells for `rise` frames, then fades over
/// `fall` frames.
struct Note {
    at: u32,
    rise: u32,
    fall: u32,
    step: u32,
    amp: i32,
}

const fn note(at_ms: u32, rise_ms: u32, fall_ms: u32, hz: u32, amp: i32) -> Note {
    Note { at: frames(at_ms), rise: frames(rise_ms), fall: frames(fall_ms), step: step32(hz), amp }
}

/// The reminder chime, "glass": a slow rising Fmaj9 arpeggio of pure sines,
/// each note left to ring.
const CHIME: [Note; 4] = [
    note(0, 12, 1600, 698, 5200),
    note(170, 12, 1500, 880, 4800),
    note(340, 12, 1400, 1047, 4400),
    note(510, 12, 1500, 1319, 4000),
];

/// Generates a sound a buffer at a time.
pub struct Voice {
    sound: Sound,
    i: u32,
    body: u32,
    total: u32,
    phase: [u32; 4],
    lfsr: u16,
}

impl Voice {
    pub fn new(sound: Sound) -> Self {
        let body = match sound {
            Sound::Tick { .. } => frames(14),
            Sound::Click => frames(22),
            Sound::Chime => CHIME.iter().map(|n| n.at + n.rise + n.fall).max().unwrap_or(0),
            Sound::PokeballOpen => frames(540),
            Sound::PokeballClose => frames(250),
        };
        let lead = frames(LEAD_MS);
        let total = (lead * 2 + body).next_multiple_of(CHUNK as u32 / 4);
        Self { sound, i: 0, body, total, phase: [0; 4], lfsr: 0xACE1 }
    }

    pub fn done(&self) -> bool {
        self.i >= self.total
    }

    /// Fill `out` with interleaved stereo i16 little-endian frames. Returns
    /// the bytes written: whole frames, and 0 once the sound is over.
    pub fn fill(&mut self, out: &mut [u8]) -> usize {
        let n = ((self.total - self.i) as usize).min(out.len() / 4);
        for frame in out[..n * 4].chunks_exact_mut(4) {
            let b = (self.sample() as i16).to_le_bytes();
            frame.copy_from_slice(&[b[0], b[1], b[0], b[1]]);
            self.i += 1;
        }
        n * 4
    }

    fn sample(&mut self) -> i32 {
        let lead = frames(LEAD_MS);
        if self.i < lead || self.i >= lead + self.body {
            return 0;
        }
        let t = self.i - lead;
        let s = match self.sound {
            Sound::Tick { pitch } => self.blip(t, step(1200 + pitch as u32 * 6), 9000, 0),
            Sound::Click => self.blip(t, step(2400), 16000, 5000),
            Sound::Chime => {
                let mut sum = 0;
                for (n, phase) in CHIME.iter().zip(&mut self.phase) {
                    if t < n.at || t >= n.at + n.rise + n.fall {
                        continue;
                    }
                    let u = t - n.at;
                    // Squared swell in, cubic fade out: no edges to click.
                    let env = if u < n.rise {
                        let r = (u * 256 / n.rise) as i32;
                        r * r / 256
                    } else {
                        let left = ((n.rise + n.fall - u) * 256 / n.fall) as i32;
                        left * left / 256 * left / 256
                    };
                    *phase = phase.wrapping_add(n.step);
                    sum += SIN256[(*phase >> 24) as usize] as i32 * n.amp / 32767 * env / 256;
                }
                sum
            }
            Sound::PokeballOpen => self.pokeball_open(t),
            Sound::PokeballClose => self.pokeball_close(t),
        };
        s.clamp(-32767, 32767)
    }

    /// Pokéball opening: mechanical latch pop -> fast rising energy sweep ->
    /// bright harmonic sparkle chime.
    fn pokeball_open(&mut self, t: u32) -> i32 {
        let mut sum = 0i32;

        // 1. 0..32 ms: crisp metallic latch pop + short release hiss.
        let pop_end = frames(32);
        if t < pop_end {
            let env = ((pop_end - t) * 256 / pop_end) as i32;
            let env = env * env / 256;
            let hz = 2100 - (t * 1100 / pop_end);
            self.phase[0] = self.phase[0].wrapping_add(step32(hz));
            let pop = SIN256[(self.phase[0] >> 24) as usize] as i32 * 13000 / 32767;
            self.lfsr = (self.lfsr >> 1) ^ (0u16.wrapping_sub(self.lfsr & 1) & 0xB400);
            let hiss = (self.lfsr as i16 as i32) * 5500 / 32768;
            sum += (pop + hiss) * env / 256;
        }

        // 2. 22..245 ms: rising electronic energy whoosh (380 Hz -> 1560 Hz)
        //    with bright octave overtone and airy shimmer.
        let sw_start = frames(22);
        let sw_len = frames(223);
        if t >= sw_start && t < sw_start + sw_len {
            let u = t - sw_start;
            let rise = frames(26);
            let env = if u < rise {
                (u * 256 / rise) as i32
            } else {
                let left = ((sw_len - u) * 256 / (sw_len - rise)) as i32;
                left * left / 256
            };
            // Quadratic pitch curve for a punchy upward energy burst.
            let p = (u * 256 / sw_len) as u32;
            let hz = 380 + (p * p * 1180) / (256 * 256);
            self.phase[0] = self.phase[0].wrapping_add(step32(hz));
            self.phase[1] = self.phase[1].wrapping_add(step32(hz * 2));
            let fund = SIN256[(self.phase[0] >> 24) as usize] as i32 * 11000 / 32767;
            let harm = SIN256[(self.phase[1] >> 24) as usize] as i32 * 5000 / 32767;
            self.lfsr = (self.lfsr >> 1) ^ (0u16.wrapping_sub(self.lfsr & 1) & 0xB400);
            let air = (self.lfsr as i16 as i32) * 2200 / 32768;
            sum += (fund + harm + air) * env / 256;
        }

        // 3. 145..540 ms: bright twin harmonic sparkle chimes (G6 + C7).
        let chimes = [
            (frames(145), frames(14), frames(350), step32(1568), 6400i32),
            (frames(195), frames(14), frames(330), step32(2093), 5600i32),
        ];
        for (idx, &(at, rise, fall, st, amp)) in chimes.iter().enumerate() {
            if t >= at && t < at + rise + fall {
                let u = t - at;
                let env = if u < rise {
                    let r = (u * 256 / rise) as i32;
                    r * r / 256
                } else {
                    let left = ((rise + fall - u) * 256 / fall) as i32;
                    left * left / 256 * left / 256
                };
                let p = &mut self.phase[2 + idx];
                *p = p.wrapping_add(st);
                sum += SIN256[(*p >> 24) as usize] as i32 * amp / 32767 * env / 256;
            }
        }
        sum
    }

    /// Pokéball closing: downward energy retract followed by a crisp latch snap.
    fn pokeball_close(&mut self, t: u32) -> i32 {
        let mut sum = 0i32;

        // 1. 0..170 ms: falling energy retract (1120 Hz -> 340 Hz).
        let sw_len = frames(170);
        if t < sw_len {
            let rise = frames(15);
            let env = if t < rise {
                (t * 256 / rise) as i32
            } else {
                let left = ((sw_len - t) * 256 / (sw_len - rise)) as i32;
                left * left / 256
            };
            let hz = 1120 - (t * 780 / sw_len);
            self.phase[0] = self.phase[0].wrapping_add(step32(hz));
            self.phase[1] = self.phase[1].wrapping_add(step32(hz * 2));
            let fund = SIN256[(self.phase[0] >> 24) as usize] as i32 * 8500 / 32767;
            let harm = SIN256[(self.phase[1] >> 24) as usize] as i32 * 3000 / 32767;
            sum += (fund + harm) * env / 256;
        }

        // 2. 165..235 ms: crisp double-body mechanical latch lock snap.
        let snap_at = frames(165);
        let snap_len = frames(70);
        if t >= snap_at && t < snap_at + snap_len {
            let u = t - snap_at;
            let env = ((snap_len - u) * 256 / snap_len) as i32;
            let env = env * env / 256;
            self.phase[2] = self.phase[2].wrapping_add(step32(1680));
            self.phase[3] = self.phase[3].wrapping_add(step32(620));
            let click = SIN256[(self.phase[2] >> 24) as usize] as i32 * 10000 / 32767;
            let thud = SIN256[(self.phase[3] >> 24) as usize] as i32 * 9000 / 32767;
            self.lfsr = (self.lfsr >> 1) ^ (0u16.wrapping_sub(self.lfsr & 1) & 0xB400);
            let hiss = (self.lfsr as i16 as i32) * 4000 / 32768;
            sum += (click + thud + hiss) * env / 256;
        }
        sum
    }

    /// A short decaying tone with optional noise, for ticks and clicks.
    fn blip(&mut self, t: u32, step: u32, amp: i32, noise: i32) -> i32 {
        // Linear-ish exponential decay: env falls to ~0 over the body.
        let env = (self.body - t) as i32 * 256 / self.body as i32;
        let env = env * env / 256;
        self.phase[0] = self.phase[0].wrapping_add(step);
        let tone = SIN64[((self.phase[0] >> 8) & 63) as usize] as i32 * amp / 127;
        self.lfsr = (self.lfsr >> 1) ^ (0u16.wrapping_sub(self.lfsr & 1) & 0xB400);
        let hiss = (self.lfsr as i16 as i32) * noise / 32768;
        (tone + hiss) * env / 256
    }
}
