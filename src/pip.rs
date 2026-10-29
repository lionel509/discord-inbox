//! Pip's handler (U6): one DM in, one day-note block out, one card back. Commands (`today`, `week`, `undo`,
//! `fix`, `help`) are read before any meal parsing and sit behind the same allowlist as meals. A message
//! that is not food is a note, never a meal, so the 21:00 check-in still fires.
//! Nutrition comes from the meal reader when `PIP_MODEL` is set (`pip_model`), else from the local table only
//! (`nutrition`); recipes are read, never written (`recipes`).
use crate::allowlist::Allowlist;
use crate::daylog::{self, Meal, NoteMsg, Totals, Written};
use crate::ingest::{typing_while, Handle, Inbound, Outcome, FAILED_MARK, SAVED_MARK};
use crate::foods::{self, SavedFood};
use crate::nutrition::{self, Parsed};
use crate::pip_model::{self, Label, Reading};
use crate::recipes::{self, Recipe, Scaled};
use crate::rich::Rich;
use crate::summary;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Pip's public presence: persona only, nothing about food or health.
pub const PRESENCE: &str = "curled up on a warm laptop 🐈";
/// Used when `PIP_KCAL_TARGET` is unset or unparsable. A placeholder until he sets his own.
pub const DEFAULT_KCAL_TARGET: u32 = 2400;
/// Used when `PIP_MODEL_TIMEOUT` is unset: seconds the meal reader gets before it counts as down.
pub const DEFAULT_MODEL_TIMEOUT_SECS: u64 = 90;

const DOWN_LINE: &str = "reader down: photos not read, counted from the table only";
const DOWN_FIELD: &str = "down; photos not read, counted from the table only";
const DOWN_REPLY: &str = "Pip's reader is down and nothing in that matched the food table, so nothing was logged. Send it again in a bit, or type it as items with weights (cooked rice 180g) to log it now.";
const UNREADABLE_HEAD: &str = "I couldn't read that as food: ";
const UNREADABLE_TAIL: &str = ". Nothing logged; send it again with names and weights (cooked rice 180g).";
const PHOTO_UNREAD_REPLY: &str = "No food I could read in the photo(s), so nothing was logged. Reply with what was in it (salmon 100g, cooked rice 180g).";

pub const HELP: &str = "Send what you ate, one item per line: salmon 100g, cooked rice 180g, cabbage 85g, or a recipe name (add a count like 2 x, or a weight).\nCommands: today (the day's card), week (the last seven days), undo (drop the last meal), fix <meal> (replace the last meal), help.\nA message with no food in it is saved as a note, not a meal.\nSend a package's nutrition label as a photo to save that food; then 24 wontons (or wontons for your usual count) logs it from the label.";

const FIX_USAGE: &str = "Send fix with the replacement meal, e.g. fix salmon 120g. It replaces the last meal logged today.";

#[derive(Clone, Debug)]
pub struct Config {
    pub nutrition_dir: PathBuf,
    pub recipes_dir: Option<PathBuf>,
    /// Saved foods (`PIP_FOODS_DIR`, absolute, #146): one note per packaged food, written from a label photo.
    pub foods_dir: Option<PathBuf>,
    pub low_kcal: u32,
    pub kcal_target: u32,
    /// The meal reader helper (`PIP_MODEL`, absolute path). `None` keeps the table-only behaviour.
    pub model: Option<PathBuf>,
    /// How long the reader gets before a meal counts as down (`PIP_MODEL_TIMEOUT`, seconds).
    pub model_timeout: Duration,
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
        let foods_dir = match std::env::var("PIP_FOODS_DIR") {
            Ok(r) if !r.trim().is_empty() => {
                let p = PathBuf::from(&r);
                if !p.is_absolute() {
                    return Err(format!("PIP_FOODS_DIR must be an absolute path, got {r:?}"));
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
        let model = match std::env::var("PIP_MODEL") {
            Ok(v) if !v.trim().is_empty() => {
                let p = PathBuf::from(v.trim());
                if !p.is_absolute() {
                    return Err(format!("PIP_MODEL must be an absolute path, got {v:?}"));
                }
                Some(p)
            }
            _ => None,
        };
        let model_timeout = match std::env::var("PIP_MODEL_TIMEOUT") {
            Ok(v) if !v.trim().is_empty() => match v.trim().parse::<u64>() {
                Ok(s) if s > 0 => Duration::from_secs(s),
                _ => return Err(format!("PIP_MODEL_TIMEOUT must be whole seconds, got {v:?}")),
            },
            _ => Duration::from_secs(DEFAULT_MODEL_TIMEOUT_SECS),
        };
        Ok(Config {
            nutrition_dir,
            recipes_dir,
            foods_dir,
            low_kcal: daylog::low_kcal_threshold(),
            kcal_target,
            model,
            model_timeout,
        })
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
    /// The reader was wired and gave nothing usable, so the table answered for the typed text.
    down: bool,
}

/// The typed text's items: the reader's when it answers, else the table's. `true` when the reader was down.
fn typed_of(s: &str, counted: &[String], read: Option<&dyn Fn(&str, &[String]) -> Option<Parsed>>) -> (Parsed, bool) {
    match read {
        None => (nutrition::parse_meal(s), false),
        Some(f) => match f(s, counted) {
            Some(p) => (p, false),
            None => (nutrition::parse_meal(s), true),
        },
    }
}

/// The message with its first recipe-name match taken out (ignoring ASCII case). `None` when the name is not in it.
fn without_name(text: &str, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let start = text.to_ascii_lowercase().find(&name.to_ascii_lowercase())?;
    let end = start + name.len();
    Some(format!("{}{}", &text[..start], &text[end..]))
}

/// Saved foods first (#146): a typed count of a saved food (`24 wontons`) becomes items from its label, and the rest goes
/// through `analyze_rest` as before. A refused count still makes the message a meal, with the refusal under Not counted.
fn analyze(text: &str, recipes: &[Recipe], foods: &[SavedFood], read: Option<&dyn Fn(&str, &[String]) -> Option<Parsed>>) -> Analysis {
    let t = foods::take(text, foods, true);
    let counted: Vec<String> = t.items.iter().map(|i| format!("{} {}", i.food.name, nutrition::label_tail(i).unwrap_or_default().trim())).collect();
    let mut an = analyze_rest(&t.rest, &counted, recipes, foods, read);
    if t.items.is_empty() && t.unknown.is_empty() {
        return an;
    }
    // The reader was told what is counted; an estimate named exactly like a counted food is that food, shown under Not
    // counted. Anything named otherwise is kept.
    let counted_foods: Vec<&SavedFood> = foods.iter().filter(|f| t.items.iter().any(|i| i.food.name == f.name)).collect();
    let mut dropped = Vec::new();
    an.parsed.items.retain(|i| {
        let drop = i.estimated && counted_foods.iter().any(|f| foods::names_food(i.food.name, f));
        if drop {
            dropped.push(nutrition::Unknown { text: format!("photo estimate {}", i.food.name), hint: "already counted from its label".to_string() });
        }
        !drop
    });
    an.parsed.unknown.extend(dropped);
    an.parsed.items.splice(0..0, t.items);
    an.parsed.unknown.splice(0..0, t.unknown);
    an.is_meal = true;
    an
}

/// Recipe first (a name match scales it), then the typed lines, then the merge where typed rows win.
/// With a reader wired (`read`), a meal needs at least one item; without one, today's rule applies.
fn analyze_rest(text: &str, counted: &[String], recipes: &[Recipe], foods: &[SavedFood], read: Option<&dyn Fn(&str, &[String]) -> Option<Parsed>>) -> Analysis {
    let hit = match recipes::resolve_or_refuse(text, recipes) {
        Ok(hit) => hit,
        Err(r) => {
            // A recipe line the typed text cannot use: the meal is still logged, with the refusal as its unknown.
            let (mut typed, down) = typed_of(&r.rest, counted, read);
            typed.unknown.insert(0, r.unknown);
            let is_meal = read.is_none() || !typed.items.is_empty();
            return Analysis { parsed: typed, overridden: Vec::new(), recipe: None, skipped: Vec::new(), assumed: Vec::new(), is_meal, down };
        }
    };
    let typed_text = hit.as_ref().map(|h| h.rest.clone()).unwrap_or_else(|| text.to_string());
    // With a reader, it sees the whole message minus only the recipe name, so typed weights beside the name survive.
    let model_text = match (&hit, read) {
        (Some(h), Some(_)) => without_name(text, &h.matched).unwrap_or_else(|| typed_text.clone()),
        _ => typed_text.clone(),
    };
    let (typed, down) = typed_of(&model_text, counted, read);
    let Some(h) = hit else {
        let is_meal = if read.is_some() { !typed.items.is_empty() } else { !typed.items.is_empty() || nutrition::looks_like_food(&typed_text) };
        return Analysis { parsed: typed, overridden: Vec::new(), recipe: None, skipped: Vec::new(), assumed: Vec::new(), is_meal, down };
    };
    let name = h.recipe.name.as_str();
    let (servings, how) = recipes::scale(&h, &typed);
    let mut scaled = h.clone();
    scaled.servings = servings;
    let pieces = recipes::ingredient_pieces(&scaled).join(", ");
    // Saved foods first, so a recipe line that names one (`8 pork wontons`) is counted from its label (#146).
    let tr = foods::take(&pieces, foods, false);
    let mut recipe_parsed = nutrition::parse_meal(&tr.rest);
    recipe_parsed.items.extend(tr.items);
    recipe_parsed.unknown.extend(tr.unknown);
    let (parsed, overridden) = nutrition::merge(typed, recipe_parsed);

    let mut label = format!("{name} ×{}", trim_num(servings));
    let mut assumed = h.recipe.assumed.clone();
    match &how {
        Scaled::Count => {}
        Scaled::Weight { food, grams } | Scaled::Typed { food, grams } => {
            label.push_str(&format!(" (scaled to your {} g {food})", trim_num(*grams)));
        }
        Scaled::Assumed => {
            match recipes::reference(h.recipe) {
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
    let is_meal = read.is_none() || !parsed.items.is_empty();
    Analysis { parsed, overridden, recipe: Some(label), skipped: h.recipe.skipped.clone(), assumed, is_meal, down }
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
        // A crash after the save but before `paid` leaves a stale owed line; clear it here or it blocks undo all day.
        if let Err(e) = daylog::paid(&cfg.nutrition_dir, msg_id) {
            eprintln!("pip: clearing owed line for {msg_id} failed: {e}");
        }
        return Outcome::Skipped;
    }
    if let Err(e) = std::fs::create_dir_all(&cfg.nutrition_dir) {
        eprintln!("pip: nutrition dir {} not creatable: {e}", cfg.nutrition_dir.display());
        return Outcome::Failed;
    }
    // Commands other than `fix` are answered from the ledger and never owe a meal. `fix` is the one command
    // that also takes a meal (and a photo).
    let fix_rest = match command(&text).filter(|c| matches!(c, Command::Fix(_)) || files.is_empty()) {
        Some(Command::Fix(rest)) => Some(rest),
        Some(other) => {
            if m.live() {
                m.typing().await;
            }
            return run_command(cfg, m, &day, msg_id, &other).await;
        }
        None => None,
    };

    // Owed before typing and any download, so a crash or a failed save leaves the meal for the restart catch-up.
    if let Err(e) = daylog::owe(&cfg.nutrition_dir, msg_id, &day, &time) {
        eprintln!("pip: owed line for {msg_id} failed: {e}");
    }
    if m.live() {
        m.typing().await;
    }
    let out = log_message(cfg, m, &day, msg_id, time, hhmm, text, files, fix_rest).await;
    if out != Outcome::Failed {
        if let Err(e) = daylog::paid(&cfg.nutrition_dir, msg_id) {
            eprintln!("pip: clearing owed line for {msg_id} failed: {e}");
        }
    }
    out
}

/// The logging half of `handle`: analysis, photo saves, the note or meal write, and the reply. A meal whose
/// write or photo fails returns `Failed` and so stays owed.
#[allow(clippy::too_many_arguments)]
async fn log_message<M: Inbound>(
    cfg: &Config,
    m: &M,
    day: &str,
    msg_id: u64,
    time: String,
    hhmm: String,
    text: String,
    files: Vec<String>,
    fix_rest: Option<String>,
) -> Outcome {
    let recipe_list = cfg.recipes_dir.as_deref().map(recipes::load).unwrap_or_default();
    let food_list = cfg.foods_dir.as_deref().map(foods::load).unwrap_or_default();
    let subject = fix_rest.clone().unwrap_or_else(|| text.clone());
    if fix_rest.is_some() && subject.is_empty() {
        return fix_usage(cfg, m, msg_id).await;
    }
    let (mut an, paths) = match cfg.model.as_deref() {
        None => {
            let an = analyze(&subject, &recipe_list, &food_list, None);
            if fix_rest.is_some() && !an.is_meal {
                return fix_usage(cfg, m, msg_id).await;
            }
            if !an.is_meal {
                // Not a meal, so nothing stays owed for it: a failed photo download below must not block undo or fix.
                let _ = daylog::paid(&cfg.nutrition_dir, msg_id);
            }
            match save_photos(cfg, m, day, &hhmm, msg_id, &files).await {
                Ok(paths) => (an, paths),
                Err(out) => return out,
            }
        }
        Some(cmd) => {
            // The reader reads the saved photos, so they are saved before any analysis.
            let paths = match save_photos(cfg, m, day, &hhmm, msg_id, &files).await {
                Ok(paths) => paths,
                Err(out) => return out,
            };
            // The reader blocks for up to the model timeout, so the analysis runs off the async workers, with typing shown.
            let (cmd, timeout, reads) = (cmd.to_path_buf(), cfg.model_timeout, paths.clone());
            let (subject_run, recipes_run, foods_run) = (subject.clone(), recipe_list.clone(), food_list.clone());
            // A printed label is handed back through this slot: it is saved as a food, not counted as a meal (#146).
            let label_slot: Arc<Mutex<Option<Label>>> = Arc::new(Mutex::new(None));
            let slot = label_slot.clone();
            let work = tokio::task::spawn_blocking(move || {
                let read = |t: &str, counted: &[String]| -> Option<Parsed> {
                    // Nothing to read (a recipe name with no typed rest, and no photo): no model call.
                    if t.trim().is_empty() && reads.is_empty() {
                        return Some(Parsed::default());
                    }
                    match pip_model::read_counted(&cmd, t, &reads, counted, timeout) {
                        Reading::Read(p) => Some(p),
                        Reading::Label(l) => {
                            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
                            Some(Parsed::default())
                        }
                        Reading::Down => None,
                    }
                };
                let read_dyn: &dyn Fn(&str, &[String]) -> Option<Parsed> = &read;
                analyze(&subject_run, &recipes_run, &foods_run, Some(read_dyn))
            });
            let an = match typing_while(m, work).await {
                Ok(an) => an,
                Err(e) => {
                    eprintln!("pip: meal analysis failed for {msg_id}: {e}");
                    return Outcome::Failed;
                }
            };
            let label = label_slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(l) = label {
                return save_label(cfg, m, msg_id, day, &l).await;
            }
            if fix_rest.is_some() && !an.is_meal {
                return fix_usage(cfg, m, msg_id).await;
            }
            if !an.is_meal {
                let _ = daylog::paid(&cfg.nutrition_dir, msg_id);
            }
            (an, paths)
        }
    };
    let photos: Vec<String> = paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
    // Typed text only: a photo's estimate is the reader's guess, and the label note says so (#146).
    if cfg.foods_dir.is_some() && files.is_empty() {
        let est: Vec<&str> = an.parsed.items.iter().filter(|i| i.estimated).map(|i| i.food.name).collect();
        if !est.is_empty() {
            an.assumed.push(format!("Estimated: {}. A photo of its nutrition label makes it exact next time.", est.join(", ")));
        }
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
        if let Err(e) = m.reply(&note_reply(cfg, &an, &text)).await {
            eprintln!("pip: reply on {msg_id} failed: {e}");
        }
        let _ = m.react(SAVED_MARK).await;
        return Outcome::Saved;
    }

    if fix_rest.is_some() {
        // Claimed after the photos are saved, so a failed download leaves the fix retryable.
        if let Some(out) = claim(cfg, m, msg_id).await {
            return out;
        }
        // Refused, not deferred: the fix is claimed, so a re-sent copy is skipped and only a new send can act.
        if let Some((id, t)) = daylog::owed_before(&cfg.nutrition_dir, day, msg_id) {
            let _ = m.reply(&format!("Your {t} meal (msg {id}) hasn't saved yet, so I didn't fix anything. Send the fix again once it's logged.")).await;
            let _ = m.react(SAVED_MARK).await;
            return Outcome::Saved;
        }
        match daylog::undo_before(&cfg.nutrition_dir, day, cfg.low_kcal, msg_id, msg_id) {
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
    let mut lines: Vec<String> = parsed.items.iter().map(nutrition::item_line).collect();
    if an.down {
        lines.push(DOWN_LINE.to_string());
    }
    let meal = Meal {
        msg_id,
        time,
        lines,
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

    // A count of a saved food is the new default for a bare name (`wontons` next time means this count) (#146).
    if let Some(dir) = cfg.foods_dir.as_deref() {
        for i in an.parsed.items.iter().filter(|i| i.food.source == "label" && !i.assumed) {
            if let Err(e) = foods::remember(dir, i.food.name, i.qty, i.unit) {
                eprintln!("pip: food note for {} failed: {e}", i.food.name);
            }
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

/// A printed label (#146): saved as a food note, never logged as a meal. The reply says what was saved and what the
/// label does not print. The ledger line is written after the note, so a replay saves nothing twice.
async fn save_label<M: Inbound>(cfg: &Config, m: &M, msg_id: u64, day: &str, l: &Label) -> Outcome {
    let text = match cfg.foods_dir.as_deref() {
        None => "That's a nutrition label. Set PIP_FOODS_DIR and I'll save it; nothing was logged.".to_string(),
        Some(dir) => match foods::from_label(l, day) {
            Err(why) => format!("Nothing saved: {why}."),
            Ok(food) => match foods::upsert(dir, &food) {
                Ok(_) => label_reply(&food),
                Err(e) => {
                    eprintln!("pip: saved food {} failed: {e}", food.name);
                    let _ = m.react(FAILED_MARK).await;
                    return Outcome::Failed;
                }
            },
        },
    };
    if let Err(e) = daylog::record_command(&cfg.nutrition_dir, msg_id) {
        eprintln!("pip: command ledger on {msg_id} failed: {e}");
    }
    if let Err(e) = m.reply(&text).await {
        eprintln!("pip: reply on {msg_id} failed: {e}");
    }
    let _ = m.react(SAVED_MARK).await;
    Outcome::Saved
}

/// The saved food's reply: its label values, what it lacks, and the two ways to count it.
fn label_reply(f: &SavedFood) -> String {
    let serving = match (f.serving_g, f.serving_pc) {
        (Some(g), Some(pc)) => format!("{} g ({} pc)", trim_num(g), trim_num(pc)),
        (Some(g), None) => format!("{} g", trim_num(g)),
        _ => "? serving".to_string(),
    };
    let v = |x: Option<f64>| x.map(trim_num).unwrap_or_else(|| "?".to_string());
    let mut text = format!(
        "Saved {} to your foods: {serving} per serving · {} kcal · P {} g · C {} g · F {} g.",
        f.name,
        v(f.kcal),
        v(f.protein),
        v(f.carbs),
        v(f.fat)
    );
    let miss = foods::missing(f);
    if !miss.is_empty() {
        text.push_str(&format!(
            " Not on the label: {}; those count as 0 until you fill them in on the note or send a clearer photo.",
            miss.join(", ")
        ));
    }
    match f.serving_pc {
        Some(pc) => text.push_str(&format!(" Log it as \"{} {}\", or just \"{}\" for your usual count.", trim_num(pc * 3.0), f.name, f.name)),
        None => text.push_str(&format!(" Log it as \"2 servings {}\", or just \"{}\" for your usual count.", f.name, f.name)),
    }
    text
}

/// The `fix` usage reply, claimed in the ledger first so a re-sent copy is skipped.
async fn fix_usage<M: Inbound>(cfg: &Config, m: &M, msg_id: u64) -> Outcome {
    if let Some(out) = claim(cfg, m, msg_id).await {
        return out;
    }
    let _ = m.reply(FIX_USAGE).await;
    let _ = m.react(SAVED_MARK).await;
    Outcome::Saved
}

/// Downloads and saves every photo of the message, in order. A failed save marks the message and returns `Failed`.
async fn save_photos<M: Inbound>(cfg: &Config, m: &M, day: &str, hhmm: &str, msg_id: u64, files: &[String]) -> Result<Vec<PathBuf>, Outcome> {
    let mut paths = Vec::new();
    for (i, name) in files.iter().enumerate() {
        let path = daylog::photo_path(&cfg.nutrition_dir, day, hhmm, msg_id, i, name);
        let saved = match m.download(i).await {
            Ok(bytes) => daylog::save_photo(&path, &bytes).map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        if let Err(e) = saved {
            eprintln!("pip: photo {msg_id}/{i} failed: {e}");
            let _ = m.react(FAILED_MARK).await;
            return Err(Outcome::Failed);
        }
        paths.push(path);
    }
    Ok(paths)
}

/// The reply for a message logged as a note (no meal). Without a reader it is today's wording; with one, it says
/// what was not read, so he knows what to send again.
fn note_reply(cfg: &Config, an: &Analysis, text: &str) -> String {
    if cfg.model.is_none() {
        return if text.is_empty() {
            "Photo saved. I don't guess from photos; reply with what was in it (e.g. salmon 100g, cooked rice 180g).".to_string()
        } else {
            format!("No food in that, so it isn't logged as a meal: \"{text}\". Send items with weights (cooked rice 180g) or a recipe name; help lists commands.")
        };
    }
    if an.down {
        return DOWN_REPLY.to_string();
    }
    if !an.parsed.unknown.is_empty() {
        let unread: Vec<String> = an.parsed.unknown.iter().map(|u| u.text.clone()).collect();
        return format!("{UNREADABLE_HEAD}{}{UNREADABLE_TAIL}", unread.join("; "));
    }
    if text.is_empty() {
        PHOTO_UNREAD_REPLY.to_string()
    } else {
        format!("No food in that, so it isn't logged as a meal: \"{text}\". Send items with weights (cooked rice 180g) or a recipe name; help lists commands.")
    }
}

/// Claims a command id in the ledger before its action runs. `Some(outcome)` means stop: the id was already
/// claimed by another delivery (`Skipped`), or the ledger write failed (`Failed`, marked).
async fn claim<M: Inbound>(cfg: &Config, m: &M, msg_id: u64) -> Option<Outcome> {
    match daylog::claim_command(&cfg.nutrition_dir, msg_id) {
        Ok(true) => None,
        Ok(false) => Some(Outcome::Skipped),
        Err(e) => {
            eprintln!("pip: command ledger on {msg_id} failed: {e}");
            let _ = m.react(FAILED_MARK).await;
            Some(Outcome::Failed)
        }
    }
}

/// Answers a command (never `fix`, which `handle` runs itself). The ledger is claimed before the action.
async fn run_command<M: Inbound>(cfg: &Config, m: &M, day: &str, msg_id: u64, cmd: &Command) -> Outcome {
    let dir = cfg.nutrition_dir.as_path();
    if let Some(out) = claim(cfg, m, msg_id).await {
        return out;
    }
    let reply = match cmd {
        Command::Help => m.reply(HELP).await,
        Command::Today => m.reply_rich(&summary::day_card(dir, day, cfg.low_kcal, cfg.kcal_target)).await,
        Command::Week => m.reply_rich(&summary::week_card(dir, day, cfg.low_kcal, cfg.kcal_target)).await,
        // Refused while an earlier meal from today is owed: that meal could be the one the undo means.
        Command::Undo => match daylog::owed_before(dir, day, msg_id) {
            Some((id, t)) => {
                m.reply(&format!("Your {t} meal (msg {id}) hasn't saved yet, so I didn't undo anything. Send undo again once it's logged."))
                    .await
            }
            None => match daylog::undo_before(dir, day, cfg.low_kcal, msg_id, msg_id) {
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
    let mut text = reply_text(p, an.recipe.as_deref(), &an.overridden, &an.skipped, &an.assumed, today);
    if an.down {
        fields.push(("Reader".to_string(), DOWN_FIELD.to_string(), false));
        text.push_str(&format!("\nReader: {DOWN_FIELD}"));
    }
    Rich {
        text,
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
                let default = nutrition::label_tail(i).unwrap_or_else(|| if i.assumed { format!(" (default {} g)", trim_num(i.grams)) } else { String::new() });
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
