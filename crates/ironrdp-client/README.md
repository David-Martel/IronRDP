# IronRDP client

Portable RDP client without GPU acceleration.

This is a a full-fledged RDP client based on IronRDP crates suite, and implemented using
non-blocking, asynchronous I/O. Portability is achieved by using softbuffer for rendering
and winit for windowing.

## Internal layout

The native client is split into a few coarse responsibilities:

- `main.rs`: CLI bootstrap, logging, Tokio runtime startup, and top-level app wiring.
- `app.rs`: window creation, initial sizing, resize/DPI handling, rendering, and translation of `winit` events into client input events.
- `rdp.rs`: connection establishment, transport upgrades, channel wiring, and reconnect policy.
- `session_driver.rs`: active-session runtime that drives an established connection and translates
  protocol output into window events.

That split keeps the live session loop separate from connection setup, which makes the runtime easier
to reason about and reduces coupling between transport code and window/rendering code.
The native window now starts at the configured desktop size instead of a hard-coded fallback, which makes
the first connection and resize path more predictable for local demo use.

## Sample usage

```shell
ironrdp-client <HOSTNAME> --username <USERNAME> --password <PASSWORD>
```

For repeatable demo sizing on Windows, you can also request an initial desktop size explicitly:

```shell
ironrdp-client <HOSTNAME> --username <USERNAME> --password <PASSWORD> --width 1600 --height 900
```

If you provide an `.rdp` file, `desktopwidth` and `desktopheight` are now used as the initial
desktop request when explicit CLI sizing is not supplied.

## Configuring log filter directives

The `IRONRDP_LOG` environment variable is used to set the log filter directives. 

```shell
IRONRDP_LOG="info,ironrdp_connector=trace" ironrdp-client <HOSTNAME> --username <USERNAME> --password <PASSWORD>
```

See [`tracing-subscriber`’s documentation][tracing-doc] for more details.

[tracing-doc]: https://docs.rs/tracing-subscriber/0.3.17/tracing_subscriber/filter/struct.EnvFilter.html#directives

## Support for `SSLKEYLOGFILE`

This client supports reading the `SSLKEYLOGFILE` environment variable.
When set, the TLS encryption secrets for the session will be dumped to the file specified
by the environment variable. 
This file can be read by Wireshark so that in can decrypt the packets.

### Example

```shell
SSLKEYLOGFILE=/tmp/tls-secrets ironrdp-client <HOSTNAME> --username <USERNAME> --password <PASSWORD>
```

### Usage in Wireshark

See this [awakecoding's repository][awakecoding-repository] explaining how to use the file in wireshark.

This crate is part of the [IronRDP] project.

[IronRDP]: https://github.com/Devolutions/IronRDP
[awakecoding-repository]: https://github.com/awakecoding/wireshark-rdp#sslkeylogfile
