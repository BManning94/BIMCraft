# The apps

`bimcraft` is the desktop app: `main.rs` reads the arguments, builds the session, attaches the updater and opens the window (the
write-up of the start, its log and the graphics fallback is under Startup in [the desktop](../docs/areas/desktop.md)). `bimcraft-cli`
is the same engine from a terminal: `run` (scripts and commands), `convert`, `info`, and `mcp`, the MCP server for agents, whose
tools run the same commands. Neither adds behaviour of its own: a feature is a command in the core.

- Only the desktop phones home, and only through `crates/update`; the CLI and the MCP server never do (a test reads their manifests).
- A release build on Windows has no console: a start that fails says so through `startup::note`, not `eprintln!` alone.
- `cargo test -p bimcraft -p bimcraft-cli` runs in a few seconds.
