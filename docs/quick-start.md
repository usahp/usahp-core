# Quick start

This walkthrough starts the daemon with the built-in simulator and watches the resulting events in the reference client. It does not require switch hardware.

## Prerequisites

- A current stable [Rust toolchain](https://rustup.rs/)
- Git
- The platform setup described in [platform requirements](/platforms) before using physical input

## 1. Build USAHP

```shell
git clone https://github.com/OwenMcGirr/usahp.git
cd usahp
cargo build --workspace
```

## 2. Review the configuration

The repository includes [`example.toml`](https://github.com/OwenMcGirr/usahp/blob/main/example.toml). Its simulator is enabled and both `Space` and `Return` control `switch_1`.

## 3. Start the daemon

```shell
cargo run -p usahp-daemon --bin usahpd -- --config example.toml
```

The daemon loads only the explicitly supplied TOML file and listens on `ws://127.0.0.1:7312` by default.

## 4. Start a listener

Open a second terminal in the repository:

```shell
cargo run -p usahp-listen -- ws://127.0.0.1:7312
```

The listener immediately prints the protocol version and current switch state.

## 5. Simulate a switch

In the daemon terminal, enter:

```text
press switch_1
release switch_1
```

The listener displays two ordered transitions. Simulator events use the same state machine, sequencing, queues, and WebSocket broadcast path as hardware events.

## Next steps

- Define physical inputs in [configuration](/configuration).
- Connect your own app with the [client integration guide](/clients).
- Read the complete [protocol 0.1 contract](/protocol-v0).
