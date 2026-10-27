//! U4: chart layouts (week bars and day tracks) drawn onto a `png::Canvas`.
use crate::png::{self, Canvas};

pub const BG: u8 = 0;
pub const GRID: u8 = 1;
pub const UNDER_LOW: u8 = 2;
pub const UNDER_TARGET: u8 = 3;
pub const AT_TARGET: u8 = 4;
pub const TARGET_LINE: u8 = 5;
pub const VEG: u8 = 6;
pub const NO_VEG: u8 = 7;
pub const PROTEIN: u8 = 8;
pub const CARBS: u8 = 9;
pub const FAT: u8 = 10;
pub const UNLOGGED: u8 = 11;

pub const PALETTE: [[u8; 3]; 12] = [
    [43, 27, 46],    // BG: plum
    [90, 70, 95],    // GRID
    [240, 128, 128], // UNDER_LOW: coral
    [247, 182, 163], // UNDER_TARGET: peach
    [158, 217, 160], // AT_TARGET: green
    [255, 236, 200], // TARGET_LINE: cream
    [126, 200, 128], // VEG
    [110, 100, 115], // NO_VEG
    [245, 169, 184], // PROTEIN: pink
    [255, 214, 165], // CARBS: baby orange
    [189, 178, 255], // FAT: lavender
    [80, 64, 85],    // UNLOGGED
];

/// One day in the week chart.
pub struct Bar {
    pub kcal: f64,
    pub logged: bool,
    pub veg: bool,
}

const W_WEEK: u32 = 560;
const H_WEEK: u32 = 280;
const BASE_Y: i64 = 220;
const PLOT_TOP: i64 = 20;
const PLOT_X0: i64 = 20;
const PLOT_W: i64 = 520;
const BAR_W: i64 = 44;

pub fn week_canvas(bars: &[Bar], target: f64, low: f64, split_kcal: [f64; 3]) -> Canvas {
    let mut c = Canvas::new(W_WEEK, H_WEEK, BG);
    let n = bars.len().max(1) as i64;
    let max_kcal = bars.iter().filter(|b| b.logged).map(|b| b.kcal).fold(0.0, f64::max);
    let ymax = (target * 1.25).max(max_kcal * 1.05).max(1.0);
    let plot_h = (BASE_Y - PLOT_TOP) as f64;
    let y_of = |kcal: f64| BASE_Y - (kcal / ymax * plot_h).round() as i64;

    for (i, b) in bars.iter().enumerate() {
        let i = i as i64;
        let slot_w = PLOT_W / n;
        let centre = PLOT_X0 + PLOT_W * i / n + slot_w / 2;
        let x = centre - BAR_W / 2;
        if b.logged {
            let colour = if b.kcal < low {
                UNDER_LOW
            } else if b.kcal < target {
                UNDER_TARGET
            } else {
                AT_TARGET
            };
            let top = y_of(b.kcal);
            c.rect(x, top, BAR_W, BASE_Y - top, colour);
            c.rect(centre - 5, 228, 10, 10, if b.veg { VEG } else { NO_VEG });
        } else {
            c.rect(x, BASE_Y - 4, BAR_W, 4, UNLOGGED);
        }
    }

    // Baseline, then the dashed target line on top.
    c.rect(PLOT_X0, BASE_Y, PLOT_W, 1, GRID);
    let ty = y_of(target);
    let mut x = PLOT_X0;
    while x < PLOT_X0 + PLOT_W {
        c.rect(x, ty, 8.min(PLOT_X0 + PLOT_W - x), 2, TARGET_LINE);
        x += 14;
    }

    // Macro strip, widths proportional to the three values.
    let sum: f64 = split_kcal.iter().sum();
    if sum > 0.0 {
        let mut cum = 0.0;
        let mut x0 = PLOT_X0;
        for (k, v) in split_kcal.iter().enumerate() {
            cum += v;
            let x1 = PLOT_X0 + (PLOT_W as f64 * cum / sum).round() as i64;
            let colour = [PROTEIN, CARBS, FAT][k];
            c.rect(x0, 252, x1 - x0, 16, colour);
            x0 = x1;
        }
    }
    c
}

pub fn week_png(bars: &[Bar], target: f64, low: f64, split_kcal: [f64; 3]) -> Vec<u8> {
    png::encode(&week_canvas(bars, target, low, split_kcal), &PALETTE)
}

const W_DAY: u32 = 560;
const H_DAY: u32 = 200;

pub fn day_canvas(eaten: [f64; 4], target: [f64; 4]) -> Canvas {
    let mut c = Canvas::new(W_DAY, H_DAY, BG);
    let tops = [20i64, 65, 110, 155];
    for (k, top) in tops.iter().enumerate() {
        c.rect(PLOT_X0, *top, PLOT_W, 28, GRID);
        let t = target[k];
        let ratio = if t > 0.0 { (eaten[k] / t).min(1.0) } else { 0.0 };
        let fill = (ratio * PLOT_W as f64).round() as i64;
        let colour = match k {
            0 if eaten[0] >= t => AT_TARGET,
            0 => UNDER_TARGET,
            1 => PROTEIN,
            2 => CARBS,
            _ => FAT,
        };
        c.rect(PLOT_X0, *top, fill, 28, colour);
        if t > 0.0 && eaten[k] >= t {
            c.rect(PLOT_X0 + PLOT_W - 4, *top, 4, 28, TARGET_LINE);
        }
    }
    c
}

pub fn day_png(eaten: [f64; 4], target: [f64; 4]) -> Vec<u8> {
    png::encode(&day_canvas(eaten, target), &PALETTE)
}
