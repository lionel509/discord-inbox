//! One deterministic pass over every text that is written to disk, sent to a model, or sent back to Discord:
//! card numbers, US SSNs, labelled bank and routing numbers, and secrets. No model, std only. Card digits go
//! through `insight::mask_cards`. A false mask is the cheaper error, but a few labels mark a number as a store
//! reference (order, station, and so on), so those digit runs are left alone unless they pass Luhn at card length.

use crate::insight::mask_cards;

const REDACTED: &str = "[redacted]";

/// Store labels whose long digit runs are references, not cards.
const STORE_LABELS: [&str; 9] =
    ["transaction", "reference", "cashier", "station", "invoice", "order", "receipt", "txn", "ref"];
/// Labels whose digits (6 to 17 of them) are a bank account or routing number: all but the last four are masked.
const ACCOUNT_LABELS: [&str; 4] = ["account", "acct", "routing", "aba"];
/// Labels whose value follows `=` or `:`, or for passwords also the word `is`.
const PASSWORD_LABELS: [&str; 3] = ["password", "passwd", "pwd"];
/// Labels whose value follows `=` or `:` only.
const SECRET_LABELS: [&str; 4] = ["api_key", "apikey", "token", "secret"];

/// A token with a fixed prefix. `keep` keeps the prefix (`Bearer `) and masks only the value after it.
struct Token {
    prefix: &'static str,
    ci: bool,
    min: usize,
    keep: bool,
    allowed: fn(char) -> bool,
}

const TOKENS: &[Token] = &[
    Token { prefix: "Authorization: Basic ", ci: true, min: 8, keep: true, allowed: token_char },
    Token { prefix: "Authorization: Bearer ", ci: true, min: 8, keep: true, allowed: token_char },
    Token { prefix: "sk-", ci: false, min: 16, keep: false, allowed: key_char },
    Token { prefix: "sk_live_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "sk_test_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "rk_live_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "pk_live_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "gho_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "ghu_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "ghs_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "ghr_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "shpat_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "shpss_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "shpca_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "shppa_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "ghp_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "github_pat_", ci: false, min: 16, keep: false, allowed: word_char },
    Token { prefix: "xoxa-", ci: false, min: 10, keep: false, allowed: key_char },
    Token { prefix: "xoxb-", ci: false, min: 10, keep: false, allowed: key_char },
    Token { prefix: "xoxp-", ci: false, min: 10, keep: false, allowed: key_char },
    Token { prefix: "xoxr-", ci: false, min: 10, keep: false, allowed: key_char },
    Token { prefix: "AKIA", ci: false, min: 16, keep: false, allowed: upper_or_digit },
    Token { prefix: "Bearer ", ci: true, min: 20, keep: true, allowed: token_char },
];

/// Masks every sensitive value in `text`. Numbers first, then secrets, so each value is masked once.
pub fn sensitive(text: &str) -> String {
    let text = mask_numbers(text);
    let text = mask_ssns(&text);
    let text = mask_labelled_accounts(&text);
    mask_secrets(&text)
}

fn digit(c: char) -> bool {
    c.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&c)
}

pub(crate) fn digit_value(c: char) -> u32 {
    if c.is_ascii_digit() {
        c as u32 - '0' as u32
    } else {
        c as u32 - 0xff10
    }
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn key_char(c: char) -> bool {
    word_char(c) || c == '-'
}

fn token_char(c: char) -> bool {
    word_char(c) || matches!(c, '.' | '~' | '+' | '/' | '=' | '-')
}

fn upper_or_digit(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

/// A gap inside a card or reference number: whitespace, `-`, en dash or em dash.
fn is_gap(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '\u{2013}' | '\u{2014}')
}

/// Spaces, tabs, `#`, `:` and `.` between a label and its number.
fn marks(c: &[char], mut q: usize) -> usize {
    while q < c.len() && matches!(c[q], ' ' | '\t' | '#' | ':' | '.') {
        q += 1;
    }
    q
}

fn spaces(c: &[char], mut q: usize) -> usize {
    while q < c.len() && matches!(c[q], ' ' | '\t') {
        q += 1;
    }
    q
}

/// True when the word `w` starts at `i`, ignoring ASCII case, and is a whole word.
fn word_at(c: &[char], i: usize, w: &str) -> bool {
    let n = w.chars().count();
    i + n <= c.len()
        && (i == 0 || !c[i - 1].is_alphanumeric())
        && (i + n == c.len() || !c[i + n].is_alphanumeric())
        && c[i..i + n].iter().zip(w.chars()).all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

/// Skips an optional `no` or `number` word after a label, then the separators.
fn after_label(c: &[char], q: usize) -> usize {
    let q = marks(c, q);
    for w in ["no", "number", "id"] {
        if word_at(c, q, w) {
            return marks(c, q + w.len());
        }
    }
    q
}

/// The end of a digit run that starts at `q`, with single gaps allowed between digits, and its digit values.
/// A run never crosses a line break.
fn run_end(c: &[char], q: usize) -> (usize, Vec<u32>) {
    let (mut j, mut end, mut digits) = (q, q, Vec::new());
    while j < c.len() {
        if digit(c[j]) {
            digits.push(digit_value(c[j]));
            j += 1;
            end = j;
        } else if is_gap(c[j]) && c[j] != '\n' && c[j] != '\r' && j + 1 < c.len() && digit(c[j + 1]) {
            j += 1;
        } else {
            break;
        }
    }
    (end, digits)
}

pub(crate) fn luhn_ok(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

/// Keeps the digit run at `q` as a reference when it is 12 to 19 digits and not a Luhn-valid card of 13 to 19.
/// A run over 19 digits, or one that passes Luhn, is left for `mask_cards`.
fn keep_span(c: &[char], q: usize, spans: &mut Vec<(usize, usize)>) {
    if q >= c.len() || !digit(c[q]) {
        return;
    }
    let (end, digits) = run_end(c, q);
    let count = digits.len();
    if !(12..=19).contains(&count) || (count >= 13 && luhn_ok(&digits)) {
        return;
    }
    if spans.last().is_none_or(|&(_, e)| q >= e) {
        spans.push((q, end));
    }
}

/// Digit runs of 12 to 19 after a store label or a `+` country code. A 13 to 19 digit run that passes Luhn is a card.
fn kept_spans(c: &[char]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for i in 0..c.len() {
        // An international phone number: `+86 138 0013 8000`.
        if c[i] == '+' && (i == 0 || !c[i - 1].is_alphanumeric()) {
            keep_span(c, i + 1, &mut spans);
        }
        if let Some(label) = STORE_LABELS.iter().find(|l| word_at(c, i, l)) {
            let q = after_label(c, i + label.len());
            keep_span(c, q, &mut spans);
        }
    }
    spans
}

/// Card digits: `mask_cards` on every piece of the text outside a store reference.
fn mask_numbers(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (s, e) in kept_spans(&c) {
        out.push_str(&mask_cards(&c[at..s].iter().collect::<String>()));
        out.extend(&c[s..e]);
        at = e;
    }
    out.push_str(&mask_cards(&c[at..].iter().collect::<String>()));
    out
}

/// `123456789`, `123 45 6789` or `123-45-6789` at `q` with no digit after: the index just past it.
fn loose_ssn_end(c: &[char], q: usize) -> Option<usize> {
    let mut j = q;
    for (k, n) in [3usize, 2, 4].into_iter().enumerate() {
        if k > 0 && j < c.len() && matches!(c[j], ' ' | '-') {
            j += 1;
        }
        if j + n > c.len() || !c[j..j + n].iter().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        j += n;
    }
    (!c.get(j).is_some_and(|ch| ch.is_ascii_digit())).then_some(j)
}

/// `###-##-####` with no digit on either side: masked as `***-**-1234`. After an `SSN` label the dashes and
/// spaces are optional, so `SSN 123456789` and `SSN: 123 45 6789` are masked too.
fn mask_ssns(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let is_ssn = |s: &[char]| {
        s.iter().enumerate().all(|(k, &ch)| match k {
            3 | 6 => ch == '-',
            _ => ch.is_ascii_digit(),
        })
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        if word_at(&c, i, "ssn") {
            let q = after_label(&c, i + 3);
            if let Some(end) = loose_ssn_end(&c, q) {
                out.extend(&c[i..q]);
                out.push_str("***-**-");
                out.extend(&c[end - 4..end]);
                i = end;
                continue;
            }
        }
        let bounded = (i == 0 || !c[i - 1].is_ascii_digit()) && c.get(i + 11).is_none_or(|ch| !ch.is_ascii_digit());
        if i + 11 <= c.len() && bounded && is_ssn(&c[i..i + 11]) {
            out.push_str("***-**-");
            out.extend(&c[i + 7..i + 11]);
            i += 11;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Account and routing numbers after their label: 6 to 17 digits, all but the last four masked.
fn mask_labelled_accounts(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut hide = vec![false; c.len()];
    for i in 0..c.len() {
        let Some(label) = ACCOUNT_LABELS.iter().find(|l| word_at(&c, i, l)) else {
            continue;
        };
        let q = after_label(&c, i + label.len());
        let mut j = q;
        while j < c.len() && digit(c[j]) {
            j += 1;
        }
        if (6..=17).contains(&(j - q)) {
            for h in hide.iter_mut().take(j - 4).skip(q) {
                *h = true;
            }
        }
    }
    c.iter()
        .zip(hide)
        .map(|(&ch, h)| if h && digit(ch) { '*' } else { ch })
        .collect()
}

/// A secret starting at `i`, as (end of the kept part, end of the secret), or `None`.
fn secret_at(c: &[char], i: usize) -> Option<(usize, usize)> {
    if i > 0 && word_char(c[i - 1]) {
        return None;
    }
    // The password in `scheme://user:password@host`: the part between the first `:` and the `@`.
    if i >= 3 && c[i - 3..i] == [':', '/', '/'] {
        let stop = c[i..].iter().position(|&ch| ch == '/' || ch.is_whitespace()).map_or(c.len(), |p| i + p);
        let auth = &c[i..stop];
        if let Some(at) = auth.iter().rposition(|&ch| ch == '@') {
            if let Some(colon) = auth[..at].iter().position(|&ch| ch == ':') {
                if colon + 1 < at {
                    return Some((i + colon + 1, i + at));
                }
            }
        }
    }
    for t in TOKENS.iter() {
        let n = t.prefix.len();
        let matched = i + n <= c.len()
            && c[i..i + n].iter().zip(t.prefix.chars()).all(|(a, b)| {
                if t.ci { a.eq_ignore_ascii_case(&b) } else { *a == b }
            });
        if matched {
            let run = c[i + n..].iter().take_while(|&&ch| (t.allowed)(ch)).count();
            if run >= t.min {
                return Some((if t.keep { i + n } else { i }, i + n + run));
            }
        }
    }
    for label in PASSWORD_LABELS.iter().chain(SECRET_LABELS.iter()) {
        if !word_at(c, i, label) {
            continue;
        }
        let q = spaces(c, i + label.len());
        let sep_end = if q < c.len() && (c[q] == '=' || c[q] == ':') {
            q + 1
        } else if PASSWORD_LABELS.contains(label) && word_at(c, q, "is") {
            { let s = spaces(c, q + 2); if c.get(s) == Some(&':') { s + 1 } else { q + 2 } }
        } else {
            continue;
        };
        let v = spaces(c, sep_end);
        let run = c[v..].iter().take_while(|ch| !ch.is_whitespace()).count();
        if run > 0 {
            return Some((v, v + run));
        }
    }
    None
}

/// API keys, tokens, Bearer values and password values, each replaced by `[redacted]`.
fn mask_secrets(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        match secret_at(&c, i) {
            Some((kept, end)) => {
                out.extend(&c[i..kept]);
                out.push_str(REDACTED);
                i = end;
            }
            None => {
                out.push(c[i]);
                i += 1;
            }
        }
    }
    out
}
