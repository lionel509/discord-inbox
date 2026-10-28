CREATE TABLE Calendar (ROWID INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT);
CREATE TABLE CalendarItem (ROWID INTEGER PRIMARY KEY AUTOINCREMENT, summary TEXT, start_date REAL, start_tz TEXT, end_date REAL, all_day INTEGER, calendar_id INTEGER, has_recurrences INTEGER, UUID TEXT);
CREATE TABLE OccurrenceCache (day REAL, event_id INTEGER, calendar_id INTEGER, store_id INTEGER, occurrence_date REAL, occurrence_start_date REAL, occurrence_end_date REAL);
