# Client integration

A USAHP client opens a WebSocket to `ws://127.0.0.1:7312`, reads JSON text frames, and treats the first message as the current state snapshot.

## Connection lifecycle

1. Connect to the configured loopback port.
2. Require a `hello` message with `protocol_version` equal to `0.1`.
3. Initialize local state from the complete `switches` snapshot.
4. Apply subsequent `switch_event` messages in sequence order.
5. On disconnection, discard assumptions about missed transitions and reconnect.
6. Replace local state with the next `hello` snapshot; v0 has no replay.

Clients do not send application messages in protocol 0.1.

## JavaScript example

```js
const states = new Map()
const socket = new WebSocket('ws://127.0.0.1:7312')

socket.addEventListener('message', ({ data }) => {
  const message = JSON.parse(data)

  if (message.protocol_version !== '0.1') {
    socket.close(1002, 'unsupported USAHP protocol')
    return
  }

  if (message.type === 'hello') {
    states.clear()
    for (const item of message.switches) {
      states.set(item.switch_id, item.state)
    }
    return
  }

  if (message.type === 'switch_event') {
    states.set(message.switch_id, message.action)
    console.log(message.sequence, message.switch_id, message.action)
  }
})
```

## Rust example

The workspace includes `usahp-listen`, a small reference client using `tokio-tungstenite` and the shared protocol types:

```rust
let (socket, _) = tokio_tungstenite::connect_async(
    "ws://127.0.0.1:7312",
).await?;
let (_, mut incoming) = socket.split();

while let Some(frame) = incoming.next().await {
    let frame = frame?;
    if !frame.is_text() {
        continue;
    }
    let message: ServerMessage = serde_json::from_str(frame.to_text()?)?;
    match message {
        ServerMessage::Hello(snapshot) => initialize(snapshot),
        ServerMessage::SwitchEvent(event) => apply(event),
    }
}
```

Run the supplied client in readable or JSON mode:

```shell
cargo run -p usahp-listen -- ws://127.0.0.1:7312
cargo run -p usahp-listen -- --json ws://127.0.0.1:7312
```

## Timing and policy

Use `monotonic_us` to calculate durations within one daemon process. It is not wall-clock time and resets when the daemon restarts. Use `sequence` to detect a gap in the live connection; reconnect to obtain a fresh snapshot if delivery becomes uncertain.

Keep debounce, hold detection, confidence scoring, and activation policy in the client. USAHP reports edges rather than user intent.
