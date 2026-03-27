# IronRDP FFI

[Diplomat]-based FFI for IronRDP.

Currently, only the .NET target is officially supported.

## How to build

- Install required tools: `cargo xtask ffi install`
  - For .NET, note that `dotnet` is also a requirement that you will need to install on your own.

- Build the shared library: `cargo xtask ffi build` (alternatively, in release mode: `cargo xtask ffi build --release`)

- Build the bindings: `cargo xtask ffi bindings`
  - To regenerate the bindings without rebuilding the .NET project, use `cargo xtask ffi bindings --skip-dotnet-build`
  - For the Windows-focused end-to-end path used in this fork, prefer `pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode ffi -Release`

At this point, you may build and run the examples for .NET:

- `dotnet run --project Devolutions.IronRdp.ConnectExample`
- `dotnet run --project Devolutions.IronRdp.AvaloniaExample`

The `ffi/dotnet/NuGet.Config` file pins restore to `nuget.org` plus the standard
Visual Studio offline cache so local machine-specific package sources do not
break the Windows demo builds.

[Diplomat]: https://github.com/rust-diplomat/diplomat
