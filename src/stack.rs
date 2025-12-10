//! A thread-safe collection for managing multiple progress indicators.
//!
//! The [`ProgressStack`] serves as the central registry for "Multi-Bar" applications.
//! It allows a renderer to iterate over a dynamic list of active tasks without fighting
//! the worker threads for locks.
//!
//! # Synchronization Strategy
//!
//! The stack uses a coarse-grained [`RwLock`](parking_lot::RwLock) to protect the *list*
//! of handles. Individual progress updates (incrementing counters) do **not** lock the stack.
//!
//! * **Workers:** Only acquire the stack lock when adding/removing a bar (rare).
//! * **Renderers:** Acquire a read lock on the stack once per frame to clone the handles,
//!   then iterate independently.

use std::{fmt, sync::Arc};

use compact_str::CompactString;
use parking_lot::RwLock;

use crate::{Progress, ProgressSnapshot};

/// A thread-safe, shared-clonable collection of [`Progress`] instances.
///
/// `ProgressStack` is designed to manage a dynamic list of progress indicators
/// (bars, spinners, etc.) that typically render together (e.g., a multi-bar CLI).
///
/// # Concurrency
///
/// This struct uses internal synchronization (via [`Arc`] and [`RwLock`]), making it
/// safe to clone and share across threads. Cloning is cheap (pointer copy), and
/// mutations on one clone are visible to all others.
#[derive(Clone, Default)]
pub struct ProgressStack {
    /// The shared list of progress items.
    ///
    /// We use `parking_lot::RwLock` for efficient, fair locking.
    inner: Arc<RwLock<Vec<Progress>>>,
}

impl fmt::Debug for ProgressStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // We only print metadata to avoid locking the inner items during debug formatting
        f.debug_struct("ProgressStack")
            .field("count", &self.len())
            .finish()
    }
}

impl ProgressStack {
    /// Creates a new, empty `ProgressStack`.
    ///
    /// # Examples
    ///
    /// ```
    /// use atomic_progress::ProgressStack;
    ///
    /// let stack = ProgressStack::new();
    /// assert!(stack.is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a [`Progress`] instance to the stack.
    ///
    /// The progress item is appended to the end of the collection.
    ///
    /// # Parameters
    ///
    /// * `progress`: The progress indicator to add.
    pub fn push(&self, progress: Progress) {
        self.inner.write().push(progress);
    }

    /// Creates a new progress bar, adds it to the stack, and returns the handle.
    ///
    /// This is a shorthand for creating a [`Progress`] via [`Progress::new_pb`]
    /// and calling [`push`](Self::push).
    ///
    /// # Parameters
    ///
    /// * `name`: The display name for the bar.
    /// * `total`: The total count for the bar.
    #[must_use]
    pub fn add_pb(&self, name: impl Into<CompactString>, total: impl Into<u64>) -> Progress {
        let progress = Progress::new_pb(name, total);
        self.push(progress.clone());
        progress
    }

    /// Creates a new spinner, adds it to the stack, and returns the handle.
    ///
    /// This is a shorthand for creating a [`Progress`] via [`Progress::new_spinner`]
    /// and calling [`push`](Self::push).
    ///
    /// # Parameters
    ///
    /// * `name`: The display name for the spinner.
    #[must_use]
    pub fn add_spinner(&self, name: impl Into<CompactString>) -> Progress {
        let progress = Progress::new_spinner(name);
        self.push(progress.clone());
        progress
    }

    /// Returns a snapshot of the state of all tracked progress instances.
    ///
    /// This aggregates the current state of every progress bar in the stack.
    /// It is typically used by renderers to draw the current UI frame.
    ///
    /// # Performance
    ///
    /// This acquires a read lock on the stack collection, and subsequently
    /// acquires read locks on each individual `Progress` state to copy their data.
    #[must_use]
    pub fn snapshot(&self) -> ProgressStackSnapshot {
        // 1. Quick lock just to clone the Arcs
        let items: Vec<Progress> = self.inner.read().clone();

        // 2. Do the heavy lifting (locking individual bars) without holding the stack lock
        let snapshots = items.iter().map(Progress::snapshot).collect();

        ProgressStackSnapshot(snapshots)
    }

    /// Returns a list of handles to all [`Progress`] instances in the stack.
    ///
    /// Since [`Progress`] is a cheap-to-clone handle (`Arc`), this returns
    /// a new vector containing clones of the handles. This allows you to retain
    /// access to the bars even if the stack is cleared or modified later.
    #[must_use]
    pub fn items(&self) -> Vec<Progress> {
        self.inner.read().clone()
    }

    /// Checks if all progress instances in the stack are marked as finished.
    ///
    /// Returns `true` if the stack is empty or if `is_finished()` returns true
    /// for every item.
    #[must_use]
    pub fn is_all_finished(&self) -> bool {
        self.inner.read().iter().all(Progress::is_finished)
    }

    /// Removes all progress instances from the stack.
    ///
    /// This clears the internal collection.
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Returns the number of progress instances currently in the stack.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns `true` if the stack contains no progress instances.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}

/// A snapshot of the state of the entire progress stack at a specific point in time.
///
/// This wrapper allows for future extension of aggregate calculations (e.g. global ETA,
/// total throughput) without changing the return signature.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Eq, PartialEq)))]
pub struct ProgressStackSnapshot(pub Vec<ProgressSnapshot>);

#[cfg(test)]
mod tests {
    use super::ProgressStack;

    /// Stack Management
    /// Verifies adding items and checking aggregate state.
    #[test]
    fn test_stack_operations() {
        let stack = ProgressStack::new();
        assert!(stack.is_empty());

        let _pb = stack.add_pb("bar", 100u64);
        let _sp = stack.add_spinner("spin");

        assert_eq!(stack.len(), 2);
        assert!(!stack.is_all_finished());

        let items = stack.items();
        items[0].finish();
        items[1].finish();

        assert!(stack.is_all_finished());
    }

    /// Snapshot Isolation
    /// Verifies that a snapshot is an owned copy of data at that instant.
    #[test]
    fn test_snapshot_isolation() {
        let stack = ProgressStack::new();
        let pb = stack.add_pb("test", 100u64);

        pb.inc(10u64);
        let snap_1 = stack.snapshot();

        pb.inc(20u64);
        let snap_2 = stack.snapshot();

        assert_eq!(
            snap_1.0[0].position(),
            10,
            "Old snapshot should remain immutable"
        );
        assert_eq!(snap_2.0[0].position(), 30, "New snapshot reflects updates");
    }
}
