# atomic-progress

[![Crates.io](https://img.shields.io/crates/v/atomic-progress)](https://crates.io/crates/atomic-progress)
[![Docs.rs](https://docs.rs/atomic-progress/badge.svg)](https://docs.rs/atomic-progress)
[![License](https://img.shields.io/crates/l/atomic-progress)](https://spdx.org/licenses/MIT)

**A high-performance, thread-safe, headless progress tracking library for Rust.**

`atomic-progress` provides the state management primitives for tracking long-running operations in concurrent applications. Unlike other libraries that couple state tracking with terminal output, `atomic-progress` is **headless**. It stores progress state (position, total, timing) efficiently and lets *you* decide how to render it—whether to a CLI, a GUI, a web interface, or a structured log.

## Key Features

  * **🚀 High Performance:** Uses `AtomicU64` for "hot" path updates (incrementing position), ensuring minimal overhead in tight loops.
  * **🔒 Concurrency Native:** All progress handles are cheap-to-clone `Arc` structs. Share them across threads without fighting the borrow checker.
  * **🎨 Renderer Agnostic:** "Pull-based" architecture. Your worker threads push updates; your UI thread pulls snapshots.
  * **🛠️ Ergonomic Integrations:**
      * **Iterators:** `iter.progress()` extension trait.
      * **I/O:** `ProgressReader` and `ProgressWriter` wrappers.
      * **Multi-Task:** `ProgressStack` for managing dynamic lists of bars.
  * **⏱️ Accurate Timing:** Built-in ETA, throughput, and elapsed time calculations handling edge cases (zero-division, pauses).
  * **🌍 Broad Platform Compatibility:** First-class support for `wasm32` (via `web-time`) and optimisations for `x86_64` intrinsics (Hardware Lock Elision).

## Platforms

`atomic-progress` is designed to run anywhere Rust runs, but it includes specific optimizations and considerations for high-performance environments.

### x86_64 (Linux, macOS, Windows)

On supported x86 hardware, this library leverages **Hardware Lock Elision (HLE)** via `parking_lot`. This allows multiple threads to access the metadata lock speculatively; if no conflicts occur, the lock is elided entirely at the CPU level, resulting in near-zero latency for metadata reads.

### WebAssembly (`wasm32-unknown-unknown`)

This library is fully compatible with WASM environments (browsers, Node.js, Cloudflare Workers). It uses `web-time` to ensure `Instant` and `Duration` calculations work correctly in the browser.

For multi-threaded WASM applications (e.g., using Web Workers with `SharedArrayBuffer`), you should consider compiling with the following target features to ensure atomics map to native WASM instructions rather than software emulation traps:

  * **`+atomics`**: Essential for the `Arc<AtomicU64>` hot path to work across workers.
  * **`+nontrapping-fptoint`**: Recommended for safe float-to-int conversions in ETA/throughput calculations without risking runtime traps on some engines.

```toml
# .cargo/config.toml
[target.wasm32-unknown-unknown]
rustflags = [
  "-C", "target-feature=+atomics,+bulk-memory,+mutable-globals",
  "-C", "target-feature=+nontrapping-fptoint", 
]
```

## Installation

Add the library to your project via Cargo:

```sh
cargo add atomic-progress
```

### Optional Features

You can enable additional features to support serialization, which is useful for sending progress snapshots over a network (e.g., to a dashboard or frontend) or between processes.

  * **`serde`**: Adds `Serialize` and `Deserialize` implementations from the [serde](https://serde.rs/) framework to `ProgressSnapshot` and `ProgressType`.
  * **`rkyv`**: Adds zero-copy serialization support via [rkyv](https://rkyv.org/), ideal for high-performance IPC or shared memory implementations.

To enable these, you can use the `cargo add` command with `--features`, or you can directly add them to your `Cargo.toml`.

## Usage Examples

### 1. The "One-Liner" (Iterators)

The easiest way to track a loop. The iterator adapter automatically detects if it should be a Bar (known size) or a Spinner (unknown size).

```rust
use std::{thread, time::Duration};

use atomic_progress::ProgressIteratorExt;

fn main() {
    let items = vec![1, 2, 3, 4, 5];

    // Automatically wraps the iterator.
    // In a real app, a separate thread would read the progress state and print it.
    for item in items.iter().progress_with_name("Processing Items") {
        thread::sleep(Duration::from_millis(100));
        // Work happens here...
    }
}
```

### 2. Concurrent / Multi-Threaded

Pass a clone of the progress handle to a worker thread.

```rust
use std::thread;

use atomic_progress::{Progress, ProgressType};

fn main() {
    // 1. Create the progress handle
    let progress = Progress::new_pb("Heavy Calculation", 100);

    // 2. Clone it for the worker thread
    let worker_progress = progress.clone();

    thread::spawn(move || {
        for _ in 0..100 {
            // Hot path: strictly atomic increment
            worker_progress.inc(1);
            thread::sleep(std::time::Duration::from_millis(10));
        }
        worker_progress.finish();
    });

    // 3. Render loop (Main thread)
    while !progress.is_finished() {
        let snapshot = progress.snapshot();
        println!(
            "{} [{}/{}] {:.1}%",
            snapshot.name,
            snapshot.position,
            snapshot.total,
            progress.get_percent()
        );
        thread::sleep(std::time::Duration::from_millis(100));
    }
}
```

### 3. Tracking I/O (Downloads/Uploads)

Wrap any `Read` or `Write` implementor to track bytes automatically.

```rust
use std::io::{self, Cursor, Read};

use atomic_progress::{Progress, io::ProgressReader};

fn main() -> io::Result<()> {
    let data = vec![0u8; 1024 * 1024]; // 1MB dummy data
    let total_size = data.len() as u64;
    
    let progress = Progress::new_pb("Downloading", total_size);
    
    // Wrap the source (Cursor, File, TcpStream, etc.)
    let mut source = ProgressReader::new(Cursor::new(data), progress.clone());
    let mut dest = io::sink();

    // Copying automatically updates the progress bar
    io::copy(&mut source, &mut dest)?;
    
    progress.finish();
    println!("Download complete: {:.2} MB/s", progress.snapshot().throughput() / 1_000_000.0);
    Ok(())
}
```

### 4. Managing Multiple Bars (`ProgressStack`)

Use `ProgressStack` when you have a dynamic number of tasks running in parallel-- or you want a simple global/static progress lock shared with your whole binary.

```rust
use std::thread;

use atomic_progress::ProgressStack;

fn main() {
    let stack = ProgressStack::new();

    // Add tasks dynamically
    for i in 0..5 {
        let bar = stack.add_pb(format!("Job #{}", i), 100);
        thread::spawn(move || {
            // ... work ...
            bar.finish();
        });
    }

    // Snapshot the entire stack at once for rendering
    let stack_snapshot = stack.snapshot();
    for snap in stack_snapshot.0 {
        println!("{}: {}%", snap.name, (snap.position as f64 / snap.total as f64) * 100.0);
    }
}
```

## Architecture: The Hot/Cold Split

`atomic-progress` is designed to avoid locking contention on the worker thread.

1.  **Hot Data (Atomics):**

      * Fields: `position`, `total`, `finished`.
      * Mechanism: `std::sync::atomic`.
      * Benefit: Calling `progress.inc(1)` is wait-free. It will never block your worker thread, ensuring your progress bar doesn't slow down your actual processing.

2.  **Cold Data (RwLock):**

      * Fields: `name`, `current_item`, `error`, `start/stop time`.
      * Mechanism: `parking_lot::RwLock`.
      * Benefit: These fields change rarely. We optimize for read-concurrency so the renderer can snapshot them without blocking writers.

## Async Safe?

**Yes.** `atomic-progress` is fully `Send` + `Sync` and can be moved into `tokio::spawn` or other async tasks without issues.

  * **Hot Path (`inc`, `set_pos`):** Uses non-blocking CPU instructions (atomics). This is safe to call directly in async code and will not block the executor.
  * **Cold Path (`set_name`, `snapshot`):** Uses `parking_lot::RwLock`, which is a synchronous lock. However, critical sections are designed to be extremely short (nanoseconds) as they only involve memory copies of string data. You do **not** need to wrap these calls in `spawn_blocking`; they are fast enough to run on the main async threads without causing starvation.
  * **Deadlocks:** The library API never returns a lock guard. All data access returns owned data (e.g., `ProgressSnapshot`). This makes it impossible to accidentally hold a synchronous lock across an `.await` point, eliminating the most common cause of async deadlocks.

## License

This project is licensed under the [MIT License](https://spdx.org/licenses/MIT).
