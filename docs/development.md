# Development

USAHP is an MIT-licensed Rust workspace with four Rust packages:

| Crate | Role |
| --- | --- |
| `usahp-core` | Shared configuration, protocol types, and logical state machine. |
| `usahp-daemon` | Input backends, broker, simulator, and loopback WebSocket server. |
| `usahp-listen` | Reference client that prints snapshots and events. |
| `usahp-control` | Tauri tray utility that owns and supervises the daemon runtime. |

## Validate a change

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

GitHub Actions runs those checks on Windows, macOS, and Linux. Changes to capture or suppression also need the [manual platform checks](/platforms#verify-suppression).

## Work on the documentation

The documentation uses VitePress and is versioned with the Rust implementation.

```shell
npm install
npm run docs:dev
```

Build the production site, including internal-link validation, with:

```shell
npm run docs:build
```

The site uses `/usahp-core/` as its production base path for GitHub Pages.

## Work on USAHP Control

Install the root Node dependencies, then launch the Tauri development build:

```shell
npm install
npm run control:dev
```

Build only the TypeScript/Vite frontend with `npm run control:build`. The normal Rust workspace formatting, Clippy, and test commands also compile and validate the Tauri backend.

## Contribution guidance

- Keep the public JSON contract versioned and update [`protocol-v0.md`](/protocol-v0) when serialization changes.
- Keep examples aligned with `example.toml` and the shared Rust types.
- Preserve the loopback-only boundary unless a future protocol explicitly changes the threat model.
- Add automated tests for state, ordering, queue, or serialization changes.
- Document platform-specific permissions and manual verification for input-backend changes.

Open an issue or pull request in the [GitHub repository](https://github.com/usahp/usahp-core).
