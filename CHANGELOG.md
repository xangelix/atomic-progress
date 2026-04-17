# Changelog

## v0.1.5

- **Feat(progress):** Implement `std::fmt::Debug` for `Progress`, convenient but a bit dangerous

## v0.1.4

### Added

- **Feat(api):** Re-exported `prettier_bytes::{ByteFormatter, Standard, Unit}` from the crate root when the `terminal` feature is enabled. This prevents a leaky abstraction and eliminates the need for downstream users to manually add `prettier-bytes` as a direct dependency to configure their progress output.

## v0.1.3

### Added

- **Feat(frontends):** Introduced the `frontends` module to encapsulate all rendering logic, heavily reinforcing the library's headless design.
- **Feat(frontends):** Added the `Frontend` trait to establish a standard contract for custom output sinks (e.g., terminal, network, GUI, or structured logs).
- **Feat(frontends):** Implemented `TerminalFrontend` for zero-flicker, in-place progress rendering using standard ANSI escape sequences.
- **Feat(frontends):** Added `Theme` customization for terminal output, providing a modern UTF-8 default alongside an `ascii()` fallback for legacy or CI environments.
- **Feat(frontends):** Integrated optional automatic byte-formatting capabilities via the `prettier-bytes` crate for human-readable position, total, and throughput metrics.

## v0.1.2

- **Chore(progress):** Expose Snapshot fields publicly (somewhat a semver risk)

## v0.1.1

- **Feat(progress):** Add `bump()` shorthand for `inc(1)` on `Progress`

## v0.1.0

- Initial Release
