# How it fits together

USAHP is a **local event broker**. It sits between switch hardware (or synthetic sources) and switch-aware applications. Every app talks to the same WebSocket endpoint and receives the same normalized events.

```
Switch hardware               Synthetic sources
Space, Enter, arrows          inject_switch, CLI, BCI, webcam
        \                       /
         \                     /
          →  usahpd (broker)  ←
                 |
          ws://127.0.0.1:7312
                 |
         +-------+-------+-------+
         |       |       |       |
      Tauri   Grid 3   Web     Custom
      demo    bridge   app     clients
```

## The broker

`usahpd` captures switch hardware globally (keyboard via `rdev` or `CGEventTap`, gamepad via `evdev` on Linux), normalizes every activation to a `switch_event` with `switch_id`, `action` (`pressed` / `released`), and `confidence` (0.0–100.0), then broadcasts to every connected WebSocket client.

The broker also accepts **synthetic events** — any process can inject a `PhysicalEvent` programmatically. This is how the Tauri demo's on-screen buttons work, and how future sources (BCI, facial gesture, virtual switch test tools) will feed in.

## Client types

### Tauri / desktop apps

The [Tauri demo](https://github.com/AACTools/usahp-tauri-demo) embeds the broker in-process and uses the [`switch-input`](https://www.npmjs.com/package/switch-input) npm package's `UsahpAdapter` to turn WebSocket frames into gesture events (tap, hold, repeat) via a `GestureEngine`.

This is the easiest path for new apps: install `scan-engine` + `switch-input` from npm, connect, and drive your scanner.

### Existing AAC software (Grid 3, etc.)

Apps like Smartbox Grid 3 have their own switch input protocols. Grid 3 on Windows listens for `SendNotifyMessageA("Sensory_SwitchInput", ...)` with `lParam` 1 = press, 0 = release.

A **bridge** translates USAHP events into the app's native protocol. The bridge is a tiny program (~50 lines) that:

1. Connects to `ws://127.0.0.1:7312`
2. On `switch_event` with `action: "pressed"` → sends the app's native press
3. On `switch_event` with `action: "released"` → sends the app's native release

For Grid 3 on Windows, the bridge calls `SendNotifyMessageA(HWND_BROADCAST, registeredMessage, 1, 1)` on press and `(..., 0)` on release.

The bridge pattern works for any AAC software with a programmatic switch input:

| Software | Protocol | Bridge sends |
| --- | --- | --- |
| Grid 3 (Windows) | `SendNotifyMessageA("Sensory_SwitchInput")` | Windows message |
| Grid 3 (iPad) | BLE HID keyboard | BLE write |
| Proloquo2Go | iOS Switch Control (HID) | BLE HID report |
| Web apps | Keyboard events | `dispatchEvent(new KeyboardEvent(...))` |
| Custom apps | WebSocket (native USAHP) | Direct — no bridge needed |

### Web apps

Web pages connect directly via `WebSocket` and use the `UsahpAdapter` from the [`switch-input`](https://www.npmjs.com/package/switch-input) npm package. The adapter handles reconnection, `hello`/`switch_event` parsing, and feeds a `GestureEngine` that turns raw edges into semantic gestures (tap, hold, repeat).

```ts
import { UsahpAdapter, GestureEngine } from 'switch-input';

const engine = new GestureEngine();
const adapter = new UsahpAdapter(engine, {
  onStatus: (s) => console.log('broker:', s),
});
```

No bridge needed — web apps are native USAHP clients.

## Sessions (optional layer)

Clients that need exclusive switch control can send a **handshake** to enter a managed session. The broker routes events only to the session holder and monitors a heartbeat for safety. Clients that don't handshake get the default passive broadcast — so adding session support is entirely opt-in.

See the [RFC](/spec) §4–6 for the full session model.

## Synthetic events

The broker accepts `PhysicalEvent { mapping_id, action }` from any source — not just hardware capture. This enables:

- **On-screen switch buttons** (the Tauri demo's `inject_switch` command)
- **Virtual switches** for testing and demos
- **BCI / facial gesture / voice** inputs that feed confidence-bearing events
- **CLI injection** for automated testing

Every source, hardware or synthetic, produces the same `switch_event` format. Clients don't need to know or care where the event came from.
