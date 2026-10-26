//! U4: the std-only PNG writer and the two chart layouts. Pixels are checked by palette index, not by eye.
use discord_inbox::charts::{self, Bar, BG, AT_TARGET, UNDER_LOW, UNLOGGED, VEG, NO_VEG, TARGET_LINE, UNDER_TARGET};
use discord_inbox::png::{self, Canvas};
use std::process::Command;

const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// (type, data) for each chunk; panics if a CRC is wrong.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&png[..8], &SIG, "PNG signature");
    let mut out = Vec::new();
    let mut i = 8;
    while i < png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let ty: [u8; 4] = png[i + 4..i + 8].try_into().unwrap();
        let data = png[i + 8..i + 8 + len].to_vec();
        let crc = u32::from_be_bytes(png[i + 8 + len..i + 12 + len].try_into().unwrap());
        let mut covered = ty.to_vec();
        covered.extend_from_slice(&data);
        assert_eq!(crc, crc32(&covered), "CRC of chunk {}", String::from_utf8_lossy(&ty));
        out.push((ty, data));
        i += 12 + len;
    }
    out
}

/// The zlib payload from stored deflate blocks; checks the header and Adler-32.
fn inflate_stored(idat: &[u8]) -> Vec<u8> {
    assert_eq!(&idat[..2], &[0x78, 0x01], "zlib header");
    let body = &idat[2..idat.len() - 4];
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let last = body[i] & 1 == 1;
        assert_eq!((body[i] >> 1) & 3, 0, "stored block");
        let len = u16::from_le_bytes([body[i + 1], body[i + 2]]) as usize;
        let nlen = u16::from_le_bytes([body[i + 3], body[i + 4]]) as usize;
        assert_eq!(len, !nlen & 0xFFFF);
        out.extend_from_slice(&body[i + 5..i + 5 + len]);
        i += 5 + len;
        if last {
            break;
        }
    }
    let tail = u32::from_be_bytes(idat[idat.len() - 4..].try_into().unwrap());
    assert_eq!(tail, adler32(&out), "Adler-32");
    out
}

fn week_bars() -> Vec<Bar> {
    let logged = |kcal: f64, veg: bool| Bar { kcal, logged: true, veg };
    let unlogged = Bar { kcal: 0.0, logged: false, veg: false };
    vec![
        logged(2500.0, true), // slot 0: at or over target, veg
        logged(1200.0, false), // slot 1: under low
        unlogged,              // slot 2: not logged
        logged(2000.0, false), // slot 3: under target
        logged(2300.0, false),
        logged(2450.0, true),
        logged(2600.0, false),
    ]
}

fn slot_centre(i: u32, n: u32) -> u32 {
    20 + (520 * i / n) + (520 / n) / 2
}

#[test]
fn week_png_has_signature_ihdr_and_verifiable_chunks() {
    let bytes = charts::week_png(&week_bars(), 2400.0, 1800.0, [120.0, 300.0, 80.0]);
    let parts = chunks(&bytes);
    assert_eq!(&parts[0].0, b"IHDR");
    let ihdr = &parts[0].1;
    assert_eq!(u32::from_be_bytes(ihdr[0..4].try_into().unwrap()), 560);
    assert_eq!(u32::from_be_bytes(ihdr[4..8].try_into().unwrap()), 280);
    assert_eq!(ihdr[8], 8, "bit depth");
    assert_eq!(ihdr[9], 3, "colour type: indexed");
    assert!(parts.iter().any(|(t, _)| t == b"PLTE"));
    assert_eq!(&parts.last().unwrap().0, b"IEND");
}

#[test]
fn stored_blocks_decode_to_filtered_rows() {
    let canvas = charts::week_canvas(&week_bars(), 2400.0, 1800.0, [120.0, 300.0, 80.0]);
    let bytes = png::encode(&canvas, &charts::PALETTE);
    let idat: Vec<u8> = chunks(&bytes).into_iter().filter(|(t, _)| t == b"IDAT").flat_map(|(_, d)| d).collect();
    let raw = inflate_stored(&idat);
    let mut want = Vec::new();
    for y in 0..canvas.h {
        want.push(0u8);
        for x in 0..canvas.w {
            want.push(canvas.get(x, y));
        }
    }
    assert_eq!(raw.len(), (canvas.h * (canvas.w + 1)) as usize);
    assert_eq!(raw, want);
}

#[test]
fn sips_reads_the_width_when_present() {
    let sips = "/usr/bin/sips";
    if !std::path::Path::new(sips).exists() {
        return;
    }
    let path = std::env::temp_dir().join(format!("pip-v2-week-{}.png", std::process::id()));
    std::fs::write(&path, charts::week_png(&week_bars(), 2400.0, 1800.0, [120.0, 300.0, 80.0])).unwrap();
    let out = Command::new(sips).args(["-g", "pixelWidth"]).arg(&path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("pixelWidth: 560"), "{text}");
}

#[test]
fn logged_day_at_target_is_green_and_low_day_is_coral() {
    let c = charts::week_canvas(&week_bars(), 2400.0, 1800.0, [120.0, 300.0, 80.0]);
    assert_eq!(c.get(slot_centre(0, 7), 219), AT_TARGET);
    assert_eq!(c.get(slot_centre(1, 7), 219), UNDER_LOW);
    assert_eq!(c.get(slot_centre(3, 7), 219), UNDER_TARGET);
    assert_eq!(BG, 0);
    assert_eq!(TARGET_LINE, 5);
    assert_eq!(NO_VEG, 7);
}

#[test]
fn unlogged_day_draws_no_bar_colour_above_the_baseline() {
    let c = charts::week_canvas(&week_bars(), 2400.0, 1800.0, [120.0, 300.0, 80.0]);
    let x0 = 20 + 520 * 2 / 7;
    let x1 = 20 + 520 * 3 / 7;
    for x in x0..x1 {
        let p = c.get(x, 200);
        assert!(p != AT_TARGET && p != UNDER_LOW && p != UNDER_TARGET, "bar colour {p} at x={x}");
    }
    assert_eq!(c.get(slot_centre(2, 7), 218), UNLOGGED, "unlogged stub");
}

#[test]
fn veg_marks_only_on_logged_days() {
    let c = charts::week_canvas(&week_bars(), 2400.0, 1800.0, [120.0, 300.0, 80.0]);
    let on_slot = |i: u32| -> Vec<u8> {
        let (a, b) = (20 + 520 * i / 7, 20 + 520 * (i + 1) / 7);
        (a..b).map(|x| c.get(x, 233)).collect()
    };
    assert!(on_slot(0).iter().any(|&p| p == VEG || p == NO_VEG), "veg slot has a square");
    assert!(!on_slot(2).iter().any(|&p| p == VEG || p == NO_VEG), "unlogged day has none");
}

#[test]
fn day_bars_fill_to_the_share_of_target() {
    let c = charts::day_canvas([1200.0, 60.0, 150.0, 40.0], [2400.0, 120.0, 300.0, 80.0]);
    assert_ne!(c.get(150, 34), 1, "kcal track fills at x=150");
    assert_eq!(c.get(400, 34), 1, "kcal track is empty past 50%");
    let bytes = charts::day_png([1200.0, 60.0, 150.0, 40.0], [2400.0, 120.0, 300.0, 80.0]);
    assert_eq!(&bytes[..8], &SIG);
}

#[test]
fn palette_has_twelve_entries_and_index_names_match() {
    assert_eq!(charts::PALETTE.len(), 12);
    assert_eq!(charts::PALETTE[BG as usize], [43, 27, 46]);
    assert_eq!(UNLOGGED, 11);
    let _unused: Canvas = Canvas::new(1, 1, BG);
}
