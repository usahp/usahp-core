---
layout: home

hero:
  name: USAHP
  text: One switch event stream. Every local app.
  tagline: A small Rust daemon suppresses configured inputs, normalizes their edges, and broadcasts them over a loopback-only WebSocket.
  actions:
    - theme: brand
      text: Get started
      link: /quick-start
    - theme: alt
      text: Protocol 0.1
      link: /protocol-v0

features:
  - title: Cross-platform keyboard input
    details: Capture and suppress configured keys on Windows, macOS, and Linux, with explicit platform permission guidance.
  - title: One logical switch, many inputs
    details: Map several physical inputs to one switch and receive only the first press and final release.
  - title: Passive local broadcast
    details: Every listener receives the same ordered event stream without controlling the daemon or blocking input capture.
---

## A deliberately small boundary

USAHP v0 is a local switch-event broker. It owns input capture, required suppression, logical state aggregation, and ordered delivery. Applications own the meaning of an interaction: debounce policy, holds, confidence, and activation all remain client concerns.

<div class="protocol-flow">
  <div><strong>1 · Capture</strong>Configured keyboard or Linux evdev input is suppressed at the source.</div>
  <div><strong>2 · Normalize</strong>Physical edges become logical <code>pressed</code> and <code>released</code> transitions.</div>
  <div><strong>3 · Broadcast</strong>Every local WebSocket listener receives the same versioned JSON event.</div>
</div>

The server binds to `127.0.0.1` only. Protocol 0.1 has no authentication, TLS, subscriptions, replay, or remote access.

[See the architecture →](/architecture)
