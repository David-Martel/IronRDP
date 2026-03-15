# IronRDP Session

Abstract state machine to drive an RDP session.

This crate owns the active-session protocol machinery after connection setup is complete.
In practice that means:

- fast-path frame processing and fragmentation reassembly
- bitmap, pointer, and codec decode into `DecodedImage`
- display control, deactivation/reactivation, and graceful shutdown sequencing
- translation between wire PDUs and higher-level session outputs consumed by the native client and server runtimes

The client and server crates now keep transport/bootstrap code outside this crate and use
their own `session_driver.rs` modules to host the live runtime loops. That keeps
`ironrdp-session` focused on protocol/session state instead of windowing, transport, or host-specific integration code.

This crate is part of the [IronRDP] project.

[IronRDP]: https://github.com/Devolutions/IronRDP
