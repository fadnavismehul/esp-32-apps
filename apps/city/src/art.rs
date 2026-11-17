//! Everything you may want to change about how City Window looks.
//!
//! This file is plain data. Each item has a comment that says what it
//! changes. Colours are `(red, green, blue)`, each 0 to 255. Screen rows count
//! down from 0 at the top to 479 at the bottom, and columns from 0 at the left
//! to 479 at the right.
//!
//! After a change, look at it with, for example:
//! `cargo run -q -p sim -- city --time 23:00 --frames 1 --out out/city-2300`
//! and then run `cargo test -p city`.

/// A colour as (red, green, blue).
pub type Rgb = (u8, u8, u8);

// ---------------------------------------------------------------------------
// Layout of the picture
// ---------------------------------------------------------------------------

/// The row where the sky meets the sea. Buildings stand on this row.
pub const HORIZON: i16 = 330;

/// How many colour bands the sky has, from the top down.
pub const SKY_BANDS: usize = 8;

/// The rows where each sky band starts, top to bottom. The last number is the
/// horizon. Bands near the horizon are thinner, like a real sunset.
pub const SKY_EDGES: [i16; SKY_BANDS + 1] = [0, 64, 116, 160, 198, 230, 256, 280, HORIZON];

/// The rows where each of the three sea bands starts, top to bottom.
pub const SEA_EDGES: [i16; 4] = [HORIZON, 350, 384, 480];

/// How one band fades into the next. Read from the band edge upward: `true`
/// rows take the lower band's colour. This gives thin retro stripes instead
/// of a smooth gradient, which would show ugly steps on this screen.
pub const BAND_FADE: [bool; 9] = [true, true, false, true, false, false, true, false, false];

/// Rows of sky above this one may show stars at night. Below it the haze
/// hides them.
pub const STAR_ROWS: i16 = 64;

/// Roughly one row in this many has a star.
pub const STAR_EVERY: u32 = 6;

// ---------------------------------------------------------------------------
// Colours for each time of day
// ---------------------------------------------------------------------------

/// The colours of one time of day. The app blends between these smoothly as
/// the clock moves from one key time to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Look {
    /// Sky bands, top to bottom.
    pub sky: [Rgb; SKY_BANDS],
    /// Sea bands, from the horizon down to the bottom of the screen.
    pub sea: [Rgb; 3],
    /// The coast road along the foot of the city.
    pub skyline: Rgb,
    /// The three building colours. Each building in `BUILDINGS` picks one.
    pub tones: [Rgb; 3],
    /// The hazy row of buildings further away, behind the main ones.
    pub far: Rgb,
    /// Top of the sun (or the whole moon).
    pub orb: Rgb,
    /// Bottom of the sun. The sun is drawn in stripes from `orb` to this.
    pub orb_low: Rgb,
    /// The thin streaks of cloud.
    pub cloud: Rgb,
    /// The palm trees in front.
    pub palm: Rgb,
}

/// Deep night: a black sky over a low burnt orange haze lit by the city's
/// sodium street lights. The towers stand almost black against it.
pub const NIGHT: Look = Look {
    sky: [
        (0, 0, 0),
        (0, 0, 0),
        (8, 2, 0),
        (20, 6, 2),
        (38, 12, 2),
        (64, 22, 4),
        (100, 38, 8),
        (150, 64, 14),
    ],
    sea: [(58, 22, 6), (18, 6, 2), (0, 0, 0)],
    skyline: (0, 0, 0),
    tones: [(8, 3, 1), (12, 4, 1), (5, 2, 0)],
    far: (60, 22, 5),
    orb: (255, 178, 96),
    orb_low: (232, 132, 60),
    cloud: (26, 8, 2),
    palm: (0, 0, 0),
};

/// Just before dawn: the night lifts to indigo and a lilac glow.
pub const BEFORE_DAWN: Look = Look {
    sky: [
        (6, 4, 26),
        (16, 10, 44),
        (32, 20, 68),
        (56, 34, 96),
        (86, 54, 122),
        (124, 78, 146),
        (166, 104, 162),
        (204, 138, 170),
    ],
    sea: [(110, 70, 130), (54, 30, 88), (12, 6, 30)],
    skyline: (14, 8, 30),
    tones: [(40, 26, 70), (52, 34, 84), (32, 20, 60)],
    far: (90, 60, 120),
    orb: (255, 222, 190),
    orb_low: (255, 160, 160),
    cloud: (130, 80, 140),
    palm: (8, 4, 20),
};

/// Dawn: pale peach and lilac.
pub const DAWN: Look = Look {
    sky: [
        (70, 56, 140),
        (110, 84, 170),
        (150, 110, 192),
        (192, 136, 204),
        (226, 156, 202),
        (248, 174, 190),
        (255, 194, 178),
        (255, 216, 170),
    ],
    sea: [(220, 160, 196), (150, 104, 176), (70, 48, 120)],
    skyline: (36, 20, 64),
    tones: [(78, 54, 112), (98, 68, 130), (64, 44, 100)],
    far: (160, 120, 180),
    orb: (255, 240, 200),
    orb_low: (255, 150, 160),
    cloud: (255, 196, 210),
    palm: (26, 12, 46),
};

/// Morning: the sky turns to soft pastel blue.
pub const MORNING: Look = Look {
    sky: [
        (120, 196, 238),
        (144, 206, 238),
        (166, 214, 238),
        (190, 220, 236),
        (212, 222, 232),
        (234, 220, 226),
        (250, 214, 214),
        (255, 210, 200),
    ],
    sea: [(110, 210, 214), (60, 176, 200), (30, 128, 170)],
    skyline: (120, 110, 150),
    tones: [(250, 196, 210), (176, 228, 214), (255, 220, 186)],
    far: (204, 206, 232),
    orb: (255, 250, 220),
    orb_low: (255, 226, 180),
    cloud: (255, 246, 246),
    palm: (20, 70, 76),
};

/// Day: bright pastel sky, soft cyan above and pale pink at the horizon.
pub const DAY: Look = Look {
    sky: [
        (96, 206, 242),
        (118, 214, 242),
        (140, 222, 242),
        (164, 228, 240),
        (190, 232, 238),
        (214, 232, 236),
        (236, 224, 232),
        (252, 212, 226),
    ],
    sea: [(80, 222, 214), (30, 190, 200), (12, 140, 174)],
    skyline: (120, 120, 160),
    tones: [(252, 190, 208), (168, 232, 218), (255, 222, 182)],
    far: (204, 214, 238),
    orb: (255, 255, 236),
    orb_low: (255, 246, 184),
    cloud: (255, 255, 255),
    palm: (16, 72, 78),
};

/// Golden hour, late afternoon: the sky warms before the sunset.
pub const GOLDEN: Look = Look {
    sky: [
        (84, 120, 200),
        (120, 130, 206),
        (160, 140, 204),
        (200, 150, 196),
        (232, 160, 180),
        (250, 172, 160),
        (255, 188, 140),
        (255, 208, 130),
    ],
    sea: [(220, 150, 150), (130, 100, 160), (50, 50, 110)],
    skyline: (50, 26, 70),
    tones: [(120, 70, 130), (150, 86, 140), (96, 56, 116)],
    far: (190, 130, 180),
    orb: (255, 244, 170),
    orb_low: (255, 150, 110),
    cloud: (255, 200, 190),
    palm: (16, 6, 26),
};

/// Sunset, the main poster look: hard bands from deep purple through magenta,
/// hot pink, coral and orange to gold.
pub const SUNSET: Look = Look {
    sky: [
        (28, 4, 56),
        (70, 10, 96),
        (128, 18, 116),
        (192, 30, 126),
        (238, 64, 124),
        (250, 108, 100),
        (255, 150, 72),
        (255, 198, 84),
    ],
    sea: [(120, 28, 104), (58, 10, 74), (14, 0, 28)],
    skyline: (8, 0, 18),
    tones: [(30, 4, 48), (40, 8, 60), (22, 2, 40)],
    far: (98, 18, 98),
    orb: (255, 232, 104),
    orb_low: (255, 60, 140),
    cloud: (108, 20, 106),
    palm: (0, 0, 0),
};

/// Dusk: the sun has gone, the last pink sinks into purple.
pub const DUSK: Look = Look {
    sky: [
        (6, 0, 20),
        (18, 2, 40),
        (38, 6, 64),
        (66, 10, 86),
        (104, 16, 104),
        (146, 26, 112),
        (186, 44, 110),
        (220, 74, 100),
    ],
    sea: [(86, 16, 88), (36, 4, 54), (6, 0, 14)],
    skyline: (6, 0, 14),
    tones: [(22, 4, 38), (30, 6, 48), (16, 2, 30)],
    far: (64, 12, 80),
    orb: (255, 232, 104),
    orb_low: (255, 60, 140),
    cloud: (60, 10, 76),
    palm: (0, 0, 0),
};

/// One point in the day. Between two keys, every colour and number blends
/// in a straight line, so the picture never jumps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    /// Minutes after midnight, 0 to 1439.
    pub at_min: u16,
    /// The colours at this time.
    pub look: Look,
    /// The row of the centre of the sun or moon. Bigger is lower; below
    /// about `HORIZON + radius` it is hidden.
    pub orb_y: i16,
    /// The radius of the sun or moon in pixels.
    pub orb_r: i16,
    /// `true` draws the moon, `false` the sun. Only change this while the orb
    /// is hidden below the horizon, so the swap is never seen.
    pub moon: bool,
    /// 0 by day, 255 at full night. It turns on the stars, the haze and the
    /// tower lights.
    pub night: u8,
}

/// The day, as a list of keys sorted by time. After the last key the day
/// wraps round to the first. Big colour changes need at least about 45
/// minutes between keys, or the test `no_jumps_over_24h` fails.
#[rustfmt::skip]
pub const KEYS: &[Key] = &[
    // Moon high in the night sky.
    Key { at_min: 3 * 60 + 5,  look: NIGHT,       orb_y: 112, orb_r: 34,  moon: true,  night: 255 },
    // The moon has set.
    Key { at_min: 5 * 60 + 10, look: NIGHT,       orb_y: 460, orb_r: 34,  moon: true,  night: 255 },
    // Before dawn. The sun is still hidden and swaps in for the moon.
    Key { at_min: 5 * 60 + 45, look: BEFORE_DAWN, orb_y: 420, orb_r: 76,  moon: false, night: 170 },
    // Sunrise, about 06:15: the top of the sun shows over the low hotels.
    Key { at_min: 6 * 60 + 15, look: DAWN,        orb_y: 330, orb_r: 76,  moon: false, night: 60 },
    Key { at_min: 7 * 60,      look: MORNING,     orb_y: 236, orb_r: 60,  moon: false, night: 0 },
    Key { at_min: 8 * 60,      look: DAY,         orb_y: 132, orb_r: 46,  moon: false, night: 0 },
    // Noon: the sun is at its highest.
    Key { at_min: 12 * 60,     look: DAY,         orb_y: 92,  orb_r: 42,  moon: false, night: 0 },
    Key { at_min: 16 * 60,     look: DAY,         orb_y: 124, orb_r: 46,  moon: false, night: 0 },
    Key { at_min: 17 * 60,     look: GOLDEN,      orb_y: 190, orb_r: 72,  moon: false, night: 0 },
    // The sunset look, about 17:30 to 19:30. The sun sits on the skyline.
    Key { at_min: 18 * 60 + 15, look: SUNSET,     orb_y: 262, orb_r: 100, moon: false, night: 64 },
    Key { at_min: 19 * 60 + 30, look: DUSK,       orb_y: 462, orb_r: 100, moon: false, night: 140 },
    // Full night. The moon swaps in below the horizon, then rises.
    Key { at_min: 20 * 60 + 15, look: NIGHT,      orb_y: 462, orb_r: 34,  moon: true,  night: 255 },
    Key { at_min: 22 * 60 + 15, look: NIGHT,      orb_y: 112, orb_r: 34,  moon: true,  night: 255 },
];

/// The time shown before the clock is known, in minutes after midnight.
pub const BEFORE_SYNC_MIN: u16 = 18 * 60 + 15;

// ---------------------------------------------------------------------------
// Sun and moon
// ---------------------------------------------------------------------------

/// The horizontal cuts through the lower half of the sun, as
/// (distance below the centre, thickness). Both are in 1/256ths of the
/// radius, so they grow with the sun.
pub const SUN_SLITS: &[(u8, u8)] = &[(22, 8), (60, 12), (98, 16), (136, 21), (174, 27), (212, 34)];

/// Below this radius the sun has no cuts (the small midday sun).
pub const SLITS_FROM_R: i16 = 48;

/// How far the moon's shadow disc sits from its centre, as
/// (right, up) in 1/256ths of the radius. Larger means a thinner crescent.
pub const MOON_SHADOW: (i16, i16) = (110, 64);

// ---------------------------------------------------------------------------
// The city
// ---------------------------------------------------------------------------

/// The shape of a building's top.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Top {
    /// A plain flat roof.
    Flat,
    /// Two setbacks, each narrower than the one below.
    Steps,
    /// Art deco crown: three setbacks and a thin spire.
    Deco,
    /// A flat roof with a thin antenna.
    Mast,
    /// A rounded dome as wide as the building.
    Dome,
}

/// One building.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Building {
    /// Left edge column.
    pub x: i16,
    /// Width in pixels.
    pub w: i16,
    /// Height of the main block above the horizon (the top adds more).
    pub h: i16,
    /// The shape of the top.
    pub top: Top,
    /// Which of the three `tones` colours it uses, 0 to 2.
    pub tone: u8,
    /// `true` gives it bands of glass by day, like a beach hotel.
    pub bands: bool,
}

const fn b(x: i16, w: i16, h: i16, top: Top, tone: u8, bands: bool) -> Building {
    Building { x, w, h, top, tone, bands }
}

/// The main skyline, drawn left to right; later ones stand in front.
/// Keep the middle low so the sun shows above it.
#[rustfmt::skip]
pub const BUILDINGS: &[Building] = &[
    // Left group.
    b(-20, 44, 50,  Top::Flat,  2, true),
    b(22,  26, 82,  Top::Steps, 0, false),
    b(46,  22, 58,  Top::Flat,  1, true),
    b(66,  36, 124, Top::Deco,  2, false),
    b(100, 24, 74,  Top::Mast,  0, false),
    b(122, 30, 42,  Top::Flat,  1, true),
    b(158, 20, 30,  Top::Dome,  2, false),
    // Low beach hotels in the middle, under the sun.
    b(182, 44, 20,  Top::Flat,  0, true),
    b(226, 26, 34,  Top::Steps, 1, false),
    b(252, 54, 18,  Top::Flat,  2, true),
    // Right group.
    b(312, 24, 52,  Top::Steps, 0, false),
    b(334, 40, 136, Top::Deco,  0, false),
    b(372, 26, 94,  Top::Steps, 2, false),
    b(396, 30, 60,  Top::Flat,  1, true),
    b(424, 22, 82,  Top::Mast,  0, false),
    b(444, 56, 48,  Top::Flat,  2, true),
];

/// The hazy buildings further back. Only their outline shows.
#[rustfmt::skip]
pub const FAR_BUILDINGS: &[Building] = &[
    b(8,   30, 74,  Top::Flat,  0, false),
    b(126, 26, 70,  Top::Steps, 0, false),
    b(150, 18, 56,  Top::Mast,  0, false),
    b(206, 24, 46,  Top::Flat,  0, false),
    b(282, 30, 40,  Top::Dome,  0, false),
    b(300, 22, 90,  Top::Flat,  0, false),
    b(400, 26, 112, Top::Steps, 0, false),
    b(452, 30, 96,  Top::Flat,  0, false),
];

/// The bands of glass on beach hotels by day: one band every
/// `WINDOW_PITCH.1` rows, `WINDOW_SIZE.1` rows tall.
pub const WINDOW_PITCH: (i16, i16) = (6, 8);
pub const WINDOW_SIZE: (i16, i16) = (2, 3);

// ---------------------------------------------------------------------------
// Night haze and tower lights
// ---------------------------------------------------------------------------

/// The haze over the city at night: the colour the far buildings fade into
/// near the ground, and how many rows up the fade reaches.
pub const HAZE_FAR: (Rgb, i16) = ((140, 58, 12), 100);

/// The same for the main buildings: only their feet fade into the haze.
pub const HAZE_NEAR: (Rgb, i16) = ((70, 26, 6), 44);

/// The bright and dim amber of the lit stair cores and floors.
pub const AMBER: Rgb = (255, 196, 120);
pub const AMBER_DIM: Rgb = (150, 84, 30);

/// The one cold accent: the lit crown and the roof beacons.
pub const TEAL: Rgb = (40, 226, 200);

/// The body of the lit crown, a darker teal.
pub const TEAL_DIM: Rgb = (10, 84, 76);

/// The halo round each roof beacon.
pub const TEAL_HALO: Rgb = (8, 70, 62);

/// A lit stair core: a dashed line of light up one tower at night, and a few
/// lit floors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stair {
    /// Which building in `BUILDINGS`, counting from 0.
    pub building: usize,
    /// The column of the line, from the building's left edge.
    pub x: i16,
    /// Lit floors as (rows below the roof, inset from the left, inset from
    /// the right, bright).
    pub floors: &'static [(i16, i16, i16, bool)],
}

/// The stair cores. Keep them few: a handful of lines reads as a city at
/// night, a grid of dots reads as confetti.
#[rustfmt::skip]
pub const STAIRS: &[Stair] = &[
    Stair { building: 3,  x: 9,  floors: &[(30, 4, 4, true), (62, 4, 14, false)] },
    Stair { building: 11, x: 28, floors: &[(54, 4, 4, true), (22, 4, 16, false)] },
    Stair { building: 12, x: 6,  floors: &[(40, 10, 4, false)] },
    Stair { building: 1,  x: 18, floors: &[] },
    Stair { building: 14, x: 5,  floors: &[] },
];

/// The tower whose stepped crown is lit teal at night.
pub const CROWN: usize = 3;

/// The towers with a teal beacon on top at night.
pub const BEACONS: &[usize] = &[11, 4, 14];

/// The bright glints each tower light leaves on the water at night, from
/// the horizon down, out of 255. Where they fall and how long they are
/// follows from where the lights are.
pub const GLINT: [u8; 3] = [150, 90, 45];

/// How the lit crown and beacons stutter when the power flickers, from
/// start to end. 255 is fully on.
pub const FLICKER: [u8; 16] = [255, 40, 255, 255, 0, 120, 255, 0, 0, 255, 30, 255, 255, 70, 255, 255];

/// The red tail lights of the car on the coast road.
pub const TAIL_LIGHT: Rgb = (255, 40, 70);

/// The car's headlight.
pub const HEAD_LIGHT: Rgb = (255, 236, 190);

/// The plane's blinking light.
pub const PLANE_LIGHT: Rgb = (255, 50, 60);

/// The colour of the small time at night, sodium amber. By day it takes the sun's colour.
pub const CLOCK_NIGHT: Rgb = (255, 156, 60);

/// How strongly the small time stands out from the sea, 0 to 255.
pub const CLOCK_STRENGTH: u8 = 170;

/// Thin streaks of cloud, as (left column, top row, width).
pub const CLOUDS: &[(i16, i16, i16)] = &[(150, 70, 140), (300, 132, 120), (24, 196, 110)];

// ---------------------------------------------------------------------------
// Reflections on the water
// ---------------------------------------------------------------------------

/// The water mirrors the sky and city. These rows split it into four
/// strips, from the horizon down to the bottom of the screen.
pub const MIRROR_EDGES: [i16; 5] = [HORIZON, 350, 384, 430, 480];

/// How much of the water's own colour shows over the mirrored picture in
/// each strip, out of 255. Water near the horizon is seen at a low angle
/// and mirrors almost everything; close to you it mirrors much less.
pub const FRESNEL: [u8; 4] = [70, 130, 185, 225];

/// The water colour each strip blends toward: an index into the look's
/// `sea` bands.
pub const MIRROR_SEA: [usize; 4] = [0, 1, 2, 2];

/// Ripples bend the mirrored picture up and down by up to 1 pixel plus one
/// more for every this many rows below the horizon.
pub const RIPPLE_ROWS_PER_PX: i16 = 22;

/// The same for the sideways wobble.
pub const WOBBLE_ROWS_PER_PX: i16 = 36;

/// Where the moon's path on the water sits, from the moon's centre, in
/// 1/256ths of its radius. Negative is left, under the lit part of the
/// crescent.
pub const MOON_PATH_SHIFT: i16 = -131;

// ---------------------------------------------------------------------------
// Palm trees
// ---------------------------------------------------------------------------

/// One palm leaf (frond), from the top of the trunk out to its tip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frond {
    /// Where the tip is, measured from the top of the trunk (right, down).
    pub tip: (i16, i16),
    /// How high the leaf arches before it droops. More negative is higher.
    pub arch: i16,
    /// Length of the longest leaflets hanging from it.
    pub leaf: i16,
}

/// One palm tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palm {
    /// Foot of the trunk (column, row). It may be off the screen.
    pub base: (i16, i16),
    /// Top of the trunk, where the leaves start.
    pub crown: (i16, i16),
    /// How much the trunk bows sideways in the middle.
    pub bend: i16,
    /// Trunk width at the foot and at the top.
    pub width: (i16, i16),
    /// The leaves.
    pub fronds: &'static [Frond],
}

const fn f(tip: (i16, i16), arch: i16, leaf: i16) -> Frond {
    Frond { tip, arch, leaf }
}

/// The palms in front of the city. The number in `[Palm; 2]` must match the
/// list. Leave the lower left of the sea fairly clear: the time goes there.
#[rustfmt::skip]
pub const PALMS: [Palm; 2] = [
    // Left palm: the big one, leaning in from the left edge.
    Palm {
        base: (-12, 500),
        crown: (84, 122),
        bend: -42,
        width: (24, 11),
        fronds: &[
            f((-124, 84), -56, 34),
            f((-120, 6), -70, 28),
            f((-72, -22), -104, 16),
            f((74, -18), -104, 16),
            f((124, 8), -66, 28),
            f((136, 92), -56, 34),
            f((40, 116), -10, 24),
        ],
    },
    // Right palm: smaller and further away, leaning in from the right.
    Palm {
        base: (506, 500),
        crown: (418, 180),
        bend: 28,
        width: (20, 10),
        fronds: &[
            f((-108, 70), -50, 30),
            f((-102, 0), -62, 24),
            f((-60, -22), -92, 15),
            f((60, -20), -92, 15),
            f((100, 14), -54, 24),
            f((96, 86), -44, 28),
            f((-30, 104), -8, 20),
        ],
    },
];
