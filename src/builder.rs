//! Fluent interface for constructing [`Progress`] instances.
//!
//! While simple progress bars can be created via [`Progress::new_pb`] or [`Progress::new_spinner`],
//! the [`ProgressBuilder`] allows for complex initialization scenarios.
//!
//! # Key Features
//!
//! * **Shared State:** You can inject existing `Arc<Atomic...>` instances. This is vital
//!   for scenarios where multiple distinct parts of an application need to update or view
//!   the *exact same* underlying counter (e.g., a global download byte counter shared by
//!   multiple workers), or you need to integrate with a foreign crate/system.
//! * **Time Travel:** Allows explicitly setting the `start` time, useful for resuming
//!   previously paused tasks or synchronizing start times across a batch of jobs.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};

use compact_str::CompactString;
use parking_lot::RwLock;
use web_time::Instant;

use crate::progress::{Cold, Progress, ProgressType};

/// A builder pattern for constructing complex [`Progress`] instances.
///
/// Use this when you need fine-grained control over initialization, such as setting
/// a specific start time, or injecting existing `Arc<Atomic...>` instances for shared
/// state management.
#[derive(Default)]
pub struct ProgressBuilder {
    kind: ProgressType,
    name: CompactString,
    start: Option<Instant>,
    total: u64,
    // Optional shared atomic references
    atomic_pos: Option<Arc<AtomicU64>>,
    atomic_total: Option<Arc<AtomicU64>>,
    atomic_finished: Option<Arc<AtomicBool>>,
}

impl ProgressBuilder {
    /// Starts building a generic Progress Bar (known total).
    #[must_use]
    pub fn new_bar(name: impl Into<CompactString>, total: impl Into<u64>) -> Self {
        Self {
            kind: ProgressType::Bar,
            name: name.into(),
            total: total.into(),
            ..Default::default()
        }
    }

    /// Starts building a Spinner (indeterminate total).
    #[must_use]
    pub fn new_spinner(name: impl Into<CompactString>) -> Self {
        Self {
            kind: ProgressType::Spinner,
            name: name.into(),
            ..Default::default()
        }
    }

    /// Sets a pre-existing atomic counter for position.
    ///
    /// This allows multiple `Progress` instances or external systems to share/update
    /// the same position state.
    #[must_use]
    pub fn with_atomic_pos(mut self, atomic_pos: Arc<AtomicU64>) -> Self {
        self.atomic_pos = Some(atomic_pos);
        self
    }

    /// Sets a pre-existing atomic counter for the total.
    #[must_use]
    pub fn with_atomic_total(mut self, atomic_total: Arc<AtomicU64>) -> Self {
        self.atomic_total = Some(atomic_total);
        self
    }

    /// Sets a pre-existing atomic boolean for the finished state.
    #[must_use]
    pub fn with_atomic_finished(mut self, atomic_finished: Arc<AtomicBool>) -> Self {
        self.atomic_finished = Some(atomic_finished);
        self
    }

    /// Sets the start time explicitly.
    #[must_use]
    pub const fn with_start_time(mut self, start: Instant) -> Self {
        self.start = Some(start);
        self
    }

    /// Sets the start time to `Instant::now()`.
    #[must_use]
    pub fn with_start_time_now(self) -> Self {
        self.with_start_time(Instant::now())
    }

    /// Consumes the builder and returns the constructed [`Progress`] instance.
    #[must_use]
    pub fn build(self) -> Progress {
        Progress {
            kind: self.kind,
            start: self.start,
            cold: Arc::new(RwLock::new(Cold {
                name: self.name,
                stopped: None,
                error: None,
            })),
            item: Arc::new(RwLock::new(CompactString::default())),
            // Use provided atomics or create new ones
            position: self
                .atomic_pos
                .unwrap_or_else(|| Arc::new(AtomicU64::new(0))),
            total: self
                .atomic_total
                .unwrap_or_else(|| Arc::new(AtomicU64::new(self.total))),
            finished: self
                .atomic_finished
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicU64};

    use super::ProgressBuilder;

    /// Shared State Injection
    /// Creates two Progress handles that point to the SAME atomic counter.
    /// This is a critical feature for distributed/shared progress tracking.
    #[test]
    fn test_shared_atomics() {
        let shared_pos = Arc::new(AtomicU64::new(0));

        let p1 = ProgressBuilder::new_bar("worker_1", 100u64)
            .with_atomic_pos(shared_pos.clone())
            .build();

        let p2 = ProgressBuilder::new_bar("worker_2", 100u64)
            .with_atomic_pos(shared_pos) // Same Arc
            .build();

        // Worker 1 updates progress
        p1.inc(10u64);

        // Worker 2 sees the update immediately (atomic load)
        assert_eq!(
            p2.get_pos(),
            10,
            "p2 should see p1's updates via shared atomic"
        );
    }
}
