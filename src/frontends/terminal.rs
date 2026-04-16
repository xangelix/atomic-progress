//! A standard terminal frontend that uses ANSI escape sequences to render
//! progress bars in-place.
//!
//! This module provides the [`TerminalFrontend`] struct, which is responsible for
//! converting a [`ProgressSnapshot`] into a human-readable, visual representation
//! suitable for standard output streams (e.g., `stderr` or `stdout`).

use std::{
    io::{self, Write},
    time::Duration,
};

use prettier_bytes::ByteFormatter;

use crate::{ProgressSnapshot, ProgressStackSnapshot, ProgressType};

/// A theme for customizing the appearance of the [`TerminalFrontend`].
///
/// This struct dictates the characters used to paint the progress indicators.
/// It provides defaults suitable for modern terminals supporting UTF-8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The character used for the filled portion of a progress bar (e.g., '█').
    pub bar_filled: char,
    /// The character used for the empty portion of a progress bar (e.g., '░').
    pub bar_empty: char,
    /// The sequence of characters used to animate spinner frames.
    pub spinner_frames: &'static [char],
}

impl Default for Theme {
    /// Provides a modern, UTF-8 based default theme.
    fn default() -> Self {
        Self::modern()
    }
}

impl Theme {
    /// A simple ASCII-only theme for environments with limited character support.
    ///
    /// Use this in CI environments, basic command prompts, or when UTF-8
    /// support cannot be guaranteed.
    #[must_use]
    pub const fn ascii() -> Self {
        Self {
            bar_filled: '#',
            bar_empty: '-',
            spinner_frames: &['|', '/', '-', '\\'],
        }
    }

    /// Provides a modern, UTF-8 based default theme.
    ///
    /// This theme uses block characters for bars and braille patterns for spinners,
    /// offering a visually rich experience in compatible terminals.
    #[must_use]
    pub const fn modern() -> Self {
        Self {
            bar_filled: '█',
            bar_empty: '░',
            spinner_frames: &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
        }
    }
}

/// Encapsulates pre-formatted string metrics for a given progress snapshot.
///
/// We compute these shared metrics once per frame to avoid duplicate allocations
/// and to simplify the signatures of the specific rendering methods (`format_bar`, `format_spinner`).
struct FormattedMetrics {
    /// The elapsed time formatted as `MM:SS` or `HH:MM:SS`.
    elapsed: String,
    /// The current position, optionally byte-formatted.
    pos: String,
    /// The total target count, optionally byte-formatted.
    total: String,
    /// The current throughput rate per second.
    rate: String,
    /// The estimated time remaining, including prefix formatting.
    eta: String,
}

/// A standard terminal frontend that uses ANSI escape sequences.
///
/// This frontend renders output in-place by issuing cursor-up ANSI commands
/// (`\x1b[{n}A`) and clearing the current line (`\x1b[2K\r`).
pub struct TerminalFrontend<W> {
    /// The underlying writable stream (typically `stderr`).
    writer: W,
    /// Tracks the number of lines rendered in the last frame to move the cursor correctly.
    last_lines: usize,
    /// The visual width of the progress bar component, in characters.
    width: usize,
    /// The visual theme containing the characters used for rendering.
    theme: Theme,
    /// The current animation frame index for spinners.
    spinner_tick: usize,
    /// An optional formatter used to convert raw integers into human-readable byte sizes.
    byte_formatter: Option<ByteFormatter>,
}

impl<W: Write> TerminalFrontend<W> {
    /// Creates a new `TerminalFrontend` wrapping the given stream.
    ///
    /// # Default Configuration
    /// * **Bar Width:** 40 characters.
    /// * **Theme:** Modern UTF-8 defaults.
    /// * **Byte Formatting:** Disabled.
    ///
    /// # Parameters
    /// * `writer`: The I/O stream to render to.
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            last_lines: 0,
            width: 40,
            theme: Theme {
                bar_filled: '█',
                bar_empty: '░',
                spinner_frames: &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            },
            spinner_tick: 0,
            byte_formatter: None,
        }
    }

    /// Customizes the visual theme.
    #[must_use]
    pub const fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Sets the width of the progress bar (in characters).
    #[must_use]
    pub const fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// Enables automatic byte formatting for position, total, and throughput
    /// using the provided `prettier_bytes::ByteFormatter` rules.
    #[must_use]
    pub const fn with_byte_formatting(mut self, formatter: ByteFormatter) -> Self {
        self.byte_formatter = Some(formatter);
        self
    }

    /// Moves the terminal cursor up by `n` lines using ANSI escape sequences.
    ///
    /// # Errors
    /// Returns an [`io::Error`] if the underlying writer fails.
    fn move_cursor_up(&mut self, n: usize) -> io::Result<()> {
        if n > 0 {
            // \x1b[{n}A is the standard ANSI code for cursor up.
            write!(self.writer, "\x1b[{n}A")?;
        }
        Ok(())
    }

    /// Formats a complete progress line based on the snapshot type.
    ///
    /// This delegates to specific formatting routines depending on whether
    /// the progress indicator is a Bar or a Spinner.
    fn format_line(&self, snapshot: &ProgressSnapshot) -> String {
        let metrics = self.format_metrics(snapshot);

        match snapshot.kind {
            ProgressType::Bar => self.format_bar(snapshot, &metrics),
            ProgressType::Spinner => self.format_spinner(snapshot, &metrics),
        }
    }

    /// Computes and formats the shared mathematical metrics for the snapshot.
    ///
    /// We extract this logic to keep the rendering cleanly separated from string allocations.
    fn format_metrics(&self, snapshot: &ProgressSnapshot) -> FormattedMetrics {
        let elapsed = snapshot
            .elapsed
            .map_or_else(|| "--:--".to_string(), format_duration);

        // Pre-calculate human-readable numbers vs bytes.
        let (pos, total) = self.byte_formatter.as_ref().map_or_else(
            || (snapshot.position.to_string(), snapshot.total.to_string()),
            |bf| {
                (
                    bf.format(snapshot.position).to_string(),
                    bf.format(snapshot.total).to_string(),
                )
            },
        );

        let rate_val = snapshot.throughput();
        let rate = if rate_val > 0.0 {
            self.byte_formatter.as_ref().map_or_else(
                || format!("{rate_val:.1}/s"),
                |bf| format!("{}/s", bf.format(rate_val as u64)),
            )
        } else if self.byte_formatter.is_some() {
            // Fallback for byte-formatted speeds when stalled.
            "--.- B/s".to_string()
        } else {
            // Fallback for standard item speeds when stalled.
            "--.-/s".to_string()
        };

        let eta = snapshot.eta().map_or_else(String::new, |eta_val| {
            format!(" | ETA {}", format_duration(eta_val))
        });

        FormattedMetrics {
            elapsed,
            pos,
            total,
            rate,
            eta,
        }
    }

    /// Renders a deterministic progress bar (where the total is known).
    fn format_bar(&self, snapshot: &ProgressSnapshot, metrics: &FormattedMetrics) -> String {
        use std::fmt::Write as _;

        #[allow(clippy::cast_precision_loss)]
        let percent = if snapshot.total == 0 {
            0.0
        } else {
            (snapshot.position as f64 / snapshot.total as f64) * 100.0
        };

        // Ensure we never render >100% or <0% visually.
        let percent = percent.clamp(0.0, 100.0);

        // Determine how many characters of the bar should be "filled".
        #[allow(clippy::cast_precision_loss)]
        let filled_float = (percent / 100.0) * (self.width as f64);

        // Guard against NaN or Infinity from edge-case calculations to prevent panic/overflow.
        let filled = if filled_float.is_nan() || filled_float.is_infinite() || filled_float < 0.0 {
            0
        } else {
            filled_float as usize
        }
        .min(self.width);

        let empty = self.width.saturating_sub(filled);

        let filled_str = self.theme.bar_filled.to_string().repeat(filled);
        let empty_str = self.theme.bar_empty.to_string().repeat(empty);

        // Determine completion or error status icons.
        let status = if snapshot.finished {
            if snapshot.error.is_some() {
                "✖"
            } else {
                "✔"
            }
        } else {
            ""
        };

        // Construct the trailing info string (name, item, errors).
        let mut info = String::new();
        if !snapshot.name.is_empty() {
            info.push_str(&snapshot.name);
        }
        if !snapshot.item.is_empty() {
            if !info.is_empty() {
                info.push(' ');
            }
            let _ = write!(info, "[{}]", snapshot.item);
        }
        if let Some(err) = &snapshot.error {
            if !info.is_empty() {
                info.push(' ');
            }
            let _ = write!(info, "ERROR: {err}");
        }

        format!(
            "{status}{}[{filled_str}{empty_str}] {percent:>5.1}% ({}/{}) | {}{} | {} | {info}",
            if status.is_empty() { "" } else { " " },
            metrics.pos,
            metrics.total,
            metrics.elapsed,
            metrics.eta,
            metrics.rate,
        )
    }

    /// Renders an indeterminate spinner (where the total is unknown).
    fn format_spinner(&self, snapshot: &ProgressSnapshot, metrics: &FormattedMetrics) -> String {
        use std::fmt::Write as _;

        // Select the appropriate frame or status character.
        let frame = if snapshot.finished {
            if snapshot.error.is_some() {
                '✖'
            } else {
                '✔'
            }
        } else if self.theme.spinner_frames.is_empty() {
            ' '
        } else {
            self.theme.spinner_frames[self.spinner_tick % self.theme.spinner_frames.len()]
        };

        let name_prefix = if snapshot.name.is_empty() {
            String::new()
        } else {
            format!("{} ", snapshot.name)
        };

        // Construct the trailing info string.
        let mut info = String::new();
        if !snapshot.item.is_empty() {
            let _ = write!(info, " [{}]", snapshot.item);
        }
        if let Some(err) = &snapshot.error {
            let _ = write!(info, " ERROR: {err}");
        }

        let items_label = if self.byte_formatter.is_some() {
            ""
        } else {
            " items"
        };

        format!(
            "{frame} {name_prefix}{}{items_label} | {} | {}{info}",
            metrics.pos, metrics.elapsed, metrics.rate
        )
    }
}

impl<W: Write> super::Frontend for TerminalFrontend<W> {
    fn render(&mut self, snapshot: &ProgressSnapshot) -> io::Result<()> {
        self.move_cursor_up(self.last_lines)?;

        let line = self.format_line(snapshot);
        // \x1b[2K clears the entire current line. \r returns cursor to column 1.
        writeln!(self.writer, "\x1b[2K\r{line}")?;

        // wrapping_add is strictly used here to prevent panic on very long-running spinners.
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
        self.last_lines = 1;
        self.writer.flush()?;
        Ok(())
    }

    fn render_stack(&mut self, stack: &ProgressStackSnapshot) -> io::Result<()> {
        self.move_cursor_up(self.last_lines)?;

        for snapshot in &stack.0 {
            let line = self.format_line(snapshot);
            writeln!(self.writer, "\x1b[2K\r{line}")?;
        }

        // Clean up "ghost" lines if the stack shrunk since the last render pass.
        if self.last_lines > stack.0.len() {
            let diff = self.last_lines - stack.0.len();
            for _ in 0..diff {
                writeln!(self.writer, "\x1b[2K\r")?;
            }
            self.move_cursor_up(diff)?;
        }

        self.spinner_tick = self.spinner_tick.wrapping_add(1);
        self.last_lines = stack.0.len();
        self.writer.flush()?;
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.move_cursor_up(self.last_lines)?;
        for _ in 0..self.last_lines {
            writeln!(self.writer, "\x1b[2K\r")?;
        }
        self.move_cursor_up(self.last_lines)?;

        self.last_lines = 0;
        self.writer.flush()?;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.last_lines = 0;
        self.writer.flush()?;
        Ok(())
    }
}

/// Helper function to format a `Duration` into `HH:MM:SS` or `MM:SS`.
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use compact_str::CompactString;

    use super::*;
    use crate::{ProgressBuilder, ProgressStack, ProgressType, frontends::Frontend};

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(45)), "00:45");
        assert_eq!(format_duration(Duration::from_secs(125)), "02:05");
        assert_eq!(format_duration(Duration::from_secs(3665)), "01:01:05");
    }

    #[test]
    fn test_terminal_frontend_rendering() {
        let mut buf = Vec::new();
        {
            let mut frontend = TerminalFrontend::new(&mut buf).with_theme(Theme::ascii());

            let snap = ProgressSnapshot {
                name: CompactString::new("Test"),
                kind: ProgressType::Bar,
                position: 50,
                total: 100,
                ..Default::default()
            };

            frontend.render(&snap).unwrap();
        }

        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("[####################--------------------]"));
        assert!(out.contains("50.0%"));
        assert!(out.contains("\x1b[2K\r")); // Verifies standard terminal clears
    }

    /// Real Terminal Output
    /// Ignored by default. Run manually to see the live progress bar in your terminal:
    /// `cargo test test_real_terminal_output -- --ignored --nocapture`
    #[test]
    #[ignore = "Visual test that writes to stderr and sleeps"]
    fn test_real_terminal_output() {
        let stack = ProgressStack::new();

        // Use ProgressBuilder to explicitly start the timers, then push to the stack
        let bar = ProgressBuilder::new_bar("Downloading", 100u64)
            .with_start_time_now()
            .build();
        stack.push(bar.clone());
        let spinner = ProgressBuilder::new_spinner("Processing")
            .with_start_time_now()
            .build();
        stack.push(spinner.clone());

        // Worker thread: updates progress and sleeps between iterations
        let worker = thread::spawn(move || {
            for i in 0..=100 {
                bar.set_pos(i);
                bar.set_item(format!("chunk_{i}.bin"));

                spinner.bump();
                spinner.set_item(format!("tasks: {i}"));

                thread::sleep(Duration::from_millis(30));
            }

            bar.finish_with_item("Complete!");
            spinner.finish_with_item("Done!");
        });

        // Renderer thread (main): writes to stderr and sleeps between frames
        let mut frontend = TerminalFrontend::new(std::io::stderr());

        while !stack.is_all_finished() {
            let snapshot = stack.snapshot();
            frontend.render_stack(&snapshot).unwrap();

            // ~30fps rendering
            thread::sleep(Duration::from_millis(33));
        }

        // Ensure the final 100% state is rendered before exiting
        let final_snapshot = stack.snapshot();
        frontend.render_stack(&final_snapshot).unwrap();
        frontend.finish().unwrap();

        worker.join().unwrap();
    }
}
