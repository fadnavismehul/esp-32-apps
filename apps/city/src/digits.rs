//! Small 3x5 cell font for digits and drift offsets for the small time display.

/// 3x5 cell font for digits 0-9.
/// Each row's low 3 bits are the cells, where bit 2 is the left cell.
pub const GLYPHS: [[u8; 5]; 10] = [
    // 0: ### / # # / # # / # # / ###
    [0b111, 0b101, 0b101, 0b101, 0b111],
    // 1:  #  / ##  /  #  /  #  / ###
    [0b010, 0b110, 0b010, 0b010, 0b111],
    // 2: ### /   # / ### / #   / ###
    [0b111, 0b001, 0b111, 0b100, 0b111],
    // 3: ### /   # / ### /   # / ###
    [0b111, 0b001, 0b111, 0b001, 0b111],
    // 4: # # / # # / ### /   # /   #
    [0b101, 0b101, 0b111, 0b001, 0b001],
    // 5: ### / #   / ### /   # / ###
    [0b111, 0b100, 0b111, 0b001, 0b111],
    // 6: ### / #   / ### / # # / ###
    [0b111, 0b100, 0b111, 0b101, 0b111],
    // 7: ### /   # /   # /   # /   #
    [0b111, 0b001, 0b001, 0b001, 0b001],
    // 8: ### / # # / ### / # # / ###
    [0b111, 0b101, 0b111, 0b101, 0b111],
    // 9: ### / # # / ### /   # / ###
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Returns HH and MM as four digits in 24-hour time.
pub fn digits(sec_of_day: u32) -> [u8; 4] {
    let sec = sec_of_day % 86_400;
    let h = sec / 3600;
    let m = (sec / 60) % 60;
    [
        (h / 10) as u8,
        (h % 10) as u8,
        (m / 10) as u8,
        (m % 10) as u8,
    ]
}

/// A fixed 8-step path within ±6 px.
const DRIFT_PATH: [(i8, i8); 8] = [
    (0, -6),
    (4, -4),
    (6, 0),
    (4, 4),
    (0, 6),
    (-4, 4),
    (-6, 0),
    (-4, -4),
];

/// Position drift offset to prevent burn-in, stepping once every 3 minutes.
pub fn drift(minute_of_day: u16) -> (i8, i8) {
    let step = (minute_of_day / 3 % 8) as usize;
    DRIFT_PATH[step]
}

/// Screen pixels per font cell. Each lit cell is a square one pixel
/// smaller, so the digits look like a small dot-matrix sign.
pub const CELL: i32 = 6;

/// Left edge column of each digit and of the colon, in cells.
const DIGIT_COL: [i32; 4] = [0, 4, 10, 14];
const COLON_COL: i32 = 8;
/// The whole time is this many cells wide and 5 tall.
const WIDTH_CELLS: i32 = 17;

/// Where the top left of the time sits before drift: low on the left, over
/// the sea, clear of the palm trunk.
pub const ORIGIN: (i32, i32) = (46, 424);

/// The box the time covers at `sec_of_day`: (left, top, right, bottom),
/// right and bottom exclusive.
pub fn bounds(sec_of_day: u32) -> (i32, i32, i32, i32) {
    let (dx, dy) = drift(((sec_of_day % 86_400) / 60) as u16);
    let x = ORIGIN.0 + dx as i32;
    let y = ORIGIN.1 + dy as i32;
    (x, y, x + WIDTH_CELLS * CELL - 1, y + 5 * CELL - 1)
}

/// Draws row `y` of the time, if it crosses it. Each row is a few fills.
pub fn draw_row(row: &mut [u16], y: i32, sec_of_day: u32, colour: u16) {
    let (x0, y0, _, y1) = bounds(sec_of_day);
    if y < y0 || y >= y1 {
        return;
    }
    let cy = (y - y0) / CELL;
    if (y - y0) % CELL == CELL - 1 {
        return;
    }
    let ds = digits(sec_of_day);
    let mut fill = |col: i32, cells: i32| {
        let a = (x0 + col * CELL).clamp(0, 480) as usize;
        let b = (x0 + (col + cells) * CELL - 1).clamp(0, 480) as usize;
        if a < b {
            row[a..b].fill(colour);
        }
    };
    for (k, &d) in ds.iter().enumerate() {
        let bits = GLYPHS[d as usize][cy as usize];
        for c in 0..3 {
            if bits & (0b100 >> c) != 0 {
                fill(DIGIT_COL[k] + c, 1);
            }
        }
    }
    if cy == 1 || cy == 3 {
        fill(COLON_COL, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digits() {
        assert_eq!(digits(5 * 3600 + 7 * 60), [0, 5, 0, 7]);
        assert_eq!(digits(86399), [2, 3, 5, 9]);
    }

    #[test]
    fn test_drift() {
        assert_eq!(drift(720), drift(721));
        assert_ne!(drift(720), drift(723));
        for m in 0..1440 {
            let (dx, dy) = drift(m);
            assert!(
                (-6..=6).contains(&dx) && (-6..=6).contains(&dy),
                "drift({m}) = ({dx}, {dy}) out of range [-6, 6]"
            );
        }
    }

    #[test]
    fn test_glyphs_distinct() {
        for (i, a) in GLYPHS.iter().enumerate() {
            for (j, b) in GLYPHS.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "glyph {i} and {j} must be distinct");
            }
        }
    }
}
