//! Local wall-clock stamps for `received:`, filenames and Inbox note stems (#40). Pure functions only; no clock reads.
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use discord_inbox::dump::note_path;
use discord_inbox::vaults;
use discord_inbox::when::stamp_in;
use std::path::Path;

fn utc(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

#[test]
fn stamp_in_west_offset_gives_local_wall_time() {
    let off = FixedOffset::west_opt(4 * 3600).unwrap();
    assert_eq!(stamp_in(utc("2026-10-15T03:06:00Z"), off), "2026-10-14T23:06:00-04:00");
}

#[test]
fn stamp_in_half_hour_east_offset() {
    let off = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
    assert_eq!(stamp_in(utc("2026-10-15T03:06:00Z"), off), "2026-10-15T08:36:00+05:30");
}

fn digits(b: &[u8]) -> bool {
    b.iter().all(|c| c.is_ascii_digit())
}

#[test]
fn stamp_keeps_shape_and_local_date_for_every_offset() {
    // 200 instants spread across 2026, and every half-hour offset from -12:00 to +14:00.
    let start = utc("2026-01-01T00:00:00Z").timestamp();
    let mut checked = 0;
    for i in 0..200i64 {
        let instant = Utc.timestamp_opt(start + i * 157_777, 0).unwrap();
        for half_hours in -24..=28i32 {
            let off = FixedOffset::east_opt(half_hours * 1800).unwrap();
            let s = stamp_in(instant, off);
            let b = s.as_bytes();
            assert_eq!(s.len(), 25, "{s}");
            assert!(digits(&b[0..4]) && b[4] == b'-', "{s}");
            assert!(digits(&b[5..7]) && b[7] == b'-', "{s}");
            assert!(digits(&b[8..10]) && b[10] == b'T', "{s}");
            assert!(digits(&b[11..13]) && b[13] == b':', "{s}");
            assert!(digits(&b[14..16]) && b[16] == b':', "{s}");
            assert!(digits(&b[17..19]), "{s}");
            assert!(b[19] == b'+' || b[19] == b'-', "{s}");
            assert!(digits(&b[20..22]) && b[22] == b':' && digits(&b[23..25]), "{s}");

            let want_date = instant.with_timezone(&off).format("%Y-%m-%d").to_string();
            assert!(s.starts_with(&want_date), "{s} vs {want_date}");
            let name = vaults::note_name(&s, "");
            assert!(name.starts_with(&want_date), "{name}");
            let p = note_path(Path::new("/scratch/Inbox"), &s, 1);
            let fname = p.file_name().unwrap().to_string_lossy().into_owned();
            assert!(fname.starts_with(&want_date), "{fname}");
            checked += 1;
        }
    }
    assert_eq!(checked, 200 * 53);
}
