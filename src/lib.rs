//! # `atomic_progress`
//!
//! A high-performance, thread-safe, and cloneable progress tracking library.
//!
//! `atomic_progress` provides primitives for tracking the state of long-running operations.
//! It is designed to be:
//!
//! * **Headless**: It stores state (position, total, time) but does not enforce a specific rendering implementation.
//! * **Concurrent**: Progress handles are cheap to clone ([`Arc`]-based) and safe to share across threads.
//! * **Low Overhead**: Uses atomic primitives for hot-path updates (incrementing position) and coarse-grained locking ([`RwLock`]) for cold paths (metadata, snapshots).
//!
//! ## Modules
//!
//! * [`builder`]: Fluent interface for constructing complex [`Progress`] instances.
//! * [`io`]: Wrappers for [`std::io::Read`] and [`std::io::Write`] that track progress automatically.
//! * [`iter`]: Extension traits for tracking progress on Iterators.
//! * [`progress`]: The core [`Progress`] state machine and snapshot logic.
//! * [`stack`]: A collection for managing multiple progress indicators simultaneously.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod builder;
pub mod io;
pub mod iter;
pub mod progress;
pub mod stack;

pub use builder::ProgressBuilder;
pub use iter::{ProgressIter, ProgressIteratorExt};
pub use progress::{Progress, ProgressSnapshot, ProgressType};
pub use stack::{ProgressStack, ProgressStackSnapshot};
