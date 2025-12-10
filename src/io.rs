//! I/O wrappers for tracking data transfer.
//!
//! This module provides [`ProgressReader`] and [`ProgressWriter`], which wrap any implementation
//! of [`std::io::Read`] or [`std::io::Write`].
//!
//! # mechanics
//!
//! These wrappers act as "pass-through" middleware. Every byte successfully read or written
//! automatically increments the associated [`Progress`] counter. This is particularly
//! useful for:
//!
//! * File downloads/uploads.
//! * Hashing large files.
//! * Compressing/Decompressing data streams.
//!
//! The overhead is minimal: a single atomic addition per `read` or `write` call.

use std::io::{self, Read, Write};

use crate::Progress;

/// A wrapper around [`Read`] that increments a [`Progress`] tracker based on bytes read.
pub struct ProgressReader<R> {
    inner: R,
    progress: Progress,
}

impl<R> ProgressReader<R> {
    /// Creates a new `ProgressReader` wrapping `inner` with the given `progress` tracker.
    pub const fn new(inner: R, progress: Progress) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        // cast is safe: we are just tracking bytes
        self.progress.inc(n as u64);
        Ok(n)
    }
}

/// A wrapper around [`Write`] that increments a [`Progress`] tracker based on bytes written.
pub struct ProgressWriter<W> {
    inner: W,
    progress: Progress,
}

impl<W> ProgressWriter<W> {
    /// Creates a new `ProgressWriter` wrapping `inner` with the given `progress` tracker.
    pub const fn new(inner: W, progress: Progress) -> Self {
        Self { inner, progress }
    }
}

impl<W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.progress.inc(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read as _, Write as _};

    use crate::io::{ProgressReader, ProgressWriter};

    use super::Progress;

    /// Reader Tracking
    /// Verifies bytes read are counted.
    #[test]
    fn test_io_reader() {
        let data = vec![0u8; 100];
        let p = Progress::new_pb("read", 100u64);
        let mut reader = ProgressReader::new(Cursor::new(&data), p.clone());

        let mut buf = [0u8; 10];
        reader.read_exact(&mut buf).unwrap();

        assert_eq!(p.get_pos(), 10);
    }

    /// Writer Tracking
    /// Verifies bytes written are counted.
    #[test]
    fn test_io_writer() {
        let p = Progress::new_pb("write", 50u64);
        let mut writer = ProgressWriter::new(Vec::new(), p.clone());

        writer.write_all(&[1, 2, 3, 4, 5]).unwrap();

        assert_eq!(p.get_pos(), 5);
    }
}
