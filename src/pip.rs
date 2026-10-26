//! Pip's handler (U6): one DM in, one day-note block out, one card back. Commands (`today`, `week`, `undo`,
//! `fix`, `help`) are read before any meal parsing and sit behind the same allowlist as meals. A message
//! that is not food is a note, never a meal, so the 21:00 check-in still fires.
//! Nutrition comes from the local table only (`nutrition`); recipes are read, never written (`recipes`).
use crate::allowlist::Allowlist;
use crate::daylog::{self, Meal, NoteMsg, Totals, Written};
use crate::ingest::{Handle, Inbound, Outcome, FAILED_MARK, SAVED_MARK};
use crate::nutrition::{self, Parsed};
use crate::recipes::{self, Recipe, Scaled};
use crate::rich::Rich;
use crate::summary;
use std::path::PathBuf;

/// Pip's public presence: persona only, nothing about food or health.
pub const PRESENCE: &str = "curled up on a warm laptop 💤";
/// Used when `PIP_KCAL_TARGET` is unset or unparsable. A placeholder until he sets his own.
pub const DEFAULT_KCAL_TARGET: u32 = 2400;

pub const HELP: &str = "Send what you ate, one item per line: salmon 100g, cooked rice 180g, cabbage 85g, or a recipe name (add a count like 2 x, or a weight).\nCommands: today (the day's card), week (the last seven days), undo (drop the last meal), fix <meal> (replace the last meal), help.\nA message with no food in it is saved as a note, not a meal.";

const FIX_USAGE: &str = "Send fix with the replacement meal, e.g. fix salmon 120g. It replaces the last meal logged today.";

#[derive(Clone, Debug)]
pub struct Config {
    pub nutrition_dir: PathBuf,
    pub recipes_dir: Option<PathBuf>,
    pub low_kcal: u32,
    pub kcal_target: u32,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let nd = std::env::var("PIP_NUTRITION_DIR")
            .map_err(|_| "PIP_NUTRITION_DIR is not set; point it at the nutrition notes folder".to_string())?;
        let nutrition_dir = PathBuf::from(&nd);
        if nd.trim().is_empty() || !nutrition_dir.is_absolute() {
            return Err(format!("PIP_NUTRITION_DIR must be an absolute path, got {nd:?}"));
        }
        let recipes_dir = match std::env::var("PIP_RECIPES_DIR") {
            Ok(r) if !r.trim().is_empty() => {
                let p = PathBuf::from(&r);
                if !p.is_absolute() {
                    return Err(format!("PIP_RECIPES_DIR must be an absolute path, got {r:?}"));
                }
                Some(p)
            }
            _ => None,
        };
        let kcal_target = std::env::var("PIP_KCAL_TARGET")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|k| *k > 0)
            .unwrap_or(DEFAULT_KCAL_TARGET);
        Ok(Config { nutrition_dir, recipes_dir, low_kcal: daylog::low_kcal_threshold(), kcal_target })
    }
}

/// A chat command, read from the whole message before any meal parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Today,
    Week,
    Undo,
    Help,
    Fix(String),
}

/// The command a message is, if any. A leading `/` or `!` is allowed; `fix` takes the rest as the meal.
pub fn command(text: &str) -> Option<Command> {
    let t = text.trim();
    let t = t.strip_prefix('/').or_else(|| t.strip_prefix('!')).unwrap_or(t).trim();
    match t.to_lowercase().as_str() {
        "today" | "day" => return Some(Command::Today),
        "week" | "weekly" => return Some(Command::Week),
        "undo" => return Some(Command::Undo),
        "help" | "?" | "commands" => return Some(Command::Help),
        _ => {}
    }
    if !t.get(..3)?.eq_ignore_ascii_case("fix") {
        return None;
    }
    let after = &t[3..];
    if !after.is_empty() && !after.starts_with(|c: char| c.is_whitespace() || c == ':') {
        return None;
    }
    Some(Command::Fix(after.trim_matches(|c: char| c == ':' || c.is_whitespace()).to_string()))
}

/// What a message counts as: the typed lines, the recipe (if a name matched) with its scale, and what was assumed.
struct Analysis {
    parsed: Parsed,
    overridden: Vec<String>,
    recipe: Option<String>,
    skipped: Vec<String>,
    assumed: Vec<String>,
    is_meal: bool,
}

/// Recipe first (a name match scales it), then the typed lines, then the merge where typed rows win.
fn analyze(text: &str, recipes: &[Recipe]) -> Analysis {
    let hit = recipes::resolve(text, recipes);
    let typed_text = hit.as_ref().map(|h| h.rest.clone()).unwrap_or_else(|| text.to_string());
    let typed = nutrition::parse_meal(&typed_text);
    let Some(h) = hit else {
        let is_meal = !typed.items.is_empty() || nutrition::looks_like_food(&typed_text);
        return Analysis { parsed: typed, overridden: Vec::new(), recipe: None, skipped: Vec::new(), assumed: Vec::new(), is_meal };
    };
    let name = h.recipe.name.as_str();
    let (servings, how) = recipes::scale(&h, &typed);
    let mut scaled = h.clone();
    scaled.servings = servings;
    let pieces = recipes::ingredient_pieces(&scaled).join(", ");
    let (parsed, overridden) = nutrition::merge(typed, nutrition::parse_meal(&pieces));

    let mut label = format!("{name} ×{}", trim_num(servings));
    let mut assumed = h.recipe.assumed.clone();
    match &how {
        Scaled::Count => {}
        Scaled::Weight { food, grams } | Scaled::Typed { food, grams } => {
            label.push_str(&format!(" (scaled to your {} g {food})", trim_num(*grams)));
        }
        Scaled::Assumed => {
            let base = nutrition::parse_meal(&h.recipe.lines.join("\n"));
            match base.items.iter().max_by(|a, b| a.grams.total_cmp(&b.grams)) {
                Some(heaviest) => assumed.push(format!(
                    "Assumed 1 serving of {name}; type the {} weight to scale it.",
                    heaviest.food.name
                )),
                None => assumed.push(format!("Assumed 1 serving of {name}.")),
            }
        }
    }
    if h.fuzzy {
        label.push_str(&format!(" · matched \"{}\"", h.matched));
    }
    Analysis { parsed, overridden, recipe: Some(label), skipped: h.recipe.skipped.clone(), assumed, is_meal: true }
}

/// Today's kcal (rounded) and meal count, for a reply.
fn today_numbers(dir: &std::path::Path, day: &str, low: u32) -> (i64, usize) {
    let s = summary::day_stat(dir, day, low);
    (s.macros.kcal.round() as i64, s.meals)
}

/// One DM: a command is answered; anything else is logged (a meal, or a note when it has no food), then answered.
pub async fn handle<M: Inbound>(allow: &Allowlist, cfg: &Config, m: &M) -> Outcome {
    if !allow.contains(m.author_id()) {
        return Outcome::Ignored;
    }
    let text = m.content().trim().to_string();
    let files = m.attachment_names();
    if text.is_empty() && files.is_empty() {
        return Outcome::Ignored;
    }
    let received = m.received();
    let day = daylog::day_key(&received).to_string();
    let time = received.get(11..16).unwrap_or("00:00").to_string();
    let hhmm = time.replace(':', "");
    let msg_id = m.msg_id();

    // Already in the day note or the command ledger (a replay, or a reply that failed): nothing to do, no download.
    if daylog::logged(&cfg.nutrition_dir, &day, msg_id) || daylog::command_seen(&cfg.nutrition_dir, msg_id) {
        return Outcome::Skipped;
    }
    if let Err(e) = std::fs::create_dir_all(&cfg.nutrition_dir) {
        eprintln!("pip: nutrition dir {} not creatable: {e}", cfg.nutrition_dir.display());
        return Outcome::Failed;
    }
    if m.live() {
        m.typing().await;
    }

    // Commands come before meal parsing. `fix` is the one command that also takes a meal (and a photo).
    let fix_rest = match command(&text).filter(|c| matches!(c, Command::Fix(_)) || files.is_empty()) {
        Some(Command::Fix(rest)) => Some(rest),
        Some(other) => return run_command(cfg, m, &day, msg_id, &other).await,
        None => None,
    };

    let recipe_list = cfg.recipes_dir.as_deref().map(recipes::load).unwrap_or_default();
    let subject = fix_rest.clone().unwrap_or_else(|| text.clone());
    if fix_rest.is_some() {
        if let Err(e) = daylog::record_command(&cfg.nutrition_dir, msg_id) {
            eprintln!("pip: command ledger on {msg_id} failed: {e}");
            let _ = m.react(FAILED_MARK).await;
            return Outcome::Failed;
        }
    }
    let an = analyze(&subject, &recipe_list);
    if fix_rest.is_some() && (subject.is_empty() || !an.is_meal) {
        let _ = m.reply(FIX_USAGE).await;
        let _ = m.react(SAVED_MARK).await;
        return Outcome::Saved;
    }

    let mut photos = Vec::new();
    for (i, name) in files.iter().enumerate() {
        let path = daylog::photo_path(&cfg.nutrition_dir, &day, &hhmm, msg_id, i, name);
        let saved = match m.download(i).await {
            Ok(bytes) => daylog::save_photo(&path, &bytes).map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        if let Err(e) = saved {
            eprintln!("pip: photo {msg_id}/{i} failed: {e}");
            let _ = m.react(FAILED_MARK).await;
            return Outcome::Failed;
        }
        photos.push(path.file_name().unwrap_or_default().to_string_lossy().into_owned());
    }

    if !an.is_meal {
        // Not food: a note block (never counted), so replays skip it and the check-in is still due.
        let n = NoteMsg { msg_id, time, text: text.clone(), photos };
        match daylog::write_note(&cfg.nutrition_dir, &day, &n, cfg.low_kcal) {
            Ok(Written::Already) => return Outcome::Skipped,
            Ok(Written::Logged) => {}
            Err(e) => {
                eprintln!("pip: day note {day} failed: {e}");
                let _ = m.react(FAILED_MARK).await;
                return Outcome::Failed;
            }
        }
        let reply = if text.is_empty() {
            "Photo saved. I don't guess from photos; reply with what was in it (e.g. salmon 100g, cooked rice 180g).".to_string()
        } else {
            format!("No food in that, so it isn't logged as a meal: \"{text}\". Send items with weights (cooked rice 180g) or a recipe name; help lists commands.")
        };
        if let Err(e) = m.reply(&reply).await {
            eprintln!("pip: reply on {msg_id} failed: {e}");
        }
        let _ = m.react(SAVED_MARK).await;
        return Outcome::Saved;
    }

    if fix_rest.is_some() {
        match daylog::undo_last(&cfg.nutrition_dir, &day, cfg.low_kcal, msg_id) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = m.reply("Nothing logged today to fix; send it as a new meal.").await;
                let _ = m.react(SAVED_MARK).await;
                return Outcome::Saved;
            }
            Err(e) => {
                eprintln!("pip: undo for fix on {msg_id} failed: {e}");
                let _ = m.react(FAILED_MARK).await;
                return Outcome::Failed;
            }
        }
    }

    let parsed = &an.parsed;
    let recipe = an.recipe.as_ref().map(|label| {
        if an.overridden.is_empty() {
            label.clone()
        } else {
            format!("{label} (typed {} overrode the recipe)", an.overridden.join(", "))
        }
    });
    let note = if parsed.items.is_empty() && parsed.unknown.is_empty() { Some(text.clone()) } else { None };
    let meal = Meal {
        msg_id,
        time,
        lines: parsed.items.iter().map(nutrition::item_line).collect(),
        macros: nutrition::macros(&parsed.items),
        veg: nutrition::has_veg(&parsed.items),
        recipe,
        unknown: parsed.unknown.iter().map(|u| format!("{} ({})", u.text, u.hint)).collect(),
        note,
        photos,
    };
    match daylog::write_meal(&cfg.nutrition_dir, &day, &meal, cfg.low_kcal) {
        Ok(Written::Already) => return Outcome::Skipped,
        Ok(Written::Logged) => {}
        Err(e) => {
            eprintln!("pip: day note {day} failed: {e}");
            let _ = m.react(FAILED_MARK).await;
            return Outcome::Failed;
        }
    }

    // The note is on disk, so a failed reply is not a failed log: a replay sees the note and skips.
    let today = daylog::totals(&cfg.nutrition_dir, &day, cfg.low_kcal);
    let card = meal_card(&an, today.as_ref(), cfg.kcal_target);
    if let Err(e) = m.reply_rich(&card).await {
        eprintln!("pip: reply on {msg_id} failed: {e}");
    }
    let _ = m.react(SAVED_MARK).await;
    Outcome::Saved
}

/// Answers a command (never `fix`, which `handle` runs itself). The ledger is written before the action.
async fn run_command<M: Inbound>(cfg: &Config, m: &M, day: &str, msg_id: u64, cmd: &Command) -> Outcome {
    let dir = cfg.nutrition_dir.as_path();
    if let Err(e) = daylog::record_command(dir, msg_id) {
        eprintln!("pip: command ledger on {msg_id} failed: {e}");
        let _ = m.react(FAILED_MARK).await;
        return Outcome::Failed;
    }
    let reply = match cmd {
        Command::Help => m.reply(HELP).await,
        Command::Today => m.reply_rich(&summary::day_card(dir, day, cfg.low_kcal, cfg.kcal_target)).await,
        Command::Week => m.reply_rich(&summary::week_card(dir, day, cfg.low_kcal, cfg.kcal_target)).await,
        Command::Undo => match daylog::undo_last(dir, day, cfg.low_kcal, msg_id) {
            Ok(Some(row)) => {
                let (k, n) = today_numbers(dir, day, cfg.low_kcal);
                m.reply(&format!(
                    "Undid the {} meal ({} kcal). Today: {k} / {} kcal over {n} meal(s).",
                    row.time,
                    row.kcal.round() as i64,
                    cfg.kcal_target
                ))
                .await
            }
            Ok(None) => m.reply("Nothing logged today to undo.").await,
            Err(e) => {
                eprintln!("pip: undo on {msg_id} failed: {e}");
                let _ = m.react(FAILED_MARK).await;
                return Outcome::Failed;
            }
        },
        // `handle` runs `fix` itself, since it takes a meal; this arm is never reached.
        Command::Fix(_) => Ok(()),
    };
    if let Err(e) = reply {
        eprintln!("pip: reply on {msg_id} failed: {e}");
    }
    let _ = m.react(SAVED_MARK).await;
    Outcome::Saved
}

/// Per-message handler for `catch_up_each`.
pub struct PipHandler<'a> {
    pub allow: &'a Allowlist,
    pub cfg: &'a Config,
}

impl<M: Inbound> Handle<M> for PipHandler<'_> {
    fn handle(&self, m: &M) -> impl std::future::Future<Output = Outcome> + Send {
        async move { handle(self.allow, self.cfg, m).await }
    }
}

/// The recipe line with the overrides it names.
fn recipe_line(label: &str, overridden: &[String]) -> String {
    if overridden.is_empty() {
        label.to_string()
    } else {
        format!("{label}; typed {} overrode the recipe", overridden.join(", "))
    }
}

/// The meal card: kcal as the title, one line per item, the macros and veg as fields, and what was not counted.
fn meal_card(an: &Analysis, today: Option<&Totals>, kcal_target: u32) -> Rich {
    let p = &an.parsed;
    let m = nutrition::macros(&p.items);
    let veg = nutrition::has_veg(&p.items);
    let mut fields = vec![
        ("Protein".to_string(), format!("{} g", trim_num(m.protein)), true),
        ("Carbs".to_string(), format!("{} g", trim_num(m.carbs)), true),
        ("Fat".to_string(), format!("{} g", trim_num(m.fat)), true),
        ("Veg".to_string(), if veg { "yes" } else { "no" }.to_string(), true),
    ];
    if let Some(label) = &an.recipe {
        fields.push(("Recipe".to_string(), recipe_line(label, &an.overridden), false));
    }
    if !an.assumed.is_empty() {
        fields.push(("Assumed".to_string(), an.assumed.join("; "), false));
    }
    if !p.unknown.is_empty() {
        let unknown: Vec<String> = p.unknown.iter().map(|u| format!("{} ({})", u.text, u.hint)).collect();
        fields.push(("Not counted".to_string(), unknown.join("; "), false));
    }
    if !an.skipped.is_empty() {
        fields.push(("Recipe lines skipped".to_string(), an.skipped.join("; "), false));
    }
    if let Some(t) = today {
        let value = format!("{} / {kcal_target} kcal · {} meal(s)", t.macros.kcal.round() as i64, t.meals);
        fields.push(("Today".to_string(), value, false));
    }
    Rich {
        text: reply_text(p, an.recipe.as_deref(), &an.overridden, &an.skipped, &an.assumed, today),
        title: format!("{} kcal", m.kcal.round() as i64),
        description: p.items.iter().map(nutrition::item_line).collect::<Vec<_>>().join("\n"),
        fields,
        color: if veg { 0x9ED9A0 } else { 0xF7B6A3 },
        footer: "today · week · undo · fix <meal> · help".to_string(),
        image: None,
    }
}

/// The plain-text reply (also the card's fallback): totals, the items, the recipe, what was assumed, what was not counted, and today so far.
pub fn reply_text(
    parsed: &Parsed,
    recipe: Option<&str>,
    overridden: &[String],
    skipped: &[String],
    assumed: &[String],
    today: Option<&Totals>,
) -> String {
    let m = nutrition::macros(&parsed.items);
    let veg = if nutrition::has_veg(&parsed.items) { "✓" } else { "✗" };
    let mut out = vec![format!(
        "{} kcal · P {} g · C {} g · F {} g · veg {veg}",
        m.kcal.round() as i64,
        trim_num(m.protein),
        trim_num(m.carbs),
        trim_num(m.fat)
    )];
    if !parsed.items.is_empty() {
        let items: Vec<String> = parsed
            .items
            .iter()
            .map(|i| {
                let kcal = i.grams / 100.0 * i.food.kcal;
                let default = if i.assumed { format!(" (default {} g)", trim_num(i.grams)) } else { String::new() };
                format!("{} g {} {}{default}", trim_num(i.grams), i.food.name, kcal.round() as i64)
            })
            .collect();
        out.push(items.join(" · "));
    }
    if let Some(label) = recipe {
        out.push(format!("Recipe: {}", recipe_line(label, overridden)));
    }
    if !assumed.is_empty() {
        out.push(format!("Assumed: {}", assumed.join("; ")));
    }
    if !parsed.unknown.is_empty() {
        let unknown: Vec<String> = parsed.unknown.iter().map(|u| format!("{} ({})", u.text, u.hint)).collect();
        out.push(format!("Unknown, not counted: {}", unknown.join("; ")));
    }
    if !skipped.is_empty() {
        out.push(format!("Recipe lines skipped: {}", skipped.join("; ")));
    }
    if let Some(t) = today {
        out.push(format!("Today: {} kcal over {} meal(s)", t.macros.kcal.round() as i64, t.meals));
    }
    out.join("\n")
}

/// Whole numbers without a decimal, otherwise one decimal place.
fn trim_num(x: f64) -> String {
    if (x - x.round()).abs() < 0.05 {
        format!("{:.0}", x.round())
    } else {
        format!("{x:.1}")
    }
}
