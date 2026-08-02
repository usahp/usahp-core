# Simulator and testing

The built-in stdin simulator proves the complete logical and delivery path without requiring physical switch hardware.

## Enable the simulator

```toml
[simulator]
stdin = true
```

Start the daemon with a configuration containing at least one mapping. The simulator derives one internal input for each unique `switch_id`.

## Commands

Enter commands in the foreground daemon terminal:

```text
press switch_1
release switch_1
```

Unknown switch IDs and malformed commands are reported without creating events. Repeating `press` while already pressed or `release` while released is an invalid physical transition and does not create another logical event.

## Why simulator parity matters

Simulator edges enter the same broker command channel as hardware edges. They therefore use the same:

- many-to-one state machine;
- global sequence counter;
- monotonic timestamp source;
- per-client bounded queues;
- WebSocket serialization and broadcast path.

Only physical capture and OS suppression are bypassed.

## Smoke test

1. Start `usahpd` with `example.toml`.
2. Start two `usahp-listen` processes.
3. Confirm both listeners receive the same initial snapshot.
4. Press and release `switch_1` through stdin.
5. Confirm both listeners receive matching sequence numbers.
6. Stop and restart one listener while the switch is held.
7. Confirm its new hello snapshot says `pressed`.

## Automated validation

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The test suite covers configuration, many-to-one aggregation, invalid transitions, sequencing, queue overflow disconnection, snapshots, multiple listeners, reconnect behaviour, and simulator parity.
