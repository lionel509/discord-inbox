//! Local wall-clock stamps (#40): `received:` front matter, note filenames and Inbox stems.

use chrono::{DateTime, FixedOffset, Local, Offset, TimeZone, Utc};

/// `2026-10-14T23:06:00-04:00`: wall time at `offset` with its offset. The first 19 chars keep the shape dump and vaults slice.
pub fn stamp_in(utc: DateTime<Utc>, offset: FixedOffset) -> String {
    utc.with_timezone(&offset).format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// `stamp_in` at the machine's local offset for that instant (DST-aware).
pub fn local_stamp(utc: DateTime<Utc>) -> String {
    let offset = Local.offset_from_utc_datetime(&utc.naive_utc()).fix();
    stamp_in(utc, offset)
}
