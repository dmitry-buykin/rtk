//! Token savings tracking and analytics system.
//!
//! This module provides comprehensive tracking of RTK command executions,
//! recording token savings, execution times, and providing aggregation APIs
//! for daily/weekly/monthly statistics.
//!
//! # Architecture
//!
//! - Storage: SQLite database (~/.local/share/rtk/tracking.db)
//! - Retention: 90-day automatic cleanup
//! - Metrics: Input/output tokens, savings %, execution time
//!
//! # Quick Start
//!
//! ```no_run
//! use rtk::tracking::{TimedExecution, Tracker};
//!
//! // Track a command execution
//! let timer = TimedExecution::start();
//! let input = "raw output";
//! let output = "filtered output";
//! timer.track("ls -la", "rtk ls", input, output);
//!
//! // Query statistics
//! let tracker = Tracker::new().unwrap();
//! let summary = tracker.get_summary().unwrap();
//! println!("Saved {} tokens", summary.total_saved);
//! ```
//!
//! See [docs/tracking.md](../docs/tracking.md) for full documentation.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

/// Number of days to retain tracking history before automatic cleanup.
const HISTORY_DAYS: i64 = 90;
const TRACKING_DB_FILE: &str = "history.db";
const REDACTED_VALUE: &str = "<redacted>";

/// Main tracking interface for recording and querying command history.
///
/// Manages SQLite database connection and provides methods for:
/// - Recording command executions with token counts and timing
/// - Querying aggregated statistics (summary, daily, weekly, monthly)
/// - Retrieving recent command history
///
/// # Database Location
///
/// - Linux: `~/.local/share/rtk/tracking.db`
/// - macOS: `~/Library/Application Support/rtk/tracking.db`
/// - Windows: `%APPDATA%\rtk\tracking.db`
///
/// # Examples
///
/// ```no_run
/// use rtk::tracking::Tracker;
///
/// let tracker = Tracker::new()?;
/// tracker.record("ls -la", "rtk ls", 1000, 200, 50)?;
///
/// let summary = tracker.get_summary()?;
/// println!("Total saved: {} tokens", summary.total_saved);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub struct Tracker {
    conn: Option<Connection>,
}

/// Individual command record from tracking history.
///
/// Contains timestamp, command name, and savings metrics for a single execution.
#[derive(Debug)]
pub struct CommandRecord {
    /// UTC timestamp when command was executed
    pub timestamp: DateTime<Utc>,
    /// RTK command that was executed (e.g., "rtk ls")
    pub rtk_cmd: String,
    /// Number of tokens saved (input - output)
    pub saved_tokens: usize,
    /// Savings percentage ((saved / input) * 100)
    pub savings_pct: f64,
}

/// Aggregated statistics across all recorded commands.
///
/// Provides overall metrics and breakdowns by command and by day.
/// Returned by [`Tracker::get_summary`].
#[derive(Debug)]
pub struct GainSummary {
    /// Total number of commands recorded
    pub total_commands: usize,
    /// Total input tokens across all commands
    pub total_input: usize,
    /// Total output tokens across all commands
    pub total_output: usize,
    /// Total tokens saved (input - output)
    pub total_saved: usize,
    /// Average savings percentage across all commands
    pub avg_savings_pct: f64,
    /// Total execution time across all commands (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
    /// Top 10 commands by tokens saved: (cmd, count, saved, avg_pct, avg_time_ms)
    pub by_command: Vec<(String, usize, usize, f64, u64)>,
    /// Last 30 days of activity: (date, saved_tokens)
    pub by_day: Vec<(String, usize)>,
}

/// Daily statistics for token savings and execution metrics.
///
/// Serializable to JSON for export via `rtk gain --daily --format json`.
///
/// # JSON Schema
///
/// ```json
/// {
///   "date": "2026-02-03",
///   "commands": 42,
///   "input_tokens": 15420,
///   "output_tokens": 3842,
///   "saved_tokens": 11578,
///   "savings_pct": 75.08,
///   "total_time_ms": 8450,
///   "avg_time_ms": 201
/// }
/// ```
#[derive(Debug, Serialize)]
pub struct DayStats {
    /// ISO date (YYYY-MM-DD)
    pub date: String,
    /// Number of commands executed this day
    pub commands: usize,
    /// Total input tokens for this day
    pub input_tokens: usize,
    /// Total output tokens for this day
    pub output_tokens: usize,
    /// Total tokens saved this day
    pub saved_tokens: usize,
    /// Savings percentage for this day
    pub savings_pct: f64,
    /// Total execution time for this day (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
}

/// Weekly statistics for token savings and execution metrics.
///
/// Serializable to JSON for export via `rtk gain --weekly --format json`.
/// Weeks start on Sunday (SQLite default).
#[derive(Debug, Serialize)]
pub struct WeekStats {
    /// Week start date (YYYY-MM-DD)
    pub week_start: String,
    /// Week end date (YYYY-MM-DD)
    pub week_end: String,
    /// Number of commands executed this week
    pub commands: usize,
    /// Total input tokens for this week
    pub input_tokens: usize,
    /// Total output tokens for this week
    pub output_tokens: usize,
    /// Total tokens saved this week
    pub saved_tokens: usize,
    /// Savings percentage for this week
    pub savings_pct: f64,
    /// Total execution time for this week (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
}

/// Monthly statistics for token savings and execution metrics.
///
/// Serializable to JSON for export via `rtk gain --monthly --format json`.
#[derive(Debug, Serialize)]
pub struct MonthStats {
    /// Month identifier (YYYY-MM)
    pub month: String,
    /// Number of commands executed this month
    pub commands: usize,
    /// Total input tokens for this month
    pub input_tokens: usize,
    /// Total output tokens for this month
    pub output_tokens: usize,
    /// Total tokens saved this month
    pub saved_tokens: usize,
    /// Savings percentage for this month
    pub savings_pct: f64,
    /// Total execution time for this month (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
}

impl Tracker {
    /// Create a new tracker instance.
    ///
    /// Opens or creates the SQLite database at the platform-specific location.
    /// Automatically creates the `commands` table if it doesn't exist and runs
    /// any necessary schema migrations.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Cannot determine database path
    /// - Cannot create parent directories
    /// - Cannot open/create SQLite database
    /// - Schema creation/migration fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new() -> Result<Self> {
        if !is_tracking_enabled()? {
            return Ok(Self { conn: None });
        }

        let db_path = get_db_path()?;
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS commands (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                original_cmd TEXT NOT NULL,
                rtk_cmd TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                saved_tokens INTEGER NOT NULL,
                savings_pct REAL NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON commands(timestamp)",
            [],
        )?;

        // Migration: add exec_time_ms column if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE commands ADD COLUMN exec_time_ms INTEGER DEFAULT 0",
            [],
        );

        enforce_local_db_permissions(&db_path)?;

        Ok(Self { conn: Some(conn) })
    }

    /// Record a command execution with token counts and timing.
    ///
    /// Calculates savings metrics and stores the record in the database.
    /// Automatically cleans up records older than 90 days after insertion.
    ///
    /// # Arguments
    ///
    /// - `original_cmd`: The standard command (e.g., "ls -la")
    /// - `rtk_cmd`: The RTK command used (e.g., "rtk ls")
    /// - `input_tokens`: Estimated tokens from standard command output
    /// - `output_tokens`: Actual tokens from RTK output
    /// - `exec_time_ms`: Execution time in milliseconds
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// tracker.record("ls -la", "rtk ls", 1000, 200, 50)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn record(
        &self,
        original_cmd: &str,
        rtk_cmd: &str,
        input_tokens: usize,
        output_tokens: usize,
        exec_time_ms: u64,
    ) -> Result<()> {
        let conn = match &self.conn {
            Some(conn) => conn,
            None => return Ok(()),
        };

        let saved = input_tokens.saturating_sub(output_tokens);
        let pct = if input_tokens > 0 {
            (saved as f64 / input_tokens as f64) * 100.0
        } else {
            0.0
        };

        let original_cmd = sanitize_command_for_tracking(original_cmd);
        let rtk_cmd = sanitize_command_for_tracking(rtk_cmd);

        conn.execute(
            "INSERT INTO commands (timestamp, original_cmd, rtk_cmd, input_tokens, output_tokens, saved_tokens, savings_pct, exec_time_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Utc::now().to_rfc3339(),
                original_cmd,
                rtk_cmd,
                input_tokens as i64,
                output_tokens as i64,
                saved as i64,
                pct,
                exec_time_ms as i64
            ],
        )?;

        let _ = conn;
        self.cleanup_old()?;
        Ok(())
    }

    fn cleanup_old(&self) -> Result<()> {
        let conn = match &self.conn {
            Some(conn) => conn,
            None => return Ok(()),
        };

        let cutoff = Utc::now() - chrono::Duration::days(HISTORY_DAYS);
        conn.execute(
            "DELETE FROM commands WHERE timestamp < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Get overall summary statistics across all recorded commands.
    ///
    /// Returns aggregated metrics including:
    /// - Total commands, tokens (input/output/saved)
    /// - Average savings percentage and execution time
    /// - Top 10 commands by tokens saved
    /// - Last 30 days of activity
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let summary = tracker.get_summary()?;
    /// println!("Saved {} tokens ({:.1}%)",
    ///     summary.total_saved, summary.avg_savings_pct);
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_summary(&self) -> Result<GainSummary> {
        let mut total_commands = 0usize;
        let mut total_input = 0usize;
        let mut total_output = 0usize;
        let mut total_saved = 0usize;
        let mut total_time_ms = 0u64;

        let Some(conn) = self.conn.as_ref() else {
            return Ok(GainSummary {
                total_commands: 0,
                total_input: 0,
                total_output: 0,
                total_saved: 0,
                avg_savings_pct: 0.0,
                total_time_ms: 0,
                avg_time_ms: 0,
                by_command: Vec::new(),
                by_day: Vec::new(),
            });
        };

        let mut stmt = conn.prepare(
            "SELECT input_tokens, output_tokens, saved_tokens, exec_time_ms FROM commands",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, i64>(3)? as u64,
            ))
        })?;

        for row in rows {
            let (input, output, saved, time_ms) = row?;
            total_commands += 1;
            total_input += input;
            total_output += output;
            total_saved += saved;
            total_time_ms += time_ms;
        }

        let avg_savings_pct = if total_input > 0 {
            (total_saved as f64 / total_input as f64) * 100.0
        } else {
            0.0
        };

        let avg_time_ms = if total_commands > 0 {
            total_time_ms / total_commands as u64
        } else {
            0
        };

        let by_command = self.get_by_command()?;
        let by_day = self.get_by_day()?;

        Ok(GainSummary {
            total_commands,
            total_input,
            total_output,
            total_saved,
            avg_savings_pct,
            total_time_ms,
            avg_time_ms,
            by_command,
            by_day,
        })
    }

    fn get_by_command(&self) -> Result<Vec<(String, usize, usize, f64, u64)>> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(Vec::new());
        };

        let mut stmt = conn.prepare(
            "SELECT rtk_cmd, COUNT(*), SUM(saved_tokens), AVG(savings_pct), AVG(exec_time_ms)
             FROM commands
             GROUP BY rtk_cmd
             ORDER BY SUM(saved_tokens) DESC
             LIMIT 10",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)? as u64,
            ))
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn get_by_day(&self) -> Result<Vec<(String, usize)>> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(Vec::new());
        };

        let mut stmt = conn.prepare(
            "SELECT DATE(timestamp), SUM(saved_tokens)
             FROM commands
             GROUP BY DATE(timestamp)
             ORDER BY DATE(timestamp) DESC
             LIMIT 30",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get daily statistics for all recorded days.
    ///
    /// Returns one [`DayStats`] per day with commands executed, tokens saved,
    /// and execution time metrics. Results are ordered chronologically (oldest first).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let days = tracker.get_all_days()?;
    /// for day in days.iter().take(7) {
    ///     println!("{}: {} commands, {} tokens saved",
    ///         day.date, day.commands, day.saved_tokens);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_all_days(&self) -> Result<Vec<DayStats>> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(Vec::new());
        };

        let mut stmt = conn.prepare(
            "SELECT
                DATE(timestamp) as date,
                COUNT(*) as commands,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(saved_tokens) as saved,
                SUM(exec_time_ms) as total_time
             FROM commands
             GROUP BY DATE(timestamp)
             ORDER BY DATE(timestamp) DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let input = row.get::<_, i64>(2)? as usize;
            let saved = row.get::<_, i64>(4)? as usize;
            let commands = row.get::<_, i64>(1)? as usize;
            let total_time = row.get::<_, i64>(5)? as u64;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            let avg_time_ms = if commands > 0 {
                total_time / commands as u64
            } else {
                0
            };

            Ok(DayStats {
                date: row.get(0)?,
                commands,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(3)? as usize,
                saved_tokens: saved,
                savings_pct,
                total_time_ms: total_time,
                avg_time_ms,
            })
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get weekly statistics grouped by week.
    ///
    /// Returns one [`WeekStats`] per week with aggregated metrics.
    /// Weeks start on Sunday (SQLite default). Results ordered chronologically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let weeks = tracker.get_by_week()?;
    /// for week in weeks {
    ///     println!("{} to {}: {} tokens saved",
    ///         week.week_start, week.week_end, week.saved_tokens);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_by_week(&self) -> Result<Vec<WeekStats>> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(Vec::new());
        };

        let mut stmt = conn.prepare(
            "SELECT
                DATE(timestamp, 'weekday 0', '-6 days') as week_start,
                DATE(timestamp, 'weekday 0') as week_end,
                COUNT(*) as commands,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(saved_tokens) as saved,
                SUM(exec_time_ms) as total_time
             FROM commands
             GROUP BY week_start
             ORDER BY week_start DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let input = row.get::<_, i64>(3)? as usize;
            let saved = row.get::<_, i64>(5)? as usize;
            let commands = row.get::<_, i64>(2)? as usize;
            let total_time = row.get::<_, i64>(6)? as u64;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            let avg_time_ms = if commands > 0 {
                total_time / commands as u64
            } else {
                0
            };

            Ok(WeekStats {
                week_start: row.get(0)?,
                week_end: row.get(1)?,
                commands,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(4)? as usize,
                saved_tokens: saved,
                savings_pct,
                total_time_ms: total_time,
                avg_time_ms,
            })
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get monthly statistics grouped by month.
    ///
    /// Returns one [`MonthStats`] per month (YYYY-MM format) with aggregated metrics.
    /// Results ordered chronologically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let months = tracker.get_by_month()?;
    /// for month in months {
    ///     println!("{}: {} tokens saved ({:.1}%)",
    ///         month.month, month.saved_tokens, month.savings_pct);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_by_month(&self) -> Result<Vec<MonthStats>> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(Vec::new());
        };

        let mut stmt = conn.prepare(
            "SELECT
                strftime('%Y-%m', timestamp) as month,
                COUNT(*) as commands,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(saved_tokens) as saved,
                SUM(exec_time_ms) as total_time
             FROM commands
             GROUP BY month
             ORDER BY month DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let input = row.get::<_, i64>(2)? as usize;
            let saved = row.get::<_, i64>(4)? as usize;
            let commands = row.get::<_, i64>(1)? as usize;
            let total_time = row.get::<_, i64>(5)? as u64;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            let avg_time_ms = if commands > 0 {
                total_time / commands as u64
            } else {
                0
            };

            Ok(MonthStats {
                month: row.get(0)?,
                commands,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(3)? as usize,
                saved_tokens: saved,
                savings_pct,
                total_time_ms: total_time,
                avg_time_ms,
            })
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get recent command history.
    ///
    /// Returns up to `limit` most recent command records, ordered by timestamp (newest first).
    ///
    /// # Arguments
    ///
    /// - `limit`: Maximum number of records to return
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let recent = tracker.get_recent(10)?;
    /// for cmd in recent {
    ///     println!("{}: {} saved {:.1}%",
    ///         cmd.timestamp, cmd.rtk_cmd, cmd.savings_pct);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_recent(&self, limit: usize) -> Result<Vec<CommandRecord>> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(Vec::new());
        };

        let mut stmt = conn.prepare(
            "SELECT timestamp, rtk_cmd, saved_tokens, savings_pct
             FROM commands
             ORDER BY timestamp DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(CommandRecord {
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(0)?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                rtk_cmd: row.get(1)?,
                saved_tokens: row.get::<_, i64>(2)? as usize,
                savings_pct: row.get(3)?,
            })
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn get_db_path() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    let data_root = data_dir.join("rtk");

    let fallback = data_root.join(TRACKING_DB_FILE);

    // Priority 1: Environment variable RTK_DB_PATH
    if let Ok(custom_path) = std::env::var("RTK_DB_PATH") {
        return Ok(sanitize_tracking_db_path(PathBuf::from(custom_path), &data_root));
    }

    // Priority 2: Configuration file
    if let Ok(config) = crate::config::Config::load() {
        if let Some(db_path) = config.tracking.database_path {
            return Ok(sanitize_tracking_db_path(db_path, &data_root));
        }
    }

    Ok(fallback)
}

fn sanitize_tracking_db_path(path: PathBuf, data_root: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return data_root.join(TRACKING_DB_FILE);
    }

    if path.is_relative() {
        if has_parent_dir_component(&path) {
            return data_root.join(TRACKING_DB_FILE);
        }
        return data_root.join(path);
    }

    if has_parent_dir_component(&path) {
        return data_root.join(TRACKING_DB_FILE);
    }

    if path.starts_with(data_root) {
        return path;
    }

    data_root.join(TRACKING_DB_FILE)
}

fn has_parent_dir_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::ParentDir))
}

fn enforce_local_db_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o777 != 0o600 {
                perms.set_mode(0o600);
                let _ = fs::set_permissions(path, perms);
            }
        }
    }

    Ok(())
}

fn is_tracking_enabled() -> Result<bool> {
    if let Ok(raw) = std::env::var("RTK_TRACKING") {
        if let Some(enabled) = parse_bool_env(raw.trim()) {
            return Ok(enabled);
        }
    }

    if let Ok(raw) = std::env::var("RTK_TRACKING_ENABLED") {
        if let Some(enabled) = parse_bool_env(raw.trim()) {
            return Ok(enabled);
        }
    }

    if let Ok(raw) = std::env::var("RTK_DISABLE_TRACKING") {
        if let Some(disabled) = parse_bool_env(raw.trim()) {
            return Ok(!disabled);
        }
    }

    Ok(crate::config::Config::load()
        .map(|config| config.tracking.enabled)
        .unwrap_or(false))
}

fn parse_bool_env(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enable" | "enabled" => Some(true),
        "0" | "false" | "no" | "off" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

/// Estimate token count from text using ~4 chars = 1 token heuristic.
///
/// This is a fast approximation suitable for tracking purposes.
/// For precise counts, integrate with your LLM's tokenizer API.
///
/// # Formula
///
/// `tokens = ceil(chars / 4)`
///
/// # Examples
///
/// ```
/// use rtk::tracking::estimate_tokens;
///
/// assert_eq!(estimate_tokens(""), 0);
/// assert_eq!(estimate_tokens("abcd"), 1);  // 4 chars = 1 token
/// assert_eq!(estimate_tokens("abcde"), 2); // 5 chars = ceil(1.25) = 2
/// assert_eq!(estimate_tokens("hello world"), 3); // 11 chars = ceil(2.75) = 3
/// ```
pub fn estimate_tokens(text: &str) -> usize {
    // ~4 chars per token on average
    (text.len() as f64 / 4.0).ceil() as usize
}

/// Helper struct for timing command execution
/// Helper for timing command execution and tracking results.
///
/// Preferred API for tracking commands. Automatically measures execution time
/// and records token savings. Use instead of the deprecated [`track`] function.
///
/// # Examples
///
/// ```no_run
/// use rtk::tracking::TimedExecution;
///
/// let timer = TimedExecution::start();
/// let input = execute_standard_command()?;
/// let output = execute_rtk_command()?;
/// timer.track("ls -la", "rtk ls", &input, &output);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub struct TimedExecution {
    start: Instant,
}

impl TimedExecution {
    /// Start timing a command execution.
    ///
    /// Creates a new timer that starts measuring elapsed time immediately.
    /// Call [`track`](Self::track) or [`track_passthrough`](Self::track_passthrough)
    /// when the command completes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::TimedExecution;
    ///
    /// let timer = TimedExecution::start();
    /// // ... execute command ...
    /// timer.track("cmd", "rtk cmd", "input", "output");
    /// ```
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Track the command with elapsed time and token counts.
    ///
    /// Records the command execution with:
    /// - Elapsed time since [`start`](Self::start)
    /// - Token counts estimated from input/output strings
    /// - Calculated savings metrics
    ///
    /// # Arguments
    ///
    /// - `original_cmd`: Standard command (e.g., "ls -la")
    /// - `rtk_cmd`: RTK command used (e.g., "rtk ls")
    /// - `input`: Standard command output (for token estimation)
    /// - `output`: RTK command output (for token estimation)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::TimedExecution;
    ///
    /// let timer = TimedExecution::start();
    /// let input = "long output...";
    /// let output = "short output";
    /// timer.track("ls -la", "rtk ls", input, output);
    /// ```
    pub fn track(&self, original_cmd: &str, rtk_cmd: &str, input: &str, output: &str) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let input_tokens = estimate_tokens(input);
        let output_tokens = estimate_tokens(output);

        if let Ok(tracker) = Tracker::new() {
            let _ = tracker.record(
                original_cmd,
                rtk_cmd,
                input_tokens,
                output_tokens,
                elapsed_ms,
            );
        }
    }

    /// Track passthrough commands (timing-only, no token counting).
    ///
    /// For commands that stream output or run interactively where output
    /// cannot be captured. Records execution time but sets tokens to 0
    /// (does not dilute savings statistics).
    ///
    /// # Arguments
    ///
    /// - `original_cmd`: Standard command (e.g., "git tag --list")
    /// - `rtk_cmd`: RTK command used (e.g., "rtk git tag --list")
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rtk::tracking::TimedExecution;
    ///
    /// let timer = TimedExecution::start();
    /// // ... execute streaming command ...
    /// timer.track_passthrough("git tag", "rtk git tag");
    /// ```
    pub fn track_passthrough(&self, original_cmd: &str, rtk_cmd: &str) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        // input_tokens=0, output_tokens=0 won't dilute savings statistics
        if let Ok(tracker) = Tracker::new() {
            let _ = tracker.record(original_cmd, rtk_cmd, 0, 0, elapsed_ms);
        }
    }
}

/// Format OsString args for tracking display.
///
/// Joins arguments with spaces, converting each to UTF-8 (lossy).
/// Useful for displaying command arguments in tracking records.
///
/// # Examples
///
/// ```
/// use std::ffi::OsString;
/// use rtk::tracking::args_display;
///
/// let args = vec![OsString::from("status"), OsString::from("--short")];
/// assert_eq!(args_display(&args), "status --short");
/// ```
pub fn args_display(args: &[OsString]) -> String {
    args.iter()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Track a command execution (legacy function, use [`TimedExecution`] for new code).
///
/// # Deprecation Notice
///
/// This function is deprecated. Use [`TimedExecution`] instead for automatic
/// timing and cleaner API.
///
/// # Arguments
///
/// - `original_cmd`: Standard command (e.g., "ls -la")
/// - `rtk_cmd`: RTK command used (e.g., "rtk ls")
/// - `input`: Standard command output (for token estimation)
/// - `output`: RTK command output (for token estimation)
///
/// # Migration
///
/// ```no_run
/// # use rtk::tracking::{track, TimedExecution};
/// // Old (deprecated)
/// track("ls -la", "rtk ls", "input", "output");
///
/// // New (preferred)
/// let timer = TimedExecution::start();
/// timer.track("ls -la", "rtk ls", "input", "output");
/// ```
#[deprecated(note = "Use TimedExecution instead")]
#[allow(dead_code)]
pub fn track(original_cmd: &str, rtk_cmd: &str, input: &str, output: &str) {
    let input_tokens = estimate_tokens(input);
    let output_tokens = estimate_tokens(output);

    if let Ok(tracker) = Tracker::new() {
        let _ = tracker.record(original_cmd, rtk_cmd, input_tokens, output_tokens, 0);
    }
}

pub(crate) fn sanitize_command_for_tracking(command: &str) -> String {
    if command.is_empty() {
        return String::new();
    }

    let mut result = Vec::new();
    let mut redact_next = false;

    for token in command.split_whitespace() {
        if redact_next {
            result.push(REDACTED_VALUE.to_string());
            let normalized = normalize_token_for_match(token);
            redact_next = matches!(normalized.as_str(), "bearer" | "basic" | "token");
            continue;
        }

        let mut masked_token = token.to_string();
        if let Some(redacted) = mask_url_credentials(&masked_token) {
            masked_token = redacted;
        }
        if let Some(redacted) = mask_sensitive_url_query(&masked_token) {
            masked_token = redacted;
        }
        if masked_token != token {
            result.push(masked_token);
            continue;
        }

        if let Some((name, _value)) = token.split_once('=') {
            if is_sensitive_key(name) {
                result.push(format!("{}={}", name, REDACTED_VALUE));
            } else {
                result.push(format!("{name}={_value}"));
            }
            continue;
        }

        if let Some((name, value)) = token.split_once(':') {
            if is_sensitive_key(name) {
                result.push(format!("{}:{}", name, REDACTED_VALUE));
                if token.ends_with(':') || should_redact_next_for_colon_value(value) {
                    redact_next = true;
                }
                continue;
            }
        }

        if is_sensitive_flag(token) {
            result.push(token.to_string());
            redact_next = true;
            continue;
        }

        if is_sensitive_key(token) {
            result.push(REDACTED_VALUE.to_string());
            continue;
        }

        result.push(token.to_string());
    }

    if redact_next {
        result.push(REDACTED_VALUE.to_string());
    }

    result.join(" ")
}

fn is_sensitive_flag(token: &str) -> bool {
    let key = token
        .trim_start_matches('-')
        .split(|c| c == '=' || c == ':')
        .next()
        .unwrap_or_default();

    is_sensitive_key(&key)
}

fn is_sensitive_key(token: &str) -> bool {
    let key = normalize_token_for_match(token);
    if key.is_empty() {
        return false;
    }

    const SENSITIVE_PARTS: [&str; 29] = [
        "token",
        "secret",
        "password",
        "auth",
        "authorization",
        "apikey",
        "api_key",
        "api-key",
        "access_token",
        "access-token",
        "auth_token",
        "auth-token",
        "private_key",
        "private-key",
        "credential",
        "client_secret",
        "client-secret",
        "private",
        "session",
        "jwt",
        "cookie",
        "key",
        "signature",
        "sessionid",
        "oauth",
        "refresh",
        "pass",
        "passphrase",
        "bearer",
    ];

    let parts = key.split(|c: char| !c.is_ascii_alphanumeric());
    if parts
        .clone()
        .filter(|part| !part.is_empty())
        .any(|part| SENSITIVE_PARTS.contains(&part))
    {
        return true;
    }

    let parts: Vec<_> = parts.filter(|part| !part.is_empty()).collect();
    parts
        .windows(2)
        .any(|window| {
            window[0] == "api" && (window[1] == "key" || window[1] == "token" || window[1] == "secret")
                || window[0] == "access" && window[1] == "token"
                || window[0] == "auth" && window[1] == "token"
                || window[0] == "private" && window[1] == "key"
                || window[0] == "client" && window[1] == "secret"
        })
}

fn normalize_token_for_match(token: &str) -> String {
    token
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .trim_start_matches('-')
        .trim_end_matches(':')
        .to_ascii_lowercase()
}

fn mask_url_credentials(token: &str) -> Option<String> {
    let scheme_end = token.find("://")?;
    let after = &token[scheme_end + 3..];
    let at = after.find('@')?;
    let creds = &after[..at];
    if creds.is_empty() {
        return None;
    }

    let masked = if creds.contains(':') {
        format!("{REDACTED_VALUE}:{REDACTED_VALUE}@")
    } else {
        format!("{REDACTED_VALUE}@")
    };

    Some(format!(
        "{}{}{}",
        &token[..scheme_end + 3],
        masked,
        &after[at + 1..]
    ))
}

fn mask_sensitive_url_query(token: &str) -> Option<String> {
    let query_start = token.find('?')?;
    let query_fragment = &token[query_start + 1..];
    let (query, fragment) = if let Some(hash_idx) = query_fragment.find('#') {
        (
            &query_fragment[..hash_idx],
            Some(&query_fragment[hash_idx + 1..]),
        )
    } else {
        (query_fragment, None)
    };

    if query.is_empty() {
        return None;
    }

    let mut changed = false;
    let mut pairs = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            pairs.push(String::new());
            continue;
        }

        if let Some((name, value)) = pair.split_once('=') {
            if is_sensitive_key(name) {
                pairs.push(format!("{name}={REDACTED_VALUE}"));
                changed = true;
            } else if value.is_empty() && is_sensitive_key(name.trim_end_matches(':')) {
                pairs.push(format!("{name}={REDACTED_VALUE}"));
                changed = true;
            } else {
                pairs.push(pair.to_string());
            }
        } else if is_sensitive_key(pair) {
            pairs.push(format!("{pair}={REDACTED_VALUE}"));
            changed = true;
        } else {
            pairs.push(pair.to_string());
        }
    }

    if !changed {
        return None;
    }

    let mut out = String::new();
    out.push_str(&token[..query_start + 1]);
    out.push_str(&pairs.join("&"));
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    Some(out)
}

fn should_redact_next_for_colon_value(value: &str) -> bool {
    matches!(
        normalize_token_for_match(value).as_str(),
        "bearer" | "basic" | "token"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    // 1. estimate_tokens — verify ~4 chars/token ratio
    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("abcde"), 2); // 5 chars = ceil(1.25) = 2
        assert_eq!(estimate_tokens("a"), 1); // 1 char = ceil(0.25) = 1
        assert_eq!(estimate_tokens("12345678"), 2); // 8 chars = 2 tokens
    }

    // 2. args_display — format OsString vec
    #[test]
    fn test_args_display() {
        let args = vec![OsString::from("status"), OsString::from("--short")];
        assert_eq!(args_display(&args), "status --short");
        assert_eq!(args_display(&[]), "");

        let single = vec![OsString::from("log")];
        assert_eq!(args_display(&single), "log");
    }

    // 3. Tracker::record + get_recent — round-trip DB
    #[test]
    fn test_tracker_record_and_recent() {
        let tracker = Tracker::new().expect("Failed to create tracker");

        // Use unique test identifier to avoid conflicts with other tests
        let test_cmd = format!("rtk git status test_{}", std::process::id());

        tracker
            .record("git status", &test_cmd, 100, 20, 50)
            .expect("Failed to record");

        let recent = tracker.get_recent(10).expect("Failed to get recent");

        // Find our specific test record
        let test_record = recent
            .iter()
            .find(|r| r.rtk_cmd == test_cmd)
            .expect("Test record not found in recent commands");

        assert_eq!(test_record.saved_tokens, 80);
        assert_eq!(test_record.savings_pct, 80.0);
    }

    // 4. track_passthrough doesn't dilute stats (input=0, output=0)
    #[test]
    fn test_track_passthrough_no_dilution() {
        let tracker = Tracker::new().expect("Failed to create tracker");

        // Use unique test identifiers
        let pid = std::process::id();
        let cmd1 = format!("rtk cmd1_test_{}", pid);
        let cmd2 = format!("rtk cmd2_passthrough_test_{}", pid);

        // Record one real command with 80% savings
        tracker
            .record("cmd1", &cmd1, 1000, 200, 10)
            .expect("Failed to record cmd1");

        // Record passthrough (0, 0)
        tracker
            .record("cmd2", &cmd2, 0, 0, 5)
            .expect("Failed to record passthrough");

        // Verify both records exist in recent history
        let recent = tracker.get_recent(20).expect("Failed to get recent");

        let record1 = recent
            .iter()
            .find(|r| r.rtk_cmd == cmd1)
            .expect("cmd1 record not found");
        let record2 = recent
            .iter()
            .find(|r| r.rtk_cmd == cmd2)
            .expect("passthrough record not found");

        // Verify cmd1 has 80% savings
        assert_eq!(record1.saved_tokens, 800);
        assert_eq!(record1.savings_pct, 80.0);

        // Verify passthrough has 0% savings
        assert_eq!(record2.saved_tokens, 0);
        assert_eq!(record2.savings_pct, 0.0);

        // This validates that passthrough (0 input, 0 output) doesn't dilute stats
        // because the savings calculation is correct for both cases
    }

    // 5. TimedExecution::track records with exec_time > 0
    #[test]
    fn test_timed_execution_records_time() {
        let timer = TimedExecution::start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        timer.track("test cmd", "rtk test", "raw input data", "filtered");

        // Verify via DB that record exists
        let tracker = Tracker::new().expect("Failed to create tracker");
        let recent = tracker.get_recent(5).expect("Failed to get recent");
        assert!(recent.iter().any(|r| r.rtk_cmd == "rtk test"));
    }

    // 6. TimedExecution::track_passthrough records with 0 tokens
    #[test]
    fn test_timed_execution_passthrough() {
        let timer = TimedExecution::start();
        timer.track_passthrough("git tag", "rtk git tag (passthrough)");

        let tracker = Tracker::new().expect("Failed to create tracker");
        let recent = tracker.get_recent(5).expect("Failed to get recent");

        let pt = recent
            .iter()
            .find(|r| r.rtk_cmd.contains("passthrough"))
            .expect("Passthrough record not found");

        // savings_pct should be 0 for passthrough
        assert_eq!(pt.savings_pct, 0.0);
        assert_eq!(pt.saved_tokens, 0);
    }

    // 7. sanitize_command_for_tracking redacts likely secrets
    #[test]
    fn test_sanitize_command_for_tracking_redacts_sensitive_tokens() {
        assert_eq!(
            sanitize_command_for_tracking("API_KEY=abc123"),
            format!("API_KEY={}", REDACTED_VALUE)
        );
        assert_eq!(
            sanitize_command_for_tracking("curl -H Authorization: Bearer deadbeef"),
            format!(
                "curl -H Authorization:{} {} {}",
                REDACTED_VALUE, REDACTED_VALUE, REDACTED_VALUE
            )
        );
        assert_eq!(sanitize_command_for_tracking("git status"), "git status");
    }

    #[test]
    fn test_mask_url_credentials() {
        assert_eq!(
            sanitize_command_for_tracking("curl https://u:p@api.example.com/data"),
            format!("curl https://{}:{}@api.example.com/data", REDACTED_VALUE, REDACTED_VALUE)
        );
        assert_eq!(
            sanitize_command_for_tracking("curl https://user@api.example.com/data"),
            "curl https://<redacted>@api.example.com/data".to_string()
        );
    }

    #[test]
    fn test_mask_url_query_sensitive_params() {
        assert_eq!(
            sanitize_command_for_tracking(
                "curl https://api.example.com/data?foo=1&access_token=abc123&signature=deadbeef"
            ),
            format!(
                "curl https://api.example.com/data?foo=1&access_token={}&signature={}",
                REDACTED_VALUE, REDACTED_VALUE
            )
        );
    }

    // 9. get_db_path respects environment variable RTK_DB_PATH
    #[test]
    fn test_custom_db_path_env() {
        use std::env;
        let _guard = env_test_lock().lock().expect("env test mutex poisoned");

        let custom_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rtk")
            .join("rtk_test_custom.db");
        env::set_var("RTK_DB_PATH", custom_path.to_string_lossy().as_ref());

        let db_path = get_db_path().expect("Failed to get db path");
        assert_eq!(db_path, custom_path);

        env::remove_var("RTK_DB_PATH");
    }

    #[test]
    fn test_custom_db_path_env_rejects_external_path() {
        use std::env;
        let _guard = env_test_lock().lock().expect("env test mutex poisoned");

        env::set_var("RTK_DB_PATH", "/tmp/rtk_test_custom.db");

        let data_root = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rtk");
        let expected = data_root.join(TRACKING_DB_FILE);

        let db_path = get_db_path().expect("Failed to get db path");
        assert_eq!(db_path, expected);

        env::remove_var("RTK_DB_PATH");
    }

    // 10. get_db_path falls back to default when no custom config
    #[test]
    fn test_default_db_path() {
        use std::env;
        let _guard = env_test_lock().lock().expect("env test mutex poisoned");

        // Ensure no env var is set
        env::remove_var("RTK_DB_PATH");

        let db_path = get_db_path().expect("Failed to get db path");
        assert!(db_path.ends_with("rtk/history.db"));
    }
}
