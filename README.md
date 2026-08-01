# USAHP

USAHP v0 is a local, cross-platform switch-event broker. A foreground Rust daemon suppresses configured switch inputs, normalizes them to logical `pressed` and `released` edges, and broadcasts those edges to every connected local application over WebSocket.

This repository deliberately implements the narrow event-broker layer. It does **not** claim OS accessibility ownership, hand input between the OS and an app, select one active client, interpret holds, or make activation decisions.

## Status

Experimental v0. The public wire protocol is version `0.1` and has no compatibility promise yet.

## Quick start

Install a current stable [Rust toolchain](https://rustup.rs/), then build the workspace:

```shell
cargo build --workspace
```

Copy [`example.toml`](example.toml), adjust its mappings, and start the daemon:

```shell
cargo run -p usahp-daemon --bin usahpd -- --config example.toml
```

In a second terminal, run the reference listener:

```shell
cargo run -p usahp-listen -- ws://127.0.0.1:7312
```

When `simulator.stdin = true`, enter commands in the daemon terminal:

```text
press switch_1
release switch_1
```

Simulator events pass through the same logical state machine and broadcast path as hardware events.

## Configuration

Every mapping has a unique physical `id`, a stable logical `switch_id`, an input type, and a code. Multiple mappings may share a `switch_id`; the daemon emits `pressed` on the first physical press and `released` after the last physical release.

Supported keyboard codes are `Space`, `Return`, `Escape`, `Tab`, the four arrow keys, and `A` through `Z`. Keyboard mappings are globally grabbed and suppressed. Unmapped keyboard input is returned to the OS.

Linux additionally supports gamepad and dedicated switch interfaces through `evdev`. Set `device` to an `/dev/input/event*` path and `code` to a numeric evdev key code. The backend exclusively grabs the complete device; unmatched inputs from that device are therefore also unavailable to other applications. Gamepad mappings are rejected on Windows and macOS because v0 cannot guarantee suppression there.

## Platform permissions

- **Windows:** global keyboard suppression uses a low-level input hook. Security software may require approval.
- **macOS:** grant Accessibility permission to the terminal or executable running `usahpd`.
- **Linux:** keyboard grabbing requires access to input devices, commonly through the `input` or `plugdev` group. Linux gamepads require read access to the selected evdev path. Avoid running as root when a narrower device permission is possible.

The daemon binds only to `127.0.0.1`, does not accept commands over WebSocket, and has no authentication or TLS in v0.

## Protocol

Clients receive a versioned JSON hello snapshot immediately after connecting, followed by ordered switch events. See [`docs/protocol-v0.md`](docs/protocol-v0.md) for the complete v0 contract.

## Development

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

GitHub Actions runs these checks on Windows, macOS, and Linux. Physical input suppression still requires manual hardware testing because hosted CI runners cannot provide global input devices or desktop accessibility permissions.

## License

[MIT](LICENSE)

