# IronRDP Server

Extendable skeleton for implementing custom RDP servers.

For now, it requires the [Tokio runtime](https://tokio.rs/).

---

The server currently supports:

**Security**
 - Enhanced RDP Security with TLS External Security Protocols (TLS 1.2 and TLS 1.3)

**Input**
 - FastPath input events
 - x224 input events and disconnect

**Codecs**
 - bitmap display updates with RDP 6.0 compression

---

Custom logic for your RDP server can be added by implementing these traits:
 - `RdpServerInputHandler` - callbacks used when the server receives input events from a client
 - `RdpServerDisplay`      - notifies the server of display updates

## Internal layout

The Tokio server is now split into a few coarse responsibilities:

- `builder.rs`: typed builder for assembling `RdpServer` instances.
- `server.rs`: listener lifecycle, transport/security bootstrap, and channel registration.
- `session_driver.rs`: accepted-client runtime, including input dispatch, display updates, server events, and deactivation/reactivation handling.
- `display.rs` / `handler.rs`: integration traits for host display and input backends.

That split keeps the long-lived listener/bootstrap path separate from the per-client session state machine, which reduces coupling between connection setup and runtime event handling.
The session driver is also the place where backlog handling, display-update dispatch,
and resize-triggered reactivation are coordinated, so reliability and protocol tests
for those paths should land close to that boundary rather than in the listener/bootstrap code.
The current Windows-native fork enforces a **single-session-at-a-time** contract as an explicit
invariant. When `run_connection` is entered, `RdpServer::active_session` (`Arc<AtomicBool>`) is set
to `true` and cleared on exit via a `SessionGuard` RAII wrapper. Any new inbound TCP connection
that arrives while a session is active is immediately dropped and logged at `INFO` level. This is
intentional behaviour: multi-session support must be designed and validated explicitly before this
invariant can be relaxed.

This crate is part of the [IronRDP] project.

## Echo RTT probes (feature `echo`)

Enable the `echo` feature to use the ECHO dynamic virtual channel (`MS-RDPEECO`) and measure round-trip time.

```rust
use ironrdp_server::RdpServer;

# async fn demo(mut server: RdpServer) -> anyhow::Result<()> {
// Grab and clone the shared handle before moving the server into a task.
let echo = server.echo_handle().clone();

let local = tokio::task::LocalSet::new();
local
	.run_until(async move {
		let server_task = tokio::task::spawn_local(async move { server.run().await });

		{
			echo.send_request(b"ping".to_vec())?;

			for measurement in echo.take_measurements() {
				println!(
					"echo payload size={} rtt={:?}",
					measurement.payload.len(),
					measurement.round_trip_time
				);
			}
		}

		server_task.await??;
		Ok::<(), anyhow::Error>(())
	})
	.await?;
# Ok(()) }
```

`send_request` queues a probe via the server event loop. If no client has opened the ECHO channel yet, the request is dropped.

[IronRDP]: https://github.com/Devolutions/IronRDP
