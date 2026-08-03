# Client integration

Connect to `ws://127.0.0.1:7312` and treat the first `hello` as the complete current state. Protocol `0.2` supports passive listeners and one managed session.

## Protocol overview

All frames are UTF-8 JSON text over WebSocket. No authentication — the broker binds to loopback only.

**Server → Client:**

| Frame | When | Key fields |
| --- | --- | --- |
| `hello` | Immediately on connect | `switches: [{ switch_id, state }]` |
| `switch_event` | Every switch edge | `switch_id`, `action` (`pressed`/`released`), `confidence` (0.0–100.0), `sequence` |
| `handshake_response` | After a client handshake | `status` (`ACCEPTED`/`REJECTED`), `session_id`, `heartbeat_interval_ms` |
| `session_revoked` | Session ended (timeout, focus loss, superseded) | `reason` |

**Client → Server (optional, for managed sessions):**

| Frame | Purpose |
| --- | --- |
| `handshake` | Request exclusive switch control |
| `heartbeat` | Keep the session alive |

A passive listener sends nothing — it receives broadcasts while no managed session is active.

---

## JavaScript / TypeScript (web + Node)

Install the [`switch-input`](https://www.npmjs.com/package/switch-input) npm package for a full adapter with reconnection, gesture detection (tap/hold/repeat), and tremor filtering:

```ts
import { GestureEngine, UsahpAdapter, connectToScanner } from 'switch-input';

const engine = new GestureEngine({ tapWindowMs: 250, holdThresholdMs: 1000 });
const adapter = new UsahpAdapter(engine, {
  onStatus: (s) => console.log('broker:', s),
  onSwitches: (ids) => console.log('switches:', ids),
});
// engine.on('tap', (e) => handleTap(e.switchId));
// adapter.detach() when done.
```

Or connect raw — no dependencies:

```js
const socket = new WebSocket('ws://127.0.0.1:7312');
socket.addEventListener('message', ({ data }) => {
  const msg = JSON.parse(data);
  if (msg.type === 'hello') {
    console.log('switches:', msg.switches);
  } else if (msg.type === 'switch_event') {
    console.log(msg.switch_id, msg.action, msg.confidence);
  }
});
```

---

## C# / .NET

Uses `System.Net.WebSockets.ClientWebSocket`. No external packages.

```csharp
using System.Net.WebSockets;
using System.Text;
using System.Text.Json;

using var ws = new ClientWebSocket();
await ws.ConnectAsync(new Uri("ws://127.0.0.1:7312"), CancellationToken.None);

var buffer = new byte[4096];
while (ws.State == WebSocketState.Open)
{
    var result = await ws.ReceiveAsync(buffer, CancellationToken.None);
    if (result.MessageType != WebSocketMessageType.Text) continue;

    var json = Encoding.UTF8.GetString(buffer, 0, result.Count);
    using var doc = JsonDocument.Parse(json);
    var type = doc.RootElement.GetProperty("type").GetString();

    switch (type)
    {
        case "hello":
            foreach (var sw in doc.RootElement.GetProperty("switches").EnumerateArray())
                Console.WriteLine($"  {sw.GetProperty("switch_id")}: {sw.GetProperty("state")}");
            break;
        case "switch_event":
            var switchId = doc.RootElement.GetProperty("switch_id").GetString();
            var action = doc.RootElement.GetProperty("action").GetString();
            Console.WriteLine($"{switchId} {action}");
            // Map to your scanning logic: if (switchId == "switch_1" && action == "pressed") Select();
            break;
    }
}
```

---

## Swift (macOS / iOS)

Uses `URLSessionWebSocketTask`. Works on macOS 10.15+ and iOS 13+.

```swift
import Foundation

struct SwitchMessage: Codable {
    let type: String
    let switchId: String?      // snake_case in JSON → camelCase in Swift
    let action: String?
    let confidence: Float?
    let switches: [SwitchSnapshot]?
}

struct SwitchSnapshot: Codable {
    let switchId: String
    let state: String
}

let task = URLSession.shared.webSocketTask(with: URL(string: "ws://127.0.0.1:7312")!)
task.resume()

func receive() {
    task.receive { result in
        switch result {
        case .success(.string(let text)):
            if let data = text.data(using: .utf8),
               let msg = try? JSONDecoder().decode(SwitchMessage.self, from: data) {
                switch msg.type {
                case "hello":
                    print("switches:", msg.switches ?? [])
                case "switch_event":
                    print("\(msg.switchId ?? "?") \(msg.action ?? "?")")
                    // Map to your scanner: if msg.switchId == "switch_1" { select() }
                default:
                    break
                }
            }
            receive()  // loop
        default:
            break
        }
    }
}
receive()
```

> **iOS note:** USAHP runs on desktop (macOS/Windows/Linux). On iOS, a broker cannot run locally — use the [hardware proxy](/spec#8-addendum-ios-ipados--implementation-prototyping) (BLE device) or connect to a remote broker on your network.

---

## Python

Uses the [`websockets`](https://pypi.org/project/websockets/) library.

```python
import asyncio
import json
import websockets

async def main():
    async with websockets.connect("ws://127.0.0.1:7312") as ws:
        async for raw in ws:
            msg = json.loads(raw)
            if msg["type"] == "hello":
                for sw in msg["switches"]:
                    print(f"  {sw['switch_id']}: {sw['state']}")
            elif msg["type"] == "switch_event":
                switch_id = msg["switch_id"]
                action = msg["action"]
                confidence = msg.get("confidence", 100.0)
                print(f"{switch_id} {action} ({confidence:.1f}%)")
                # Map to your logic:
                # if switch_id == "switch_1" and action == "pressed":
                #     select()

asyncio.run(main())
```

---

## C++

Uses [boost::beast](https://www.boost.org/doc/libs/release/libs/beast/) for WebSocket and [nlohmann/json](https://github.com/nlohmann/json) for parsing.

```cpp
#include <boost/beast/websocket.hpp>
#include <boost/asio/connect.hpp>
#include <boost/asio/ip/tcp.hpp>
#include <nlohmann/json.hpp>
#include <iostream>

namespace beast = boost::beast;
namespace websocket = beast::websocket;
namespace net = boost::asio;
using tcp = net::ip::tcp;
using json = nlohmann::json;

int main() {
    net::io_context ioc;
    tcp::resolver resolver(ioc);
    websocket::stream<tcp::socket> ws(ioc);
    auto const results = resolver.resolve("127.0.0.1", "7312");
    net::connect(ws.next_layer(), results);
    ws.handshake("127.0.0.1", "/");

    beast::flat_buffer buffer;
    while (true) {
        buffer.clear();
        ws.read(buffer);
        auto msg = json::parse(beast::make_printable(buffer.data()));

        if (msg["type"] == "switch_event") {
            std::cout << msg["switch_id"] << " " << msg["action"]
                      << " " << msg.value("confidence", 100.0) << std::endl;
            // if (msg["switch_id"] == "switch_1" && msg["action"] == "pressed") select();
        }
    }
}
```

---

## Rust

The workspace includes `usahp-listen`, a reference client. For your own app, use `tokio-tungstenite` + `usahp-core` types:

```rust
use futures_util::StreamExt;
use tokio_tungstenite::connect_async;

#[tokio::main]
async fn main() {
    let (socket, _) = connect_async("ws://127.0.0.1:7312").await.unwrap();
    let (_, mut incoming) = socket.split();

    while let Some(msg) = incoming.next().await {
        let text = msg.unwrap().into_text().unwrap();
        if let Ok(parsed) = serde_json::from_str::<usahp_core::ServerMessage>(&text) {
            match parsed {
                usahp_core::ServerMessage::SwitchEvent(ev) => {
                    let conf = ev.confidence.map(|c| format!("{c:.1}%")).unwrap_or_else(|| "—".into());
                    println!("{} {:?} ({conf})", ev.switch_id, ev.action);
                }
                _ => {}
            }
        }
    }
}
```

Or run the reference listener:

```shell
cargo run -p usahp-listen -- ws://127.0.0.1:7312
cargo run -p usahp-listen -- --json ws://127.0.0.1:7312
```

---

## Managed sessions (optional)

Clients that need exclusive switch control send a handshake after `hello`. The broker routes events only to the session holder and monitors a heartbeat for safety.

```js
// After hello:
socket.send(JSON.stringify({
  type: 'handshake',
  protocol_version: '0.3.0',
  app_id: 'org.example.switch-app',
  requested_mode: 'exclusive_foreground',
  pid: process.pid  // optional, enables focus-driven revocation
}));

// On ACCEPTED (with session_id), send heartbeats at the reported interval:
socket.send(JSON.stringify({ type: 'heartbeat', session_id }));
```

On `session_revoked`, stop acting on input and reconnect or request a new session.

Passive listeners (no handshake) receive broadcasts while no managed session is active — no code changes needed.

---

## Key principles

- **Edges, not clicks.** USAHP sends discrete `pressed` and `released` events. Your client decides how to interpret them (leading-edge activation, trailing-edge activation, hold detection).
- **Confidence is for analog.** Binary switches always send 100.0 (pressed) / 0.0 (released). Analog sources (BCI, facial gesture, pressure) send intermediate values. Your client can threshold these however it wants.
- **No replay.** If you disconnect and reconnect, you get a fresh `hello` snapshot. There is no event replay. Use `sequence` to detect gaps.
- **Keep debounce, hold detection, and activation policy in the client.** USAHP reports edges, not intent. The `switch-input` npm package handles all of this with a `GestureEngine`.
