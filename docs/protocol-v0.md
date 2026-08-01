# USAHP Event Broker Protocol 0.1

## Purpose

Protocol 0.1 lets local applications observe normalized switch edges from one daemon. It is a passive broadcast protocol at the client boundary: every connected client receives the same logical events, while hardware capture and suppression remain daemon responsibilities.

## Transport

- WebSocket over TCP bound to IPv4 loopback only.
- Default URL: `ws://127.0.0.1:7312`.
- Server-to-client text frames containing one complete UTF-8 JSON object.
- Clients do not send application messages in version 0.1.
- No authentication, encryption, remote binding, filtering, or replay.

## Hello

The first message sent after a successful WebSocket handshake is a complete snapshot of configured logical switches:

```json
{
  "type": "hello",
  "protocol_version": "0.1",
  "switches": [
    {"switch_id": "switch_1", "state": "released"}
  ]
}
```

Switches are ordered lexicographically by `switch_id`. A client connecting while a mapped input is held receives `"state": "pressed"`.

## Switch event

```json
{
  "type": "switch_event",
  "protocol_version": "0.1",
  "sequence": 42,
  "monotonic_us": 1843201,
  "switch_id": "switch_1",
  "action": "pressed"
}
```

- `sequence` begins at 1 and increases globally for every emitted logical transition during one daemon process.
- `monotonic_us` is elapsed microseconds since daemon startup. It supports ordering and duration measurement but is not a wall-clock timestamp.
- `action` is exactly `pressed` or `released`.
- Duplicate physical edges are ignored and not assigned sequence numbers.
- With many-to-one mappings, only the first press and final release produce logical events.

## Delivery and recovery

Events are delivered in sequence order to each connected client. Each client has a bounded queue. A client that fills its queue is disconnected so it cannot delay capture or other clients. Version 0.1 provides no replay; after reconnecting, the hello snapshot is the source of current state.

## Explicit exclusions

Version 0.1 does not include exclusive client ownership, foreground-app detection, OS accessibility handoff, confidence values, debounce policy, hold events, per-client subscriptions, remote clients, or client-to-daemon control messages.

