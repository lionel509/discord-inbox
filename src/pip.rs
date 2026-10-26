//! Pip's handler (U4): one DM in, one day-note block out, one short reply back.
//! Nutrition comes from the local table only (`nutrition`); recipes are read, never written (`recipes`).
use crate::allowlist::Allowlist;
use crate::daylog::{self, Meal, Totals, Written};
use crate::ingest::{Handle, Inbound, Outcome, FAILED_MARK, SAVED_MARK};
use crate::nutrition::{self, Parsed};
use crate::recipes;
use std::path::PathBuf;

/// Pip's public presence: persona only, nothing about food or health.
pub const PRESENCE: &str = "curled up on a warm laptop 🐈";

#[derive(Clone, Debug)]
pub struct Config {
    pub nutrition_dir: PathBuf,
    pub recipes_dir: Option<PathBuf>,
    pub low_kcal: u32,
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
        Ok(Config { nutrition_dir, recipes_dir, low_kcal: daylog::low_kcal_threshold() })
    }
}

/// One DM: log it (photos saved beside the note), then reply with what was counted.
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

    // Already in the day note (a replay, or a DM whose reply failed): nothing to do, and no download.
    if daylog::logged(&cfg.nutrition_dir, &day, msg_id) {
        return Outcome::Skipped;
    }
    if let Err(e) = std::fs::create_dir_all(&cfg.nutrition_dir) {
        eprintln!("pip: nutrition dir {} not creatable: {e}", cfg.nutrition_dir.display());
        return Outcome::Failed;
    }
    if m.live() {
        m.typing().await;
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

    // Recipes are re-read on every message: he edits them day to day.
    let recipe_list = cfg.recipes_dir.as_deref().map(recipes::load).unwrap_or_default();
    let hit = recipes::resolve(&text, &recipe_list);
    let typed_text = match &hit {
        Some(h) => h.rest.clone(),
        None => text.clone(),
    };
    let typed = nutrition::parse_meal(&typed_text);
    let (parsed, overridden, recipe, skipped, servings) = match &hit {
        Some(h) => {
            let pieces = recipes::ingredient_pieces(h).join(", ");
            let (merged, over) = nutrition::merge(typed, nutrition::parse_meal(&pieces));
            let label = format!("{} ×{}", h.recipe.name, trim_num(h.servings));
            (merged, over, Some(label), h.recipe.skipped.clone(), h.servings)
        }
        None => (typed, Vec::new(), None, Vec::new(), 1.0),
    };

    let note = if parsed.items.is_empty() && parsed.unknown.is_empty() && !text.is_empty() {
        Some(text.clone())
    } else {
        None
    };
    let recipe = recipe.map(|label| {
        if overridden.is_empty() {
            label
        } else {
            format!("{label} (typed {} overrode the recipe)", overridden.join(", "))
        }
    });
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
    let recipe_arg = hit.as_ref().map(|h| (h.recipe.name.as_str(), servings));
    let reply = reply_text(&parsed, recipe_arg, &overridden, &skipped, today.as_ref());
    if let Err(e) = m.reply(&reply).await {
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

/// The reply: totals, the items with their numbers, what was not counted, and today so far.
pub fn reply_text(
    parsed: &Parsed,
    recipe: Option<(&str, f64)>,
    overridden: &[String],
    skipped: &[String],
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
    if let Some((name, servings)) = recipe {
        let mut line = format!("Recipe: {name} ×{}", trim_num(servings));
        if !overridden.is_empty() {
            line.push_str(&format!("; typed {} overrode the recipe", overridden.join(", ")));
        }
        out.push(line);
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
