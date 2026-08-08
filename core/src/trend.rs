//! The history DSM does not keep.
//!
//! `SYNO.Core.System.Utilization` is a snapshot. `SYNO.ResourceMonitor.Setting`
//! on a DS-series reports `enable_history: false`, and `SYNO.ResourceMonitor.Log`
//! answers with an empty list — so there is no 24-hour, 7-day or 30-day series
//! to fetch. The app records its own.
//!
//! Recording every poll for 30 days would be half a million samples at a
//! five-second interval, so a sample is offered to four tiers and each tier
//! accepts one only once its own interval has elapsed. That bounds the whole
//! store at a few thousand points, which is both cheap to persist and already
//! more points than a chart has pixels.
//!
//! Every function whose answer depends on the time takes `now` as an argument.
//! The clock is the caller's business, which is what makes this testable
//! without waiting.

use std::collections::VecDeque;
use std::path::Path;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Utilization;

/// The ranges the UI offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Range {
    Hour,
    Day,
    Week,
    Month,
}

impl Range {
    pub const ALL: [Range; 4] = [Range::Hour, Range::Day, Range::Week, Range::Month];

    /// How often this tier accepts a sample.
    pub fn interval(self) -> ChronoDuration {
        match self {
            Range::Hour => ChronoDuration::seconds(5),
            Range::Day => ChronoDuration::seconds(60),
            Range::Week => ChronoDuration::minutes(5),
            Range::Month => ChronoDuration::minutes(30),
        }
    }

    /// How far back it reaches.
    pub fn span(self) -> ChronoDuration {
        match self {
            Range::Hour => ChronoDuration::hours(1),
            Range::Day => ChronoDuration::hours(24),
            Range::Week => ChronoDuration::days(7),
            Range::Month => ChronoDuration::days(30),
        }
    }

    /// The most samples this tier ever holds — span divided by interval, plus
    /// one so a full window is not one short.
    pub fn capacity(self) -> usize {
        (self.span().num_seconds() / self.interval().num_seconds()) as usize + 1
    }

    /// The label the toggle group shows.
    pub fn label(self) -> &'static str {
        match self {
            Range::Hour => "1 h",
            Range::Day => "24 h",
            Range::Week => "7 d",
            Range::Month => "30 d",
        }
    }
}

/// One recorded moment, reduced to the handful of numbers the charts draw.
///
/// Deliberately not the whole [`Utilization`]: storing every field for 30
/// days to draw four lines would be most of a megabyte of JSON nobody reads.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub at: DateTime<Utc>,
    pub cpu_percent: u8,
    pub memory_percent: u8,
    pub network_rx: u64,
    pub network_tx: u64,
    pub disk_utilization: u8,
    pub temperature_c: Option<i64>,
}

impl Sample {
    /// Reduce a live reading. `temperature_c` comes from `SYNO.Core.System`,
    /// a different call, so it is passed in rather than dug out.
    pub fn new(at: DateTime<Utc>, u: &Utilization, temperature_c: Option<i64>) -> Self {
        let net = u.network_total();
        Sample {
            at,
            cpu_percent: u.cpu.total(),
            memory_percent: u.memory.usage_percent,
            network_rx: net.map_or(0, |n| n.rx),
            network_tx: net.map_or(0, |n| n.tx),
            disk_utilization: u.disk.map_or(0, |d| d.utilization),
            temperature_c,
        }
    }
}

/// One tier: samples at a fixed cadence, oldest dropped when full.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Series {
    range: Range,
    samples: VecDeque<Sample>,
}

impl Series {
    fn new(range: Range) -> Self {
        Series {
            range,
            samples: VecDeque::new(),
        }
    }

    /// Offer a sample. Accepted only if this tier's interval has passed.
    fn offer(&mut self, sample: Sample) -> bool {
        if let Some(last) = self.samples.back() {
            // The backwards case is tested first, and has to be: a clock that
            // went backwards — a suspend/resume, or an NTP step — produces a
            // negative duration, which compares as "too soon" and returns
            // below. The tier would then reject every sample until the clock
            // caught up, and the chart would silently stop.
            if sample.at < last.at {
                self.samples.clear();
            } else if sample.at.signed_duration_since(last.at) < self.range.interval() {
                return false;
            }
        }
        self.samples.push_back(sample);
        while self.samples.len() > self.range.capacity() {
            self.samples.pop_front();
        }
        true
    }

    /// Samples inside the window ending at `now`.
    fn window(&self, now: DateTime<Utc>) -> Vec<Sample> {
        let cutoff = now - self.range.span();
        self.samples
            .iter()
            .filter(|s| s.at >= cutoff)
            .copied()
            .collect()
    }
}

/// Recorded history for one DiskStation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trends {
    series: Vec<Series>,
}

impl Default for Trends {
    fn default() -> Self {
        Trends {
            series: Range::ALL.iter().map(|r| Series::new(*r)).collect(),
        }
    }
}

impl Trends {
    pub fn new() -> Self {
        Trends::default()
    }

    /// Record a sample into whichever tiers are due for one.
    pub fn record(&mut self, sample: Sample) {
        for series in &mut self.series {
            series.offer(sample);
        }
    }

    /// The samples to draw for a range, oldest first.
    pub fn window(&self, range: Range, now: DateTime<Utc>) -> Vec<Sample> {
        self.series
            .iter()
            .find(|s| s.range == range)
            .map(|s| s.window(now))
            .unwrap_or_default()
    }

    /// Total samples held, across every tier. For diagnostics.
    pub fn len(&self) -> usize {
        self.series.iter().map(|s| s.samples.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Load history from disk.
    ///
    /// A missing file is an empty history, not an error: the first run of the
    /// app has none, and that is the normal case rather than a fault.
    /// A corrupt one is also an empty history — losing a chart is a much
    /// smaller harm than refusing to start.
    pub fn load(path: &Path) -> Trends {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write history to disk, atomically.
    ///
    /// Temp file then rename, so a crash mid-write leaves the previous
    /// history rather than a truncated one.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cpu, Memory, Utilization};

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000 + seconds, 0).expect("valid timestamp")
    }

    fn sample(seconds: i64, cpu: u8) -> Sample {
        Sample {
            at: at(seconds),
            cpu_percent: cpu,
            memory_percent: 10,
            network_rx: 0,
            network_tx: 0,
            disk_utilization: 0,
            temperature_c: Some(42),
        }
    }

    #[test]
    fn a_sample_reduces_a_live_reading() {
        let u = Utilization {
            cpu: Cpu {
                user: 3,
                system: 2,
                other: 1,
                ..Cpu::default()
            },
            memory: Memory {
                usage_percent: 7,
                ..Memory::default()
            },
            ..Utilization::default()
        };
        let s = Sample::new(at(0), &u, Some(42));
        assert_eq!(s.cpu_percent, 6);
        assert_eq!(s.memory_percent, 7);
        assert_eq!(s.temperature_c, Some(42));
    }

    #[test]
    fn the_hour_tier_takes_a_sample_every_five_seconds_and_the_day_tier_does_not() {
        let mut t = Trends::new();
        t.record(sample(0, 10));
        t.record(sample(5, 20));

        // Both land in the 1 h tier; only the first reaches the 24 h tier,
        // which is not due again for a minute.
        assert_eq!(t.window(Range::Hour, at(5)).len(), 2);
        assert_eq!(t.window(Range::Day, at(5)).len(), 1);
    }

    #[test]
    fn a_sample_offered_too_soon_is_dropped_rather_than_stored() {
        let mut t = Trends::new();
        t.record(sample(0, 10));
        t.record(sample(1, 20));
        assert_eq!(t.window(Range::Hour, at(1)).len(), 1);
    }

    #[test]
    fn every_tier_accepts_the_first_sample_so_a_fresh_install_draws_something() {
        let mut t = Trends::new();
        t.record(sample(0, 10));
        for range in Range::ALL {
            assert_eq!(t.window(range, at(0)).len(), 1, "{range:?} should have it");
        }
    }

    #[test]
    fn a_tier_drops_its_oldest_rather_than_growing_without_bound() {
        let mut t = Trends::new();
        let cap = Range::Hour.capacity();
        // Two full windows' worth, at the tier's exact cadence.
        for i in 0..(cap as i64 * 2) {
            t.record(sample(i * 5, 50));
        }
        let held = t.window(Range::Hour, at(cap as i64 * 10));
        assert!(held.len() <= cap, "held {} > capacity {}", held.len(), cap);
    }

    #[test]
    fn the_window_excludes_samples_older_than_the_range() {
        let mut t = Trends::new();
        t.record(sample(0, 10));
        // Two hours later, the sample from the start is outside the 1 h
        // window but still inside the 24 h one.
        let now = at(7200);
        assert!(t.window(Range::Hour, now).is_empty());
        assert_eq!(t.window(Range::Day, now).len(), 1);
    }

    #[test]
    fn samples_come_back_oldest_first_so_a_chart_can_draw_them_in_order() {
        let mut t = Trends::new();
        for i in 0..4 {
            t.record(sample(i * 5, i as u8 * 10));
        }
        let w = t.window(Range::Hour, at(20));
        assert_eq!(w.first().expect("some").cpu_percent, 0);
        assert_eq!(w.last().expect("some").cpu_percent, 30);
    }

    #[test]
    fn a_clock_that_jumps_backwards_restarts_the_window_instead_of_wedging_it() {
        // Suspend and resume, or an NTP step. Without the reset, every later
        // sample compares as "too soon" against a future timestamp and the
        // chart stops updating until the clock catches up.
        let mut t = Trends::new();
        t.record(sample(10_000, 10));
        t.record(sample(0, 20));
        let w = t.window(Range::Hour, at(0));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].cpu_percent, 20);
    }

    #[test]
    fn capacities_are_bounded_enough_to_persist() {
        // The whole reason for tiering: 30 days at the poll interval would be
        // over half a million samples.
        let total: usize = Range::ALL.iter().map(|r| r.capacity()).sum();
        assert!(
            total < 10_000,
            "total capacity {total} is too large to write out"
        );
    }

    #[test]
    fn history_round_trips_through_a_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trends.json");

        let mut t = Trends::new();
        t.record(sample(0, 42));
        t.save(&path).expect("should save");

        let back = Trends::load(&path);
        assert_eq!(back.window(Range::Hour, at(0))[0].cpu_percent, 42);
    }

    #[test]
    fn a_missing_history_file_is_an_empty_history_not_a_failure() {
        // The first run of the app, which is not an error condition.
        assert!(Trends::load(Path::new("/nonexistent/trends.json")).is_empty());
    }

    #[test]
    fn a_corrupt_history_file_costs_the_charts_not_the_app() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trends.json");
        std::fs::write(&path, "{ this is not json").expect("write");
        assert!(Trends::load(&path).is_empty());
    }

    #[test]
    fn saving_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trends.json");
        Trends::new().save(&path).expect("should save");

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?}");
    }
}
