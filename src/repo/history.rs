use std::collections::HashMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Result, paths};

/// Visit is how often and how recently one repository was entered.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Visit {
    pub count: i64,
    pub last: i64, // unix seconds
}

/// now is the current time in unix seconds.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Visit {
    /// score weighs frequency by recency, the way z and zoxide do: a
    /// repository visited twice this hour outranks one visited ten times last
    /// month.
    pub fn score(&self, now: i64) -> f64 {
        let age = now - self.last;
        let count = self.count as f64;
        match age {
            a if a < 3600 => count * 4.0,
            a if a < 24 * 3600 => count * 2.0,
            a if a < 7 * 24 * 3600 => count * 0.5,
            _ => count * 0.25,
        }
    }
}

/// History is the visit log. It holds its own file path, so a test can point
/// it at a temporary directory instead of the user's state directory.
#[derive(Debug, Clone, Default)]
pub struct History {
    pub path: String,
    visits: HashMap<String, Visit>,
}

/// history_file is where the log lives:
///
///   $XDG_STATE_HOME/gm/frecency.json, else ~/.local/state/gm/frecency.json
pub fn history_file() -> Result<String> {
    let dir = match std::env::var("XDG_STATE_HOME") {
        Ok(d) if !d.is_empty() => d,
        _ => paths::join(&paths::home()?, ".local/state"),
    };
    Ok(paths::join(&dir, "gm/frecency.json"))
}

impl History {
    /// open reads the log at an explicit path. A missing, unreadable or
    /// corrupt log is an empty one: losing history must never block
    /// navigation.
    pub fn open(path: &str) -> History {
        let mut h = History {
            path: path.to_string(),
            visits: HashMap::new(),
        };
        let Ok(b) = std::fs::read(path) else { return h };
        let Ok(serde_json::Value::Object(m)) = serde_json::from_slice(&b) else {
            return h;
        };
        for (p, v) in m {
            let field = |k: &str| v.get(k).and_then(serde_json::Value::as_i64).unwrap_or(0);
            h.visits.insert(
                p,
                Visit {
                    count: field("count"),
                    last: field("last"),
                },
            );
        }
        h
    }

    /// visit reports what is known about one repository; the default Visit
    /// means never entered.
    pub fn visit(&self, path: &str) -> Visit {
        self.visits.get(path).copied().unwrap_or_default()
    }

    /// bump records a visit and prunes entries whose repository is gone. It
    /// applies both to the log as it is on disk right now, not to the copy
    /// read when this History was opened: the finder holds that copy for a
    /// whole session, and every visit another gm wrote in the meantime would
    /// otherwise be written back out of existence.
    pub fn bump(&mut self, path: &str) -> Result<()> {
        if !self.path.is_empty() {
            self.visits = History::open(&self.path).visits;
        }
        let v = self.visits.entry(path.to_string()).or_default();
        v.count += 1;
        v.last = now();
        self.visits.retain(|p, _| {
            !matches!(std::fs::metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        });
        self.save()
    }

    /// save writes through a temporary file, so an interrupted write cannot
    /// leave a half-written log behind.
    fn save(&self) -> Result<()> {
        if self.path.is_empty() {
            return Ok(());
        }
        let dir = paths::dir(&self.path);
        std::fs::create_dir_all(&dir)?;
        let body: serde_json::Map<String, serde_json::Value> = self
            .visits
            .iter()
            .map(|(p, v)| {
                (
                    p.clone(),
                    serde_json::json!({"count": v.count, "last": v.last}),
                )
            })
            .collect();
        // A unique name, so two gm processes saving at once cannot write and
        // rename the same temporary file.
        let tmp = paths::join(
            &dir,
            &format!("frecency-{}-{}.tmp", std::process::id(), nanos()),
        );
        let written = (|| -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            f.write_all(serde_json::Value::Object(body).to_string().as_bytes())?;
            drop(f);
            std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o644))?;
            std::fs::rename(&tmp, &self.path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp); // no half-written log left next to the real one
        }
        Ok(written?)
    }
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDir, mkdir};

    #[test]
    fn score_ranks_recent_first() {
        let n = now();
        let recent = Visit {
            count: 2,
            last: n - 30 * 60,
        };
        let stale = Visit {
            count: 10,
            last: n - 60 * 24 * 3600,
        };
        assert!(recent.score(n) > stale.score(n));
        assert_eq!(Visit::default().score(n), 0.0);
    }

    // The log's contract: a bump survives a reload, and a repository that no
    // longer exists is pruned.
    #[test]
    fn round_trip() {
        let dir = TempDir::new();
        let path = dir.join("frecency.json");
        let live = dir.join("live");
        mkdir(&live);

        let mut h = History::open(&path);
        h.bump(&live).unwrap();
        h.bump(&dir.join("gone")).unwrap();

        let again = History::open(&path);
        assert_eq!(again.visit(&live).count, 1);
        assert_eq!(again.visit(&dir.join("gone")).count, 0);
    }

    // The window the finder holds open: a visit written by another gm after
    // this History was opened must survive this History's own bump.
    #[test]
    fn bump_keeps_concurrent_visits() {
        let dir = TempDir::new();
        let path = dir.join("frecency.json");
        let (a, b) = (dir.join("a"), dir.join("b"));
        mkdir(&a);
        mkdir(&b);

        let mut h = History::open(&path); // the finder opens the log and holds it
        History::open(&path).bump(&b).unwrap();
        h.bump(&a).unwrap();

        let again = History::open(&path);
        assert_eq!(again.visit(&a).count, 1);
        assert_eq!(
            again.visit(&b).count,
            1,
            "a visit written while the finder was open is lost"
        );
        assert_eq!(
            h.visit(&b).count,
            1,
            "the in-memory log disagrees with the file"
        );
    }
}
