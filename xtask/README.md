# IronRDP project automation

Free-form automation following [`cargo xtask`](https://github.com/matklad/cargo-xtask) specification.

For this Windows-focused fork, `cargo xtask ...` remains the portable repo
automation surface, while [`build.ps1`](../build.ps1) is the optimized local
build and deployment entrypoint. Use `xtask` for repo-agnostic validation and
CI parity; use `build.ps1` when you want CargoTools-managed `sccache`, linker,
job-count, artifact publishing, and machine-aware deployment metadata.

For local Windows validation, prefer:

```pwsh
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode test -UseNextest
```

That path keeps the test run aligned with the same CargoTools-managed machine
configuration used for packaging and deployment.

When using `build.ps1` on a new Windows machine, keep the `stable` Rust toolchain
updated with `rustup update stable`. CargoTools' wrapper currently executes
wrapped builds through `rustup run stable cargo`, so drift between the `stable`
alias and the repo's pinned toolchain will break the optimized path even if a
newer explicit toolchain is installed locally.
