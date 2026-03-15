# IronRDP project automation

Free-form automation following [`cargo xtask`](https://github.com/matklad/cargo-xtask) specification.

For this Windows-focused fork, `cargo xtask ...` remains the portable repo
automation surface, while [`build.ps1`](../build.ps1) is the optimized local
build and deployment entrypoint. Use `xtask` for repo-agnostic validation and
CI parity; use `build.ps1` when you want CargoTools-managed `sccache`, linker,
job-count, artifact publishing, and machine-aware deployment metadata.
