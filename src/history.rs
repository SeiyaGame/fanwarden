//! Time series stored in SQLite: temperatures, fan speeds and zone duties.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Clone)]
pub struct History {
    conn: Arc<Mutex<Connection>>,
}

/// Series aligned on common timestamps, the shape uPlot expects.
#[derive(Debug, Serialize, PartialEq)]
pub struct HistoryData {
    pub timestamps: Vec<u64>,
    pub series: Vec<HistorySeries>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct HistorySeries {
    pub key: String,
    pub values: Vec<Option<f32>>,
}

impl History {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS series (id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE);
             CREATE TABLE IF NOT EXISTS samples (
                 series INTEGER NOT NULL REFERENCES series(id),
                 ts INTEGER NOT NULL,
                 value REAL NOT NULL,
                 PRIMARY KEY (series, ts)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS samples_ts ON samples (ts);",
        )?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn record(&self, ts: u64, values: &[(String, f32)]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut upsert_series = tx.prepare_cached(
                "INSERT INTO series (key) VALUES (?1) ON CONFLICT(key) DO UPDATE SET key = key RETURNING id",
            )?;
            let mut insert =
                tx.prepare_cached("INSERT OR REPLACE INTO samples (series, ts, value) VALUES (?1, ?2, ?3)")?;
            for (key, value) in values {
                if !value.is_finite() {
                    continue;
                }
                let id: i64 = upsert_series.query_row(params![key], |row| row.get(0))?;
                insert.execute(params![id, ts as i64, *value as f64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn prune(&self, older_than: u64) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM samples WHERE ts < ?1", params![older_than as i64])?)
    }

    pub fn keys(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key FROM series ORDER BY key")?;
        let keys = stmt.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
        Ok(keys)
    }

    /// Averages samples in `[from, to]` into at most `max_points` buckets.
    /// `prefixes` filters series by key prefix (`temp:`, `fan:`, `duty:`); empty means all.
    pub fn query(&self, from: u64, to: u64, max_points: u64, prefixes: &[String]) -> anyhow::Result<HistoryData> {
        let bucket = ((to.saturating_sub(from)) / max_points.max(1)).max(1) as i64;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT s.key, (m.ts / ?3) * ?3 AS b, AVG(m.value)
             FROM samples m JOIN series s ON s.id = m.series
             WHERE m.ts BETWEEN ?1 AND ?2
             GROUP BY m.series, b",
        )?;
        let rows = stmt.query_map(params![from as i64, to as i64, bucket], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, f64>(2)?))
        })?;

        let mut by_key: BTreeMap<String, HashMap<i64, f32>> = BTreeMap::new();
        let mut buckets = std::collections::BTreeSet::new();
        for row in rows {
            let (key, b, value) = row?;
            if !prefixes.is_empty() && !prefixes.iter().any(|p| key.starts_with(p.as_str())) {
                continue;
            }
            buckets.insert(b);
            by_key.entry(key).or_default().insert(b, ((value * 10.0).round() / 10.0) as f32);
        }
        let buckets: Vec<i64> = buckets.into_iter().collect();
        Ok(HistoryData {
            timestamps: buckets.iter().map(|&b| b as u64).collect(),
            series: by_key
                .into_iter()
                .map(|(key, points)| HistorySeries {
                    key,
                    values: buckets.iter().map(|b| points.get(b).copied()).collect(),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_query_and_prune() {
        let h = History::in_memory().unwrap();
        h.record(100, &[("temp:cpu".into(), 40.0), ("fan:FAN1".into(), 1000.0)]).unwrap();
        h.record(110, &[("temp:cpu".into(), 42.0)]).unwrap();
        h.record(200, &[("temp:cpu".into(), 50.0), ("fan:FAN1".into(), 1200.0)]).unwrap();

        // Two buckets of 50s: [100,150) and [200,250).
        let data = h.query(100, 200, 2, &[]).unwrap();
        assert_eq!(data.timestamps, vec![100, 200]);
        assert_eq!(data.series.len(), 2);
        let cpu = data.series.iter().find(|s| s.key == "temp:cpu").unwrap();
        assert_eq!(cpu.values, vec![Some(41.0), Some(50.0)]);

        let temps = h.query(0, 1000, 1000, &["temp:".into()]).unwrap();
        assert_eq!(temps.series.len(), 1);

        assert_eq!(h.prune(150).unwrap(), 3);
        assert_eq!(h.query(0, 1000, 10, &[]).unwrap().timestamps, vec![200]);
        assert_eq!(h.keys().unwrap(), vec!["fan:FAN1", "temp:cpu"]);
    }
}
