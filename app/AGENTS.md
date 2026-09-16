# Native desktop app

## Principles

- Single responsibility; compose small modules and explicit services.
- Keep domain logic separate from UI and infrastructure.
- Fail visibly; never discard a draft after a failed save.
- Markdown is canonical. Preserve authored whitespace, line endings, links, tags, and tables across editing, save/load, sync, import and export.
- Add a regression test for any content drift or lost-edit bug.

## Stack and structure

- Rust application: `native/`, with iced 0.14 and iced-m3 common controls.
- `src/ui/`: state, update logic, views, deterministic screenshot fixtures.
- `src/runtime.rs`: native service context and typed sync events.
- `src/commands/`: typed application operations.
- `src/domain/`, `src/ports/`, `src/adapters/`: existing notes, SQLite, Nostr and Blossom services.
- SQLite migrations remain under `src/adapters/sqlite/migrations.rs`.

## Conventions

Use idiomatic Rust names and `cargo fmt`. Keep backend business rules in existing services. UI errors must retain editable content and prevent unsafe navigation. Use iced-m3 for common controls, including task checkboxes. `src/ui/markdown_editor/` renders the Markdown document and maps cursor positions to its unchanged source; iced's native editor supplies keyboard, clipboard, and input-method handling. Reveal formatting delimiters at the cursor instead of adding a separate preview mode. No webview or JavaScript frontend.

## Validation

- `cargo test --manifest-path native/Cargo.toml`
- `cargo fmt --check --manifest-path native/Cargo.toml`
- `cargo clippy --manifest-path native/Cargo.toml --all-targets`
- Screenshot tests use the production widget tree and bundled fonts. tiny-skia matches the reviewed baselines exactly; `ICED_TEST_BACKEND=wgpu` runs the same scenarios with bounded edge tolerance against those baselines. Run both modes for visual changes when an adapter is available. Missing baselines or requested adapters fail. Update only with `ICED_TEST_BACKEND=tiny-skia UPDATE_SCREENSHOTS=1 cargo test --manifest-path native/Cargo.toml --test screenshots --test editor_interactions`, inspect all changed images, then rerun normally.
- Test persistence with disposable directories; never use the user's live account for tests.
