//! Core primitives for tracking progress state.
//!
//! This module defines the [`Progress`] struct, which acts as the central handle for
//! updates. It is designed around a "Hot/Cold" split to maximize performance in
//! multi-threaded environments:
//!
//! * **Hot Data:** Position, Total, and Finished state are stored in `Atomic` primitives.
//!   This allows high-frequency updates (e.g., in tight loops) without locking contention.
//! * **Cold Data:** Metadata like names, current items, and error states are guarded by
//!   an [`RwLock`](parking_lot::RwLock). These are accessed less frequently, typically
//!   only by the rendering thread or when significant state changes occur.
//!
//! # Snapshots
//!
//! To render progress safely, use [`Progress::snapshot`] to obtain a [`ProgressSnapshot`].
//! This provides a consistent, immutable view of the progress state at a specific instant,
//! calculating derived metrics like ETA and throughput automatically.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use compact_str::CompactString;
use parking_lot::RwLock;
use web_time::Instant;

/// A thread-safe, cloneable handle to a progress indicator.
///
/// `Progress` separates "hot" data (position, total, finished status) which are stored in
/// atomics for high-performance updates, from "cold" data (names, errors, timing) which are
/// guarded by an [`RwLock`].
///
/// Cloning a `Progress` is cheap (Arc bump) and points to the same underlying state.
#[derive(Clone)]
pub struct Progress {
    /// The type of progress indicator (Bar vs Spinner). Immutable after creation.
    pub(crate) kind: ProgressType,

    /// The instant the progress tracker was created/started.
    pub(crate) start: Option<Instant>,

    /// Infrequently accessed metadata (name, error state, stop time).
    pub(crate) cold: Arc<RwLock<Cold>>,

    /// The current "item" being processed (e.g., filename).
    pub(crate) item: Arc<RwLock<CompactString>>,

    // Atomic fields for wait-free updates on the hot path.
    pub(crate) position: Arc<AtomicU64>,
    pub(crate) total: Arc<AtomicU64>,
    pub(crate) finished: Arc<AtomicBool>,
}

/// "Cold" storage for metadata that changes infrequently.
pub struct Cold {
    pub(crate) name: CompactString,
    pub(crate) stopped: Option<Instant>,
    pub(crate) error: Option<CompactString>,
}

/// Defines the behavior/visualization hint for the progress indicator.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Eq, PartialEq)))]
pub enum ProgressType {
    /// A spinner, used when the total number of items is unknown.
    #[default]
    Spinner,
    /// A progress bar, used when the total is known.
    Bar,
}

impl Progress {
    /// Creates a new `Progress` instance.
    ///
    /// # Parameters
    ///
    /// * `kind`: The type of indicator.
    /// * `name`: A label for the task.
    /// * `total`: The total expected count (use 0 for spinners).
    pub fn new(kind: ProgressType, name: impl Into<CompactString>, total: impl Into<u64>) -> Self {
        Self {
            kind,
            start: None,
            cold: Arc::new(RwLock::new(Cold {
                name: name.into(),
                stopped: None,
                error: None,
            })),
            item: Arc::new(RwLock::new(CompactString::default())),
            position: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(total.into())),
            finished: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a new generic progress bar with a known total.
    #[must_use]
    pub fn new_pb(name: impl Into<CompactString>, total: impl Into<u64>) -> Self {
        Self::new(ProgressType::Bar, name, total)
    }

    /// Creates a new spinner (indeterminate progress).
    #[must_use]
    pub fn new_spinner(name: impl Into<CompactString>) -> Self {
        Self::new(ProgressType::Spinner, name, 0u64)
    }

    // ========================================================================
    // Metadata Accessors
    // ========================================================================

    /// Gets the current name/label of the progress task.
    #[must_use]
    pub fn get_name(&self) -> CompactString {
        self.cold.read().name.clone()
    }

    /// Updates the name/label of the progress task.
    pub fn set_name(&self, name: impl Into<CompactString>) {
        self.cold.write().name = name.into();
    }

    /// Gets the current item description (e.g., currently processing file).
    #[must_use]
    pub fn get_item(&self) -> CompactString {
        self.item.read().clone()
    }

    /// Updates the current item description.
    pub fn set_item(&self, item: impl Into<CompactString>) {
        *self.item.write() = item.into();
    }

    /// Returns the error message, if one occurred.
    #[must_use]
    pub fn get_error(&self) -> Option<CompactString> {
        self.cold.read().error.clone()
    }

    /// Sets (or clears) an error message for this task.
    pub fn set_error(&self, error: Option<impl Into<CompactString>>) {
        let error = error.map(Into::into);
        self.cold.write().error = error;
    }

    // ========================================================================
    // State & Metrics (Hot Path)
    // ========================================================================

    /// Increments the progress position by the specified amount.
    ///
    /// This uses `Ordering::Relaxed` for maximum performance.
    pub fn inc(&self, amount: impl Into<u64>) {
        self.position.fetch_add(amount.into(), Ordering::Relaxed);
    }

    /// Gets the current position.
    #[must_use]
    pub fn get_pos(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// Sets the absolute position.
    pub fn set_pos(&self, pos: u64) {
        self.position.store(pos, Ordering::Relaxed);
    }

    /// Gets the total target count.
    #[must_use]
    pub fn get_total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Updates the total target count.
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// Checks if the task is marked as finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        // Acquire ensures we see any memory writes that happened before the finish flag was set.
        self.finished.load(Ordering::Acquire)
    }

    /// Manually sets the finished state.
    ///
    /// Prefer using [`finish`](Self::finish), [`finish_with_item`](Self::finish_with_item),
    /// or [`finish_with_error`](Self::finish_with_error) to ensure timestamps are recorded.
    pub fn set_finished(&self, finished: bool) {
        self.finished.store(finished, Ordering::Release);
    }

    // ========================================================================
    // Timing & Calculations
    // ========================================================================

    /// Calculates the duration elapsed since creation.
    ///
    /// If the task is finished, this returns the duration between start and finish.
    /// If never started (no start time recorded), returns `None`.
    #[must_use]
    pub fn get_elapsed(&self) -> Option<Duration> {
        let start = self.start?;
        let cold = self.cold.read();

        Some(
            cold.stopped
                .map_or_else(|| start.elapsed(), |stopped| stopped.duration_since(start)),
        )
    }

    /// Returns the current completion percentage (0.0 to 100.0).
    ///
    /// Returns `0.0` if `total` is zero.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn get_percent(&self) -> f64 {
        let pos = self.get_pos() as f64;
        let total = self.get_total() as f64;

        if total == 0.0 {
            0.0
        } else {
            (pos / total) * 100.0
        }
    }

    // ========================================================================
    // Lifecycle Management
    // ========================================================================

    /// Marks the task as finished and records the stop time.
    pub fn finish(&self) {
        if self.start.is_some() {
            self.cold.write().stopped.replace(Instant::now());
        }
        self.set_finished(true);
    }

    /// Sets the current item and marks the task as finished.
    pub fn finish_with_item(&self, item: impl Into<CompactString>) {
        self.set_item(item);
        self.finish(); // Calls set_finished(true) internally
    }

    /// Sets an error message and marks the task as finished.
    pub fn finish_with_error(&self, error: impl Into<CompactString>) {
        self.set_error(Some(error));
        self.finish();
    }

    // ========================================================================
    // Advanced / Internal
    // ========================================================================

    /// Returns a shared reference to the atomic position counter.
    ///
    /// Useful for sharing this specific counter with other systems.
    #[must_use]
    pub fn atomic_pos(&self) -> Arc<AtomicU64> {
        self.position.clone()
    }

    /// Returns a shared reference to the atomic total counter.
    #[must_use]
    pub fn atomic_total(&self) -> Arc<AtomicU64> {
        self.total.clone()
    }

    /// Creates a consistent snapshot of the current state.
    ///
    /// This involves acquiring a read lock on the "cold" data.
    #[must_use]
    pub fn snapshot(&self) -> ProgressSnapshot {
        self.into()
    }
}

/// A plain-data snapshot of a [`Progress`] state at a specific point in time.
///
/// This is typically used for rendering, as it holds owned data and requires no locking
/// to access.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Eq, PartialEq)))]
pub struct ProgressSnapshot {
    kind: ProgressType,

    name: CompactString,
    item: CompactString,

    elapsed: Option<Duration>,

    position: u64,
    total: u64,

    finished: bool,

    error: Option<CompactString>,
}

impl From<&Progress> for ProgressSnapshot {
    fn from(progress: &Progress) -> Self {
        // Lock cold data once
        let cold = progress.cold.read();
        let name = cold.name.clone();
        let error = cold.error.clone();
        drop(cold);

        Self {
            kind: progress.kind,
            name,
            item: progress.item.read().clone(),
            elapsed: progress.get_elapsed(),
            position: progress.position.load(Ordering::Relaxed),
            total: progress.total.load(Ordering::Relaxed),
            finished: progress.finished.load(Ordering::Relaxed),
            error,
        }
    }
}

impl ProgressSnapshot {
    /// Returns the type of progress indicator.
    #[must_use]
    pub const fn kind(&self) -> ProgressType {
        self.kind
    }

    /// Returns the name/label of the progress task.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the current item description.
    #[must_use]
    pub fn item(&self) -> &str {
        &self.item
    }

    /// Returns the elapsed duration.
    #[must_use]
    pub const fn elapsed(&self) -> Option<Duration> {
        self.elapsed
    }

    /// Returns the current position.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.position
    }
    /// Returns the total target count.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Returns whether the task is finished.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.finished
    }

    /// Returns the error message, if any.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Estimates the time remaining (ETA) based on average speed since start.
    ///
    /// Returns `None` if:
    /// * No progress has been made.
    /// * Total is zero.
    /// * Process is finished.
    /// * Elapsed time is effectively zero.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn eta(&self) -> Option<Duration> {
        if self.position == 0 || self.total == 0 || self.finished {
            return None;
        }

        let elapsed = self.elapsed?;
        let secs = elapsed.as_secs_f64();

        // Avoid division by zero or extremely small intervals
        if secs <= 1e-6 {
            return None;
        }

        let rate = self.position as f64 / secs;
        if rate <= 0.0 {
            return None;
        }

        let remaining_items = self.total.saturating_sub(self.position);
        let remaining_secs = remaining_items as f64 / rate;

        Some(Duration::from_secs_f64(remaining_secs))
    }

    /// Calculates the average throughput (items per second) over the entire lifetime.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn throughput(&self) -> f64 {
        if let Some(elapsed) = self.elapsed {
            let secs = elapsed.as_secs_f64();
            if secs > 0.0 {
                return self.position as f64 / secs;
            }
        }
        0.0
    }

    /// Calculates the instantaneous throughput relative to a previous snapshot.
    ///
    /// This is useful for calculating "current speed" (e.g., in the last second).
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn throughput_since(&self, prev: &Self) -> f64 {
        let pos_diff = self.position.saturating_sub(prev.position) as f64;

        let time_diff = match (self.elapsed, prev.elapsed) {
            (Some(curr), Some(old)) => curr.as_secs_f64() - old.as_secs_f64(),
            _ => 0.0,
        };

        if time_diff > 0.0 {
            pos_diff / time_diff
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::Progress;

    /// Basic Lifecycle
    /// Verifies the fundamental state machine: New -> Inc -> Finish.
    #[test]
    #[allow(clippy::float_cmp)]
    fn test_basic_lifecycle() {
        let p = Progress::new_pb("test_job", 100u64);

        assert_eq!(p.get_pos(), 0);
        assert!(!p.is_finished());
        assert_eq!(p.get_percent(), 0.0);

        p.inc(50u64);
        assert_eq!(p.get_pos(), 50);
        assert_eq!(p.get_percent(), 50.0);

        p.finish();
        assert!(p.is_finished());

        // Default constructor does not start the timer; elapsed should be None.
        assert!(p.get_elapsed().is_none());
    }

    /// Concurrency & Atomics
    /// Ensures that high-contention updates from multiple threads are lossless.
    #[test]
    fn test_concurrency_atomics() {
        let p = Progress::new_spinner("concurrent_job");
        let mut handles = vec![];

        // Spawn 10 threads, each incrementing 100 times
        for _ in 0..10 {
            let p_ref = p.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    p_ref.inc(1u64);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(p.get_pos(), 1000, "Atomic updates should be lossless");
    }

    /// Snapshot Metadata
    /// Verifies that "Cold" data (names, errors) propagates to snapshots correctly.
    #[test]
    fn test_snapshot_metadata() {
        let p = Progress::new_pb("initial_name", 100u64);

        // Mutate cold state
        p.set_name("updated_name");
        p.set_item("file_a.txt");
        p.set_error(Some("disk_full"));

        let snap = p.snapshot();

        assert_eq!(snap.name, "updated_name");
        assert_eq!(snap.item, "file_a.txt");
        assert_eq!(snap.error, Some("disk_full".into()));
    }

    /// Throughput & ETA Safety
    /// Verifies mathematical correctness and edge-case safety (NaN/Inf checks).
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_math_safety() {
        let p = Progress::new_pb("math_test", 100u64);
        let snap = p.snapshot();

        // Edge case: No time elapsed, no progress
        assert_eq!(snap.throughput(), 0.0);
        assert!(snap.eta().is_none());

        // We can't easily mock time without dependency injection or sleeping.
        // We settle for verifying that 0 total handles percentage gracefully.
        let p_zero = Progress::new_pb("zero_total", 0u64);
        assert_eq!(p_zero.get_percent(), 0.0);
    }
}
