# Limits and explicit exclusions

USAHP v0 intentionally solves one problem: deliver suppressed, normalized local switch edges to every listening application.

## Protocol stability

The wire version is `0.1` and does not carry a compatibility promise. Clients should reject unsupported protocol versions and expect the daemon to restart with sequence numbering reset.

## Transport limits

- IPv4 loopback only (`127.0.0.1`)
- no authentication or TLS
- no remote access
- no per-client subscriptions or filtering
- no replay of missed events
- no client-to-daemon control messages
- slow clients are disconnected when their bounded queue fills

Because the server is loopback-only and passive, it is intended for local processes within the same user environment—not as a network service or security boundary.

## Input limits

- keyboard codes are limited to the documented set;
- gamepad and dedicated switch capture is Linux-only;
- an entire Linux evdev device is grabbed, including unmatched controls;
- every enabled mapping must guarantee suppression or startup validation fails;
- physical permissions and hardware behaviour require manual platform testing.

## Application policy is out of scope

Protocol 0.1 does not define:

- debounce or noise filtering;
- hold, repeat, chord, or gesture events;
- confidence values;
- activation policy;
- exclusive client ownership;
- foreground-application detection;
- OS accessibility handoff;
- keyboard or pointer synthesis;
- persistent state across daemon restarts.

Clients build these policies from `pressed` and `released` edges. This is an explicit architectural boundary rather than missing protocol metadata.
