# Devolutions.IronRdp

Windows .NET bindings for the IronRDP native library.

This package ships the managed Diplomat bindings plus the `win-x64` native
runtime asset (`DevolutionsIronRdp.dll`) used by the Windows-focused
David-Martel fork of IronRDP.

## Build

From the repository root:

```powershell
pwsh -NoLogo -NoProfile -File .\build.ps1 -Mode ffi -Release
```

That flow builds the Rust FFI library, regenerates the .NET bindings, copies
the native DLL into the package runtime layout, and builds the .NET package.

## Notes

- The package targets Windows deployment scenarios.
- The generated C# bindings under `Generated/` are produced by `diplomat-tool`.
