//! Iterator adapters for automatic progress tracking.
//!
//! This module provides the [`ProgressIteratorExt`] trait, which adds helper methods
//! to any Rust [`Iterator`]. This allows you to attach a progress bar to a loop with
//! a single method call.
//!
//! # Heuristics
//!
//! The adapters automatically check [`Iterator::size_hint`]:
//! * If the iterator provides an exact upper bound, a **Bar** is created with that total.
//! * If the bounds are unknown (or `(0, None)`), a **Spinner** is created.
//!
//! # Example
//!
//! ```ignore
//! use atomic_progress::ProgressIteratorExt;
//!
//! // Automatically becomes a Bar because vec.len() is known
//! for item in vec![1, 2, 3].into_iter().progress() {
//!     // ...
//! }
//! ```

use compact_str::CompactString;

use crate::{
    progress::{Progress, ProgressType},
    stack::ProgressStack,
};

/// An iterator adapter that wraps an underlying iterator and tracks progress.
///
/// Increments the progress position on every call to `next()`.
pub struct ProgressIter<I> {
    iter: I,
    progress: Progress,
}

impl<I> ProgressIter<I> {
    /// Creates a new `ProgressIter`.
    ///
    /// Note: This is usually constructed via [`ProgressIteratorExt`] methods.
    pub const fn new(iter: I, progress: Progress) -> Self {
        Self { iter, progress }
    }
}

impl<I: Iterator> Iterator for ProgressIter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.iter.next();

        if item.is_some() {
            // Immediate atomic update for strict accuracy
            self.progress.inc(1u64);
        } else {
            // Iterator exhausted
            self.progress.finish();
        }

        item
    }
}

/// Extension trait to easily attach progress tracking to any Iterator.
pub trait ProgressIteratorExt: Sized {
    /// Wraps the iterator in a [`Progress`].
    ///
    /// Automatically detects if the iterator has a known size (via `size_hint`) to
    /// choose between a Bar or Spinner.
    fn progress(self) -> ProgressIter<Self>;

    /// Wraps the iterator in a [`Progress`] with a specific name.
    fn progress_with_name(self, name: impl Into<CompactString>) -> ProgressIter<Self>;

    /// Wraps the iterator using an existing [`Progress`] instance.
    fn progress_with(self, progress: Progress) -> ProgressIter<Self>;

    /// Creates a new [`Progress`] attached to the given [`ProgressStack`] and wraps the iterator.
    fn progress_in(self, stack: &ProgressStack) -> ProgressIter<Self>;

    /// Internal helper to determine progress type and total from `size_hint`.
    fn type_from_size_hint(&self) -> (ProgressType, u64);
}

impl<I: Iterator> ProgressIteratorExt for I {
    fn progress(self) -> ProgressIter<Self> {
        self.progress_with_name(CompactString::default())
    }

    fn progress_with_name(self, name: impl Into<CompactString>) -> ProgressIter<Self> {
        let (kind, total) = self.type_from_size_hint();
        let progress = Progress::new(kind, name, total);
        ProgressIter::new(self, progress)
    }

    fn progress_with(self, progress: Progress) -> ProgressIter<Self> {
        ProgressIter::new(self, progress)
    }

    fn progress_in(self, stack: &ProgressStack) -> ProgressIter<Self> {
        let (kind, total) = self.type_from_size_hint();
        // Stack methods return the Arc<Progress>, which we pass to the iterator
        let progress = match kind {
            ProgressType::Bar => stack.add_pb(CompactString::default(), total),
            ProgressType::Spinner => stack.add_spinner(CompactString::default()),
        };
        ProgressIter::new(self, progress)
    }

    fn type_from_size_hint(&self) -> (ProgressType, u64) {
        let (lower, upper) = self.size_hint();
        // If the upper bound matches the lower bound, we have an exact size -> Bar.
        // Otherwise -> Spinner.
        match upper {
            Some(u) if u == lower => (ProgressType::Bar, u as u64),
            _ => (ProgressType::Spinner, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProgressIteratorExt as _;

    /// Iterator Integration
    /// Verifies the extension trait correctly wraps and tracks an iterator.
    #[test]
    fn test_iterator_adapter() {
        let data = [1, 2, 3, 4, 5];
        let mut count = 0;

        // Create iterator, wrap in progress, consume it
        let iter = data.iter().progress_with_name("iter_test");
        let progress_handle = iter.progress.clone(); // Clone handle to inspect state

        for _ in iter {
            count += 1;
        }

        assert_eq!(count, 5);
        assert_eq!(progress_handle.get_pos(), 5);
        assert!(
            progress_handle.is_finished(),
            "Iterator exhaustion should finish progress"
        );
        assert_eq!(
            progress_handle.get_total(),
            5,
            "Total should be inferred from Vec len"
        );
    }
}
