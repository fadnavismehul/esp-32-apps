//! Logger that prints to USB serial and keeps the last few KB for `GET /logs`.

use core::cell::RefCell;
use core::fmt::Write;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;

const RING: usize = 4096;

struct Ring {
    buf: [u8; RING],
    /// Total bytes ever written; `len % RING` is the write position.
    len: usize,
}

static LOG: Mutex<Cs, RefCell<Ring>> = Mutex::new(RefCell::new(Ring { buf: [0; RING], len: 0 }));

struct Line {
    buf: [u8; 256],
    len: usize,
}

impl Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut line = Line { buf: [0; 256], len: 0 };
        let secs = embassy_time::Instant::now().as_millis();
        let _ = write!(line, "[{:>6}.{:03}] {:<5} {}\n", secs / 1000, secs % 1000, record.level(), record.args());
        let text = &line.buf[..line.len];
        esp_println::print!("{}", core::str::from_utf8(text).unwrap_or("?\n"));
        LOG.lock(|r| {
            let mut r = r.borrow_mut();
            for &b in text {
                let at = r.len % RING;
                r.buf[at] = b;
                r.len += 1;
            }
        });
    }

    fn flush(&self) {}
}

pub fn init() {
    static LOGGER: Logger = Logger;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
}

/// Copy the buffered log, oldest first, into `out`. Returns bytes copied.
pub fn snapshot(out: &mut [u8]) -> usize {
    LOG.lock(|r| {
        let r = r.borrow();
        let held = r.len.min(RING);
        let start = r.len - held;
        let n = held.min(out.len());
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = r.buf[(start + held - n + i) % RING];
        }
        n
    })
}
