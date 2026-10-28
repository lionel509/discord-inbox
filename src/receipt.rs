//! Receipt photos read by the local OCR: is the text a receipt, when was it printed, and which lines are food.
//! Pure: no I/O and no logging. Only the cleaned `items` and `drinks` strings and the parsed date leave here.
use chrono::NaiveDate;

/// What a receipt read gives back. `printed` is `("YYYY-MM-DD", "HH:MM")`.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub printed: Option<(String, String)>,
    pub items: Vec<String>,
    pub drinks: Vec<String>,
}

/// Where the logged time came from.
#[derive(Debug, Clone, PartialEq)]
pub enum Stamp {
    Printed,
    PrintedYesterday,
    NotFound,
    OutOfRange(String),
}

/// Shown on a drink line, which is listed under "Not counted" because the receipt has no size.
pub const DRINK_HINT: &str = "size and regular/diet aren't on the receipt, so not counted; send e.g. regular soda 20 fl oz or diet soda";

/// Lines with these words are card, payment and store lines, never food.
const SKIP: &[&str] = &[
    "tax", "tip", "balance", "payment", "paid", "change", "card", "visa", "mastercard", "amex",
    "discover", "cash", "credit", "debit", "auth", "approval", "approved", "order", "check", "table", "server",
    "cashier", "guest", "receipt", "thank", "store", "tel", "phone", "ref", "transaction", "coupon", "discount",
    "promo", "savings",
];
const DRINK_WORDS: &[&str] = &[
    "soda", "drink", "beverage", "pop", "fountain", "cola", "coke", "pepsi", "sprite", "lemonade", "tea", "juice",
];
/// OCR noise and campus shorthand, fixed in order on a space-padded line.
const FIXES: &[(&str, &str)] = &[
    (" w- ", " with "),
    (" w/ ", " with "),
    (" c ajun ", " cajun "),
    (" gra ", " gravy "),
    (" grvy ", " gravy "),
    (" combo ", " "),
];

/// Whether the OCR text is a receipt: a total line and a price somewhere. Menus, nutrition labels and food photos fail.
/// A total line with a price on it or on the line straight after (Vision can split the price onto the next line).
pub fn is_receipt(ocr: &str) -> bool {
    let lines: Vec<&str> = ocr.lines().collect();
    lines.iter().enumerate().any(|(i, l)| {
        is_total_line(l)
            && (l.split_whitespace().last().is_some_and(is_price_token)
                || lines.get(i + 1).is_some_and(|next| price_only(next)))
    })
}

/// `YYYY-MM-DD` for the day before `day`, or `None` when `day` isn't a date.
pub fn prev_day(day: &str) -> Option<String> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()?
        .pred_opt()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// Chooses the logged day and time. The printed time counts when it is on the send day and no more than ten
/// minutes after the send time, or on the day before. Anything else is logged at the send time, with the reason.
pub fn pick_time(printed: Option<(String, String)>, msg_day: &str, msg_time: &str) -> (String, String, Stamp) {
    let Some((d, t)) = printed else {
        return (msg_day.to_string(), msg_time.to_string(), Stamp::NotFound);
    };
    let within = match (minutes(&t), minutes(msg_time)) {
        (Some(a), Some(b)) => a <= b + 10,
        _ => false,
    };
    if d == msg_day && within {
        return (d, t, Stamp::Printed);
    }
    if prev_day(msg_day).as_deref() == Some(d.as_str()) {
        return (d, t, Stamp::PrintedYesterday);
    }
    let stamp = Stamp::OutOfRange(format!("{d} {t}"));
    (msg_day.to_string(), msg_time.to_string(), stamp)
}

/// Reads a receipt's printed time and its food and drink lines.
pub fn read(ocr: &str) -> Receipt {
    let printed = printed(ocr);
    let lines: Vec<&str> = ocr.lines().collect();
    let end = lines.iter().position(|l| is_total_line(l)).unwrap_or(lines.len());
    let body: Vec<&str> = lines[..end].iter().copied().filter(|l| !has_word(l, SKIP)).collect();
    // Marked mode: the `>` lines are the items. Otherwise the priced lines are.
    let marked = body.iter().any(|l| l.trim_start().starts_with('>'));
    let mut items = Vec::new();
    let mut drinks = Vec::new();
    for l in body {
        if marked {
            if !l.trim_start().starts_with('>') {
                continue;
            }
        } else if !has_price(l) {
            continue;
        }
        let Some(c) = clean(l) else { continue };
        if c == "water" || c == "bottled water" {
            continue;
        }
        if words(&c).iter().any(|w| DRINK_WORDS.contains(&w.as_str())) {
            drinks.push(c);
        } else {
            items.push(c);
        }
    }
    Receipt { printed, items, drinks }
}

fn words(line: &str) -> Vec<String> {
    line.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn has_word(line: &str, set: &[&str]) -> bool {
    words(line).iter().any(|w| set.contains(&w.as_str()))
}

fn is_total_line(line: &str) -> bool {
    has_word(line, &["total", "subtotal"])
}

/// `12.34`, with an optional leading `-` or a `$` before it (a glued `TOTAL$12.34` counts).
fn is_price_token(tok: &str) -> bool {
    let t = tok.rsplit('$').next().unwrap_or(tok).trim_start_matches('-');
    let Some((a, b)) = t.split_once('.') else { return false };
    !a.is_empty()
        && a.bytes().all(|c| c.is_ascii_digit())
        && b.len() == 2
        && b.bytes().all(|c| c.is_ascii_digit())
}

/// Every token is `$` or a price, and at least one is a price.
fn price_only(line: &str) -> bool {
    let mut any = false;
    for t in line.split_whitespace() {
        if is_price_token(t) {
            any = true;
        } else if t != "$" {
            return false;
        }
    }
    any
}

fn has_price(line: &str) -> bool {
    line.split_whitespace().any(is_price_token)
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One item line, cleaned: the marker and prices gone, OCR noise fixed, `2pc` split, a missing `fried` added.
/// `None` when nothing is left.
fn clean(line: &str) -> Option<String> {
    let s = line.trim().trim_start_matches(|c: char| c == '>' || c == '*' || c == '-' || c.is_whitespace());
    let mut kept: Vec<String> = Vec::new();
    for tok in s.split_whitespace() {
        if tok == "$" || tok == "-" {
            continue;
        }
        if let Some(i) = tok.find('$') {
            let pre = &tok[..i];
            if !pre.is_empty() {
                kept.push(pre.to_string());
            }
            continue;
        }
        if is_price_token(tok) {
            continue;
        }
        kept.push(tok.to_string());
    }
    let joined = collapse(&kept.join(" ").to_lowercase().replace("w/", "w/ "));
    let mut padded = format!(" {joined} ");
    for (from, to) in FIXES {
        padded = padded.replace(from, to);
    }
    // Split `2pc` into `2 pc`.
    let mut toks: Vec<String> = Vec::new();
    for t in padded.split_whitespace() {
        let digits = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        let (num, unit) = t.split_at(digits);
        if !num.is_empty() && (unit == "pc" || unit == "pcs") {
            toks.push(num.to_string());
            toks.push("pc".to_string());
        } else {
            toks.push(t.to_string());
        }
    }
    let mut fin: Vec<String> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        fin.push(t.clone());
        if t == "pc" && toks.get(i + 1).is_some_and(|n| n == "chicken") {
            fin.push("fried".to_string());
        }
    }
    let out = fin.join(" ");
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The first line with a date, and the time on it or on the line after.
fn printed(ocr: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = ocr.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let Some(day) = toks.iter().find_map(|t| find_date(t)) else { continue };
        let time = find_time(&toks).or_else(|| {
            let next: Vec<&str> = lines.get(i + 1)?.split_whitespace().collect();
            find_time(&next)
        })?;
        return Some((day, format!("{:02}:{:02}", time.0, time.1)));
    }
    None
}

fn num(s: &str, max_len: usize) -> Option<u32> {
    if s.is_empty() || s.len() > max_len || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `M/D/YYYY`, `M/D/YY` (read as 20YY), or `YYYY-MM-DD`, as `YYYY-MM-DD`.
fn find_date(tok: &str) -> Option<String> {
    let (y, m, d) = if tok.contains('/') {
        let p: Vec<&str> = tok.split('/').collect();
        if p.len() != 3 {
            return None;
        }
        let y = num(p[2], 4).filter(|_| p[2].len() == 2 || p[2].len() == 4)?;
        let y = if p[2].len() == 2 { 2000 + y } else { y };
        (y, num(p[0], 2)?, num(p[1], 2)?)
    } else if tok.len() == 10 && tok.contains('-') {
        let p: Vec<&str> = tok.split('-').collect();
        if p.len() != 3 || p[0].len() != 4 {
            return None;
        }
        (num(p[0], 4)?, num(p[1], 2)?, num(p[2], 2)?)
    } else {
        return None;
    };
    NaiveDate::from_ymd_opt(y as i32, m, d).map(|d| d.format("%Y-%m-%d").to_string())
}

/// `H:MM` or `H:MM:SS`, with an optional `AM`/`PM` glued on or as the next token. Returns 24-hour `(h, m)`.
fn find_time(toks: &[&str]) -> Option<(u32, u32)> {
    for (i, t) in toks.iter().enumerate() {
        let raw = t.to_ascii_lowercase().replace('.', "");
        let (core, mer) = if let Some(c) = raw.strip_suffix("am") {
            (c.to_string(), Some(false))
        } else if let Some(c) = raw.strip_suffix("pm") {
            (c.to_string(), Some(true))
        } else {
            (raw.clone(), None)
        };
        let Some((h, m)) = clock(&core) else { continue };
        let mer = mer.or_else(|| toks.get(i + 1).and_then(|n| meridiem(n)));
        let h = match mer {
            Some(pm) => {
                if h == 0 || h > 12 {
                    continue;
                }
                match (pm, h) {
                    (true, 12) => 12,
                    (true, h) => h + 12,
                    (false, 12) => 0,
                    (false, h) => h,
                }
            }
            None => h,
        };
        if h < 24 && m < 60 {
            return Some((h, m));
        }
    }
    None
}

fn meridiem(tok: &str) -> Option<bool> {
    match tok.to_ascii_lowercase().replace('.', "").as_str() {
        "am" => Some(false),
        "pm" => Some(true),
        _ => None,
    }
}

fn clock(s: &str) -> Option<(u32, u32)> {
    let p: Vec<&str> = s.split(':').collect();
    if p.len() != 2 && p.len() != 3 {
        return None;
    }
    let h = num(p[0], 2)?;
    let m = num(p[1], 2).filter(|_| p[1].len() == 2)?;
    if p.len() == 3 {
        num(p[2], 2).filter(|_| p[2].len() == 2)?;
    }
    Some((h, m))
}

fn minutes(hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.split_once(':')?;
    Some(h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?)
}
