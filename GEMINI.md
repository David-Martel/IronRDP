# IronRDP Project Context

IronRDP is a collection of Rust crates providing a modular and secure implementation of the Microsoft Remote Desktop Protocol (RDP). It is designed to be highly portable, with a strong emphasis on security through rigorous fuzzing and architectural boundaries.

## Project Structure & Architecture

The project follows a 3-tier architectural model as described in `ARCHITECTURE.md`:

- **Core Tier (`crates/ironrdp-*`)**: Foundational crates (e.g., `ironrdp-pdu`, `ironrdp-core`).
    - **Invariants**: No I/O allowed, must be fuzzed, no platform-dependent code, minimal dependencies, and `no_std` compatible where practical.
    - **Key Traits**: `Decode` and `Encode` for PDU handling.
- **Extra Tier**: High-level libraries and binaries built on top of the core tier.
    - **I/O**: Async (Tokio/Futures) and Blocking abstractions are provided here.
    - **Binaries**: `ironrdp-client` (portable RDP client).
- **Internal Tier**: Tooling, test suites, and fuzzing targets.
    - **Testing**: `ironrdp-testsuite-core` and `ironrdp-testsuite-extra` consolidate integration tests.
- **Community Tier**: Community-maintained extensions (e.g., `ironrdp-server`, `ironrdp-acceptor`).

## Building and Running

The project uses `cargo` and a custom `xtask` automation runner.

### Key Commands

- **Bootstrap Environment**: `cargo xtask bootstrap` (Installs necessary development tools).
- **Run Full CI Suite**: `cargo xtask ci` (Runs fmt, typos, tests, lints, fuzzing, and FFI checks).
- **Run RDP Client**: `cargo run --bin ironrdp-client -- <HOSTNAME> --username <USERNAME> --password <PASSWORD>`
- **Run Screenshot Example**: `cargo run --example=screenshot -- --host <HOSTNAME> --username <USERNAME> --password <PASSWORD> --output out.bmp`
- **Fuzzing**: `cargo xtask fuzz run` (Runs the fuzzer on core PDU targets).
- **Coverage**: `cargo xtask cov report` (Generates a coverage report).

### Windows-Specific Build
For Windows development, a comprehensive PowerShell script is provided:
- `pwsh -File .\build.ps1 -Mode doctor` (Environment check).
- `pwsh -File .\build.ps1 -Mode package -Release` (Build portable artifacts and installers).

## Development Conventions

### Rust Version (MSRV)
- The project is pinned to **Rust 1.94.0** (as specified in `rust-toolchain.toml`).

### Coding Style (`STYLE.md`)
- **PDU Sizes**: Use inline comments for each field when defining sizes (e.g., `1 /* Version */ + 2 /* Length */`).
- **Error Handling**: Use `crate_name::Result` (e.g., `anyhow::Result`). Error messages should be lowercase and without trailing punctuation.
- **Logging**: Use `tracing` with structured fields (e.g., `info!(%server_addr, "Message")`).
- **Invariants**: Document code invariants using `// INVARIANT: <condition>` comments.
- **Helper Functions**: Avoid single-use helper functions; use blocks instead unless `?` or `return` is needed.
- **Operators**: Strongly prefer `<` and `<=` over `>` and `>=` for consistent "number line" readability.

### Testing Practices
- **Snapshot Testing**: Use `expect-test` for structured data comparison.
- **Fixture Testing**: Use `rstest` for generalized inputs.
- **Property Testing**: Use `proptest` for arbitrary input validation.
- **Fuzzing**: Mandatory for all Core tier crates.
- **API Boundaries**: Tests should focus on the public API (tested via `testsuite-*` crates).

## Important Files
- `ARCHITECTURE.md`: Comprehensive guide to system design and invariants.
- `STYLE.md`: Detailed coding and documentation standards.
- `Cargo.toml`: Workspace configuration and project-wide lints.
- `xtask/`: Rust-based automation for CI and development tasks.
- `build.ps1`: Windows-native build and deployment entrypoint.
