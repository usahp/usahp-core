# The USAHP draft in plain English

This is a plain-language companion to the [draft USAHP handoff specification](/spec). It explains a possible future standard. It is not a description of everything the current USAHP broker can do.

::: warning Draft, not a current guarantee
The implemented contract is [protocol 0.2](/protocol-v0). It provides one local heartbeat-managed session, but operating-system handoff, focus detection, arbitration, escape hatches, and continuous confidence remain proposed future work.
:::

## The problem

Switch users can face a conflict when they move between operating-system scanning and an application with its own scanning interface. Both systems may try to read the same switch, causing duplicate or missing actions.

The draft USAHP standard explores a common way to hand control between those systems without forcing every application to understand every type of switch hardware.

## The proposed target

The target design has four conceptual states:

1. **OS routing.** The operating system owns switch navigation.
2. **Handshake.** An application asks for switch control and the system accepts or rejects it.
3. **App control.** An accepted application receives switch events while the system monitors safety conditions.
4. **Recovery.** The system revokes application control after a failure and returns to OS routing.

These states are a design goal. The current daemon is not an operating-system service and cannot provide full system routing or recovery.

## Proposed safety mechanisms

The draft explores two complementary mechanisms:

- **Escape hatch:** a user-controlled hardware pattern that an OS-level service would intercept before an application.
- **Heartbeat:** a session client periodically proves it is responsive; missed heartbeats cause revocation.

The escape hatch and system-level recovery are not implemented. A heartbeat-backed local session exists in protocol 0.2, but it is not operating-system recovery or clinical lockout protection.

## Proposed application roles

The future design considers three roles:

- **Exclusive foreground:** a focused application temporarily receives switch input.
- **Primary controller:** an approved background controller receives global input.
- **Passive observer:** an application receives a read-only copy.

The broker implements one local `exclusive_foreground` session without checking actual window focus. Primary controllers, passive subscriptions during exclusivity, arbitration UI, and operating-system enforcement remain future work.

## Confidence

The current wire format carries `100.0` for a binary press and `0.0` for a release. It does not yet carry continuous analog samples from BCI, facial-gesture, pressure, or similar sources.

The final confidence model is intentionally open. Its units, range, unknown state, sampling, snapshots, and protocol version will be designed in [issue #6](https://github.com/usahp/usahp-core/issues/6). Applications will continue to own threshold and activation policy.

## Platform ideas

The draft discusses possible platform-specific implementations:

- Desktop systems could use a privileged local service.
- Android could use an `AccessibilityService` and bound-service IPC.
- iOS would require operating-system support or an external hardware bridge.

These are proposals, not shipped USAHP components or commitments from platform vendors.

## What exists today

The implemented Rust broker currently:

- captures and suppresses configured keyboard input on Windows, macOS, and Linux;
- supports exclusively grabbed evdev switch or gamepad devices on Linux;
- aggregates physical inputs into logical `pressed` and `released` edges;
- provides legacy broadcasts and one heartbeat-managed exclusive route on `ws://127.0.0.1:7312`;
- provides a simulator and reference listener;
- carries interim binary confidence values;
- releases held state and pauses capture after managed-session revocation.

It does not provide OS scanning, focus detection, arbitration, an escape hatch, remote access, or a continuous confidence stream.

## For developers today

1. Connect to `ws://127.0.0.1:7312`.
2. Read the `hello` snapshot.
3. Listen for `switch_event` frames.
4. Keep debounce, holds, thresholds, and activation policy in your application.
5. Reconnect and replace local state from the next snapshot if the socket closes.

Use the [implemented protocol reference](/protocol-v0) rather than this draft when building a current client.
