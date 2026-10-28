// stats.rs — local dictation stats (rusqlite).
//
// One row per dictation session. The panel reads today's word count and the
// day streak from `dashboard()`, which also derives WPM, totals,
// per-category usage and monthly growth from the same rows.

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CATEGORIES: [(&str, &str); 6] = [
    ("ai", "AI prompts"),
    ("work", "Work messages"),
    ("personal", "Personal messages"),
    ("docs", "Documents"),
    ("email", "Emails"),
    ("other", "Other tasks"),
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CategoryUsage {
    pub key: String,
    pub label: String,
    pub count: i64,
    pub pct: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DayActivity {
    pub date: String,
    pub words: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub wpm: i64,
    pub percentile: String,
    pub total_fixes: i64,
    pub words_corrected: i64,
    pub dictionary_fixes: i64,
    pub total_words: i64,
    pub monthly_growth_pct: f64,
    pub desktop_words: i64,
    pub mobile_words: i64,
    pub total_apps_used: i64,
    pub categories: Vec<CategoryUsage>,
    pub current_streak: i64,
    pub longest_streak: i64,
    pub days: Vec<DayActivity>,
}

pub struct StatsDb {
    conn: Connection,
}

impl StatsDb {
    /// Open the stats DB, surviving deletion and corruption: a file that
    /// fails to open or fails `PRAGMA quick_check` is quarantined
    /// (renamed *.corrupt-<ts>) and a fresh schema is created in its place.
    pub fn open(path: &Path) -> Result<Self, String> {
        match Self::open_impl(path) {
            Ok(db) => Ok(db),
            Err(first) => {
                log::warn!("stats db unusable ({first}) — quarantining and recreating");
                Self::quarantine(path);
                Self::open_impl(path)
                    .map_err(|e| format!("recreate stats db after corruption: {e}"))
            }
        }
    }

    fn quarantine(path: &Path) {
        let ts = Local::now().format("%Y%m%d-%H%M%S");
        let backup = path.with_extension(format!("sqlite3.corrupt-{ts}"));
        if path.exists() {
            if let Err(e) = std::fs::rename(path, &backup) {
                log::error!("quarantine stats db: {e}; deleting instead");
                let _ = std::fs::remove_file(path);
            } else {
                log::warn!("corrupt stats db moved to {}", backup.display());
            }
        }
        // stale WAL/SHM would resurrect the corruption on reopen
        for suffix in ["-wal", "-shm"] {
            let mut os = path.as_os_str().to_owned();
            os.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(os));
        }
    }

    fn open_impl(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("open stats db: {e}"))?;
        let check: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|e| format!("quick_check: {e}"))?;
        if check != "ok" {
            return Err(format!("integrity check failed: {check}"));
        }
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS sessions (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 ts          INTEGER NOT NULL,
                 date        TEXT    NOT NULL,
                 app         TEXT    NOT NULL,
                 category    TEXT    NOT NULL,
                 device      TEXT    NOT NULL DEFAULT 'desktop',
                 items       INTEGER NOT NULL DEFAULT 1,
                 words       INTEGER NOT NULL,
                 duration_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_date ON sessions(date);
             CREATE INDEX IF NOT EXISTS idx_sessions_cat  ON sessions(category);
             CREATE TABLE IF NOT EXISTS counters (
                 key   TEXT PRIMARY KEY,
                 value INTEGER NOT NULL
             );",
        )
        .map_err(|e| format!("init stats schema: {e}"))?;
        Ok(Self { conn })
    }

    /// Record one real dictation session and bump the auto-fix counters.
    pub fn record_session(
        &self,
        app: &str,
        category: &str,
        words: i64,
        duration_ms: i64,
        words_corrected: i64,
    ) -> Result<(), String> {
        let now = Local::now();
        self.conn
            .execute(
                "INSERT INTO sessions (ts, date, app, category, device, items, words, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, 'desktop', 1, ?5, ?6)",
                params![
                    now.timestamp(),
                    now.format("%Y-%m-%d").to_string(),
                    app,
                    category,
                    words,
                    duration_ms
                ],
            )
            .map_err(|e| format!("insert session: {e}"))?;
        if words_corrected > 0 {
            self.bump_counter("words_corrected", words_corrected)?;
        }
        Ok(())
    }

    fn bump_counter(&self, key: &str, by: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO counters (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = value + ?2",
                params![key, by],
            )
            .map_err(|e| format!("bump counter {key}: {e}"))?;
        Ok(())
    }

    fn counter(&self, key: &str) -> i64 {
        self.conn
            .query_row("SELECT value FROM counters WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .unwrap_or(0)
    }

    fn scalar(&self, sql: &str) -> i64 {
        self.conn
            .query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    pub fn dashboard(&self) -> Result<DashboardStats, String> {
        let total_words = self.scalar("SELECT SUM(words) FROM sessions");
        let desktop_words = self.scalar("SELECT SUM(words) FROM sessions WHERE device = 'desktop'");
        let mobile_words = self.scalar("SELECT SUM(words) FROM sessions WHERE device = 'mobile'");
        let total_apps_used =
            self.scalar("SELECT COUNT(DISTINCT app) FROM sessions WHERE device = 'desktop'");

        // WPM over the most recent sessions that carry a real duration
        let (w, ms): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(words),0), COALESCE(SUM(duration_ms),0) FROM (
                     SELECT words, duration_ms FROM sessions
                     WHERE duration_ms > 0 ORDER BY ts DESC LIMIT 200)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("wpm query: {e}"))?;
        let wpm = if ms > 0 {
            ((w as f64) / (ms as f64 / 60_000.0)).round() as i64
        } else {
            0
        };

        let words_corrected = self.counter("words_corrected");
        let dictionary_fixes = self.counter("dictionary_fixes");

        // growth: words dictated this calendar month vs everything before it
        let today = Local::now().date_naive();
        let month_start = today.with_day(1).unwrap_or(today);
        let month_words = self.scalar(&format!(
            "SELECT SUM(words) FROM sessions WHERE date >= '{}'",
            month_start.format("%Y-%m-%d")
        ));
        let before = total_words - month_words;
        let monthly_growth_pct = if before > 0 {
            ((month_words as f64 / before as f64) * 1000.0).round() / 10.0
        } else {
            0.0
        };

        // per-category desktop usage
        let mut cat_words: HashMap<String, i64> = HashMap::new();
        let mut cat_items: HashMap<String, i64> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT category, SUM(words), SUM(items) FROM sessions
                     WHERE device = 'desktop' GROUP BY category",
                )
                .map_err(|e| format!("cat query: {e}"))?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|e| format!("cat rows: {e}"))?;
            for row in rows.flatten() {
                cat_words.insert(row.0.clone(), row.1);
                cat_items.insert(row.0, row.2);
            }
        }
        let mut categories: Vec<CategoryUsage> = CATEGORIES
            .iter()
            .map(|(key, label)| {
                let words = *cat_words.get(*key).unwrap_or(&0);
                let pct = if desktop_words > 0 {
                    ((words as f64 / desktop_words as f64) * 100.0).round() as i64
                } else {
                    0
                };
                CategoryUsage {
                    key: (*key).into(),
                    label: (*label).into(),
                    count: *cat_items.get(*key).unwrap_or(&0),
                    pct,
                }
            })
            .collect();
        // biggest share first — the UI's color ramp encodes rank
        categories.sort_by(|a, b| b.pct.cmp(&a.pct).then(b.count.cmp(&a.count)));

        // daily activity + streaks
        let mut daily: HashMap<NaiveDate, i64> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT date, SUM(words) FROM sessions GROUP BY date")
                .map_err(|e| format!("daily query: {e}"))?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| format!("daily rows: {e}"))?;
            for row in rows.flatten() {
                if let Ok(d) = NaiveDate::parse_from_str(&row.0, "%Y-%m-%d") {
                    daily.insert(d, row.1);
                }
            }
        }
        let (current_streak, longest_streak) = streaks(&daily, today);

        // last 18 heatmap columns worth of days (zero-filled)
        let mut days = Vec::with_capacity(126);
        for i in (0..126).rev() {
            let d = today - ChronoDuration::days(i);
            days.push(DayActivity {
                date: d.format("%Y-%m-%d").to_string(),
                words: *daily.get(&d).unwrap_or(&0),
            });
        }

        Ok(DashboardStats {
            wpm,
            percentile: percentile_for(wpm),
            total_fixes: words_corrected + dictionary_fixes,
            words_corrected,
            dictionary_fixes,
            total_words,
            monthly_growth_pct,
            desktop_words,
            mobile_words,
            total_apps_used,
            categories,
            current_streak,
            longest_streak,
            days,
        })
    }
}

fn percentile_for(wpm: i64) -> String {
    let p = match wpm {
        w if w >= 160 => "Top 0.1%",
        w if w >= 140 => "Top 0.5%",
        w if w >= 120 => "Top 1%",
        w if w >= 100 => "Top 5%",
        w if w >= 80 => "Top 10%",
        w if w >= 60 => "Top 25%",
        _ => "Top 50%",
    };
    p.into()
}

/// (current, longest) run of consecutive active days. The current streak is
/// counted from today, or from yesterday if today has no dictation yet.
fn streaks(daily: &HashMap<NaiveDate, i64>, today: NaiveDate) -> (i64, i64) {
    let active = |d: &NaiveDate| daily.get(d).map(|w| *w > 0).unwrap_or(false);

    let mut current = 0i64;
    let mut d = if active(&today) {
        today
    } else {
        today - ChronoDuration::days(1)
    };
    while active(&d) {
        current += 1;
        d -= ChronoDuration::days(1);
    }

    let mut dates: Vec<&NaiveDate> = daily
        .iter()
        .filter(|(_, w)| **w > 0)
        .map(|(d, _)| d)
        .collect();
    dates.sort();
    let mut longest = 0i64;
    let mut run = 0i64;
    let mut prev: Option<&NaiveDate> = None;
    for date in dates {
        run = match prev {
            Some(p) if *date - *p == ChronoDuration::days(1) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        prev = Some(date);
    }
    (current, longest)
}
