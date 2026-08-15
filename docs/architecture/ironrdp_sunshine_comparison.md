# Architectural Comparison and Fleet Validation Plan: Sunshine/Moonlight vs. IronRDP

This document compares **Sunshine / Moonlight** with **IronRDP** (`David-Martel/IronRDP`) and defines a reproducible multi-node validation procedure.
It does not claim a host or cross-machine result unless the corresponding command output and artifact are retained with a dated test report.

---

## 1. Architectural Comparison Overview

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│                             SUNSHINE / MOONLIGHT                                 │
│  Objective: Ultra-low latency (1-5ms), high frame-rate (60-120 FPS) screen stream │
├──────────────────────────┬───────────────────────────┬───────────────────────────┤
│ Capture Layer            │ Encode & Color            │ Transport Layer           │
│ • Linux DRM/KMS          │ • Intel VA-API (LP H.264) │ • RTP / UDP Video Stream  │
│ • Wayland PipeWire       │ • NVIDIA NVENC (CUDA)     │ • Reed-Solomon FEC        │
│   DMA-BUF zero-copy      │ • Vulkan Video & AMF      │ • Dynamic Bitrate Control │
└──────────────────────────┴───────────────────────────┴───────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────────────┐
│                                   IRONRDP                                        │
│  Objective: Secure, enterprise multi-channel remote desktop protocol in Rust     │
├──────────────────────────┬───────────────────────────┬───────────────────────────┤
│ Protocol Core            │ Graphics Pipeline         │ Transport & Channels      │
│ • MCS / T.128 / CredSSP  │ • RDPGFX / EGFX           │ • Async Tokio Sessions    │
│ • Dynamic Virtual Chans  │ • AVC420 / AVC444         │ • TCP / RDPShortPath UDP  │
│ • Input & Clipboard      │ • OpenH264 Wrapper        │ • TLS / NLA Security      │
└──────────────────────────┴───────────────────────────┴───────────────────────────┘
```

---

## 2. IronRDP Desktop Test Provider

The server example renders a deterministic desktop-pattern frame for graphics and dirty-rectangle tests.
An operator may instead supply a binary P6 PPM image with `--background-ppm <PATH>`.
The image is parsed and scaled once at server startup, then the cached BGRA frame is reused for every session and stripe update.

The example requires a certificate and private-key path for TLS.
Hybrid CredSSP mode also requires `--user` and the `IRONRDP_SERVER_PASSWORD` runtime environment variable; no default credential is provided.

---

## 3. Multi-Architecture Revalidation Matrix

Record the exact commit, toolchain, command, exit code, and retained log before changing a row from **Not revalidated**.

| Host class | Target | Required command | Current status |
| :--- | :--- | :--- | :--- |
| Windows workstation | `x86_64-pc-windows-msvc` | `cargo check --workspace` through the documented CargoTools wrapper | **Not revalidated** |
| Linux workstation | `x86_64-unknown-linux-gnu` | `cargo check --workspace` | **Not revalidated** |
| DGX Spark | `aarch64-unknown-linux-gnu` | `cargo check --workspace` | **Not revalidated** |

---

## 4. Cross-Machine Validation Procedure

1. Provision the remote server as a user service with a host-managed TLS identity and runtime credential.
2. Start it through `scripts/fleet/start_spark_server.sh <SSH_DESTINATION> <PORT>`; the SSH destination must already have a verified host key.
3. Run the screenshot client from a separate node through the approved network path.
4. Validate the retained image with `tools/validate_rdp_rendering.py`.
5. Store the image, command transcript, toolchain versions, node identifiers, and commit SHA in a dated test artifact directory before documenting a pass.

Do not commit machine-local absolute paths, private-key locations, passwords, or transient `/tmp` evidence as architectural proof.
