# USAHP Event Broker Protocol 0.2

This page is the canonical implemented contract for USAHP protocol version `0.2.0`. The [draft specification](/spec) is aspirational and does not override this page.

## Transport and legacy listeners

USAHP uses JSON text frames over a loopback-only WebSocket, normally `ws://127.0.0.1:7312`. There is no authentication, TLS, remote binding, replay, or subscription API.

Every connection immediately receives a `hello`. A passive client that sends no handshake continues to receive legacy broadcasts until a managed session is accepted. While a managed session exists, physical events are routed only to its connection.

```json
{
  "type": "hello",
  "protocol_version": "0.2.0",
  "switches": [
    { "switch_id": "switch_1", "state": "released" }
  ]
}
```

## Switch events

```json
{
  "type": "switch_event",
  "protocol_version": "0.2.0",
  "sequence": 42,
  "monotonic_us": 1843201,
  "switch_id": "switch_1",
  "action": "pressed",
  "confidence": 100.0
}
```

`sequence` is global and increasing for one daemon process. `monotonic_us` is microseconds since daemon startup, not wall-clock time. Many physical inputs may map to one logical switch: only the first press and final release emit events.

The binary `confidence` values (`100.0` pressed, `0.0` released) are interim. The final units, absence semantics, validation, update frequency, and snapshot model remain open in [issue #6](https://github.com/usahp/usahp-core/issues/6). Clients own thresholds and activation policy.

## Managed session handshake

Only `exclusive_foreground` is implemented. `app_id` must contain 1–128 ASCII letters, digits, `.`, `_`, or `-`.

```json
{
  "type": "handshake",
  "protocol_version": "0.2.0",
  "app_id": "org.example.switch-app",
  "requested_mode": "exclusive_foreground"
}
```

The daemon owns the heartbeat policy. An accepted response contains an opaque random session ID:

```json
{
  "type": "handshake_response",
  "status": "ACCEPTED",
  "protocol_version": "0.2.0",
  "session_id": "3e7372f6-6bc8-4abf-93e2-63f3ed08a453",
  "heartbeat_interval_ms": 500,
  "missed_heartbeat_limit": 3
}
```

A rejection has a separate shape and one of `PROTOCOL_MISMATCH`, `INVALID_APP_ID`, `UNSUPPORTED_MODE`, `SESSION_BUSY`, or `CAPTURE_UNAVAILABLE`:

```json
{
  "type": "handshake_response",
  "status": "REJECTED",
  "protocol_version": "0.2.0",
  "reason": "SESSION_BUSY"
}
```

## Heartbeat and revocation

The accepted client sends its session ID every 500 ms:

```json
{
  "type": "heartbeat",
  "session_id": "3e7372f6-6bc8-4abf-93e2-63f3ed08a453"
}
```

After three missed heartbeats, disconnect, queue overflow, or library-requested revocation, the broker stops capture, emits ordered releases for held logical switches, clears physical state, and pauses. If the connection is usable it receives:

```json
{
  "type": "session_revoked",
  "protocol_version": "0.2.0",
  "session_id": "3e7372f6-6bc8-4abf-93e2-63f3ed08a453",
  "reason": "HEARTBEAT_TIMEOUT"
}
```

While paused, snapshots are released and hardware or simulator input produces no events. A later valid handshake must reacquire every capture backend before acceptance. Windows and macOS hooks pass input through while paused; Linux evdev releases its exclusive grab, drains stale events, and re-grabs before resuming.

## Explicit exclusions

Protocol 0.2 does not provide authentication, remote access, focus detection, arbitration UI, operating-system scanning or takeover, an escape hatch, continuous analog confidence, replay, hold detection, or clinical recovery guarantees.
