# Architecture

USAHP keeps hardware handling separate from application behaviour. The daemon is the single input owner; any number of local clients can observe its normalized output.

```text
keyboard / Linux evdev
          │
          ▼
 capture + suppression
          │ physical edges
          ▼
 logical state machine ◄── stdin simulator
          │ logical transitions
          ▼
 broker + global sequence
          │
          ├── bounded client queue ── WebSocket client A
          └── bounded client queue ── WebSocket client B
```

## Capture and suppression

Every configured mapping must use a backend that can guarantee suppression. Keyboard mappings use a global grab on all supported platforms. Linux gamepad and dedicated switch inputs use an exclusive evdev device grab. A gamepad mapping on Windows or macOS is rejected at startup because v0 cannot guarantee suppression there.

## Logical aggregation

Each physical mapping has a unique `id` and points to a logical `switch_id`. Several mappings may point to the same logical switch:

- the first physical press emits one logical `pressed` event;
- further presses for that switch change no logical state;
- releases emit nothing while another mapped input remains held;
- the final physical release emits one logical `released` event.

Duplicate physical transitions are invalid and are ignored by the broker.

## Delivery

The broker assigns one global sequence number and a monotonic timestamp to every logical transition. Each client gets its own bounded queue. When a client cannot keep up and fills that queue, USAHP disconnects it rather than delaying capture or other listeners.

New and reconnected clients first receive a [`hello` snapshot](/protocol-v0#hello) containing the current state of every configured logical switch. Protocol 0.1 does not replay missed events.

## Responsibility boundary

| USAHP daemon | Client application |
| --- | --- |
| Capture configured inputs | Interpret an interaction |
| Guarantee suppression | Debounce if required |
| Aggregate physical state | Detect holds and repeats |
| Order logical transitions | Apply confidence or policy |
| Broadcast to local clients | Trigger an application action |

This boundary lets several applications observe a common, low-level event stream without embedding hardware-specific code in each app.
