# Configuration

`usahpd` requires a TOML file supplied with `--config`. Unknown fields are rejected so spelling mistakes and unsupported options fail early.

## Complete example

```toml
[server]
port = 7312
client_queue_capacity = 256

[simulator]
stdin = true

[[mappings]]
id = "space"
switch_id = "switch_1"
input = "keyboard"
code = "Space"

[[mappings]]
id = "enter"
switch_id = "switch_1"
input = "keyboard"
code = "Return"
```

Both physical mappings control `switch_1`. Holding either one keeps the logical switch pressed; the logical release occurs only after both are released.

## Server

| Field | Default | Meaning |
| --- | --- | --- |
| `port` | `7312` | TCP port on IPv4 loopback (`127.0.0.1`). |
| `client_queue_capacity` | `256` | Maximum pending messages per client before disconnection. Must be greater than zero. |

The host address is not configurable in v0 and cannot be exposed remotely.

## Simulator

Set `stdin = true` to accept `press <switch_id>` and `release <switch_id>` commands in the daemon terminal. The default is `false`.

## Mapping fields

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | Yes | Unique physical mapping identifier. |
| `switch_id` | Yes | Stable logical switch identifier exposed to clients. |
| `input` | Yes | `keyboard` or `gamepad`. |
| `code` | Yes | Keyboard name or numeric Linux evdev key code. |
| `device` | Gamepad only | Linux `/dev/input/event*` device path. Must not be set for keyboard mappings. |

`switch_id` and `code` cannot be empty, at least one mapping is required, and mapping IDs cannot repeat.

## Keyboard codes

Codes are case-insensitive. Supported names are:

- `Space`, `Return` or `Enter`, `Escape` or `Esc`, and `Tab`
- `Up`/`UpArrow`, `Down`/`DownArrow`, `Left`/`LeftArrow`, and `Right`/`RightArrow`
- letters `A` through `Z`

Configured keys are globally suppressed. Unmapped keyboard input is returned to the operating system.

## Linux gamepad or switch device

```toml
[[mappings]]
id = "gamepad-south"
switch_id = "switch_2"
input = "gamepad"
code = "304"
device = "/dev/input/event5"
```

The code is a numeric evdev key code. USAHP grabs the complete device exclusively, so unmatched inputs from that device are also unavailable to other applications. Gamepad mappings are rejected on Windows and macOS.

::: warning Choose the device carefully
Do not exclusively grab a general-purpose controller that another application must continue to use.
:::
