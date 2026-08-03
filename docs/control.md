# USAHP Control

USAHP Control is the optional desktop tray host for the USAHP service. It runs the same Rust broker, capture backends, and loopback WebSocket server as `usahpd`, while making current ownership and service controls visible.

## Start the utility

For development, install the workspace dependencies and run:

```shell
npm install
npm run control:dev
```

The dashboard opens on launch. Minimizing or closing the window hides it to the system tray; click the tray icon or choose **Open USAHP Control** to restore it.

## First launch and configuration

On first launch, USAHP Control copies an embedded configuration into the operating system's per-user application data directory, remembers its path, validates it, and starts automatically. The default maps **Space** and **Enter** to `switch_1`; while the service is running those keys are globally captured and suppressed. Use **Change Configuration** to select a different TOML file such as [`example.toml`](https://github.com/usahp/usahp-core/blob/main/example.toml).

The generated file is never overwritten after creation. USAHP Control remembers only the selected file path and reports an error without replacing the file if it later becomes missing or invalid.

Choosing another configuration while a service has already been loaded restarts USAHP Control. This allows platform input hooks and device grabs to be rebuilt cleanly with the new mappings.

## Dashboard

The dashboard reports live state only:

- whether the service is starting, running, stopping, stopped, or in error;
- the selected configuration, loopback WebSocket address, and capture state;
- configured logical switches and their current state;
- anonymous passive listeners;
- managed clients, their app ID and optional PID, requested mode, and acceptance or rejection;
- the currently active managed session.

Connection and request information exists only in memory. It disappears when the client disconnects or USAHP Control exits; there is no audit log.

## Stop, restart, and quit

**Stop Service** releases every pressed logical switch, revokes the managed session, returns capture to the operating system, disconnects clients, and closes the WebSocket listener. The tray utility remains open, and **Start Service** rebinds the listener and reacquires capture from a released state.

When a managed app owns the session, Stop or Quit asks for confirmation before revoking it. **Quit USAHP** performs the same coordinated shutdown and then exits the tray process.

If another process already owns the configured port, startup remains in an error state. USAHP Control never terminates or takes over that process.

## `usahp-control` versus `usahpd`

Use one host at a time:

- `usahp-control` is intended for interactive desktop use and owns the service in its tray process;
- `usahpd` is the headless CLI host and requires an explicit `--config` path.

Both expose the same loopback-only public protocol. `ServiceSupervisor` is the canonical way for a Rust host to own the broker lifecycle; migrating the separate Tauri demo and introducing optional capture transports remain follow-up work in [issue #15](https://github.com/usahp/usahp-core/issues/15). The utility's management snapshots and commands are private in-process Rust APIs and are not available to WebSocket clients.

## Platform notes

The utility needs the same input permissions as the daemon. On macOS it detects missing Accessibility access and shows an explicit **Grant Accessibility Access** action; it never triggers the system prompt without that click. Linux requires access to the configured input devices, and Windows security software may prompt for global input-hook access. Hosted CI can compile the tray application but cannot verify interactive permissions or real hardware suppression.

USAHP Control deliberately accepts loopback addresses only. Possible future capture transports in issue #15 do not relax the current WebSocket security boundary.
