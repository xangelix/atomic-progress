//! Renderers and traits for visualizing progress.
//!
//! This module provides the [`Frontend`] trait, allowing you to implement custom
//! display logic for your progress trackers.

#[cfg(feature = "terminal")]
pub mod terminal;

use std::io;

use super::{ProgressSnapshot, ProgressStackSnapshot};

/// A trait for rendering progress state.
///
/// By implementing this trait, you can direct progress updates to any output
/// mechanism: a terminal, a GUI framework, a log file, or a network socket.
pub trait Frontend {
    /// Renders a single progress snapshot.
    fn render(&mut self, snapshot: &ProgressSnapshot) -> io::Result<()>;

    /// Renders a snapshot of a progress stack (multiple trackers).
    fn render_stack(&mut self, stack_snapshot: &ProgressStackSnapshot) -> io::Result<()>;

    /// Clears the rendered progress indicators from the screen.
    fn clear(&mut self) -> io::Result<()>;

    /// Signals that progress tracking is finished, allowing the frontend
    /// to clean up or flush its output.
    fn finish(&mut self) -> io::Result<()>;
}
