# AGENTS

## Projects

- **app/** — Native desktop notes app (Rust + iced + iced-m3)
- **relay/** — Nostr sync relay
- **blossom/** — Blossom blob storage server
- **web/** — Web app: admin portal and browser-based notes client
- **www/** — Marketing site
- **mcp/** — MCP server
- **packages/data/** — Shared database schema and migrations
- **packages/nostr/** — Shared Nostr utilities and types
- **docs/** — Development docs and product context

## Defaults

- Treat the project as greenfield. Prefer clean solutions over compatibility layers, migration hacks, or legacy-preserving abstractions.
- Do not carry forward temporary transition logic once a cleaner baseline can replace it.
- **Linting**: All packages use oxlint (not ESLint). Root `.oxlintrc.json` provides shared config; app/ and web/ override with React-specific settings. `@tanstack/eslint-plugin-query` rules are not yet available — revisit when oxlint JS plugins stabilize.
- **Formatting**: All packages use oxfmt (not Prettier). Root `.oxfmtrc.json` provides shared config with Tailwind class sorting enabled.

## App

Native desktop notes app in `app/native/` (Rust, iced 0.14, iced-m3).

- UI state and messages live in `src/ui/`; common controls use iced-m3.
- Domain, ports, and storage/sync adapters remain in separate modules.
- `runtime::AppContext` owns native services; UI actions call typed commands directly.
- Markdown is canonical; editing, save/load, import/export and sync must preserve authored structure.
- Use `cargo fmt`, `cargo clippy`, and `cargo test` with `app/native/Cargo.toml`.
- Screenshot tests render the production view with tiny-skia and bundled fonts. Baselines live in `app/native/tests/screenshots/`. Update intentionally with `UPDATE_SCREENSHOTS=1`, inspect the PNGs, and rerun without that variable.
