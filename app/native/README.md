# Comet native desktop

The desktop is a Rust application built with [iced 0.14](https://github.com/iced-rs/iced) and [iced-m3](https://github.com/tvolk131/iced-m3). It uses the existing SQLite, Nostr, and Blossom services directly.

## Run

```sh
cargo run --manifest-path app/native/Cargo.toml
```

Rust 1.88 or later is required. Linux builds need pkg-config, OpenSSL, D-Bus, fontconfig, Wayland, and xkbcommon development packages. No Node.js or webview is needed to build the desktop executable. The desktop app prefers GPU rendering through wgpu (Metal on macOS), with tiny-skia as a fallback when a GPU is unavailable. The `web-colors` feature keeps translucent surfaces consistent with the software renderer’s sRGB blending.

Development uses `md.comet-alpha.dev` in the platform configuration directory; release builds use `md.comet-alpha`. These are the existing desktop identifiers. Set `COMET_DATA_DIR` to use a separate workspace. Account databases and OS keychain names are preserved.

The notes pane has a floating “New note” button anchored at its bottom-right corner. It creates a note directly and stays in place while scrolling; extra space after the final note keeps it clear of the button. Import and export live in the “More” menu beside the list heading. Settings are in the sidebar, available through “Browse” on smaller windows.

Both appearances use Comet’s original bundled color palettes: neutral gray surfaces and blue accents mapped to Material roles.

The editor is one editable Markdown document. Headings, emphasis, lists, task boxes, quotes, code, tables, and local images render directly. Placing the cursor inside or directly beside formatted text reveals its Markdown delimiters; moving away hides them again. Tightly nested formatting around the same text reveals as one group, including combinations of bold, italic, strikethrough, links, and inline code. Heading reveal stops at the end of the heading’s physical line, before its line ending. List and quote prefixes reveal on their active physical line, replacing rendered bullets and quote bars with the authored markers. Task boxes remain clickable while their list prefix is edited. Mouse clicks reveal formatting on release, and the text layout stays stable during a drag. Clicking a task box changes only its `[ ]` or `[x]` marker. Tables reveal the current row's source while editing. Command-click (Control-click off macOS) opens a link.

Focus-driven reveal and hide transitions use a 150 ms ease-out: inline delimiters expand/contract and fade, moving surrounding text through the same layout used for hit testing and caret placement. Paragraphs that wrap in either endpoint settle directly into the final layout, without fading their text. This avoids both whole-paragraph flicker and repeated cascading wraps; single-row paragraphs retain inline motion. Code fences fade within their reserved height; tables, images, and rules use a short slide/fade between representations. Moving the caret redirects an unfinished transition from its current state. A held pointer pauses motion until release. Text edits, note switches, resizing, and font-size changes settle immediately. Settings → Editor animations is enabled by default, applies immediately, and is saved with appearance preferences. The app reads macOS Reduce Motion at launch; it takes precedence over the editor switch and also applies to Material controls. Static screenshot tests advance redraw time to compare settled views.

Command-Z undoes edits and Command-Shift-Z redoes them (Control-Z / Control-Shift-Z or Control-Y off macOS). Continuous typing and deletion form groups; a pause, cursor movement, or a different action starts a new step. Formatting, pasting, and task-box changes undo in one step, restoring the cursor and selection. Formatting buttons return keyboard focus to the document. Undo applies only while the editor has focus and respects read-only notes. History survives autosave and switching between recent notes; it resets after an external content replacement, account change, or restart. History stores bounded Markdown patches for up to eight recent notes, with up to 200 steps or 8 MiB per note (always retaining the latest edit).

`src/ui/markdown_editor/` maps rendered glyphs back to the original UTF-8 source and delegates keyboard, clipboard, and input-method handling to iced's native editor. Save/load, sync, and drafts still use the authored Markdown, including whitespace and line endings. The parser receives an offset-preserving view of CR-only and LFCR endings so its block boundaries agree with the editor’s physical lines; the original text is never rewritten. Trackpad scrolling preserves logical pixel movement, including fractional deltas and the current display scale; each mouse-wheel line moves one rendered text row. The scrollbar supports dragging beyond the track, clicking the track to jump, and resuming typing at the unchanged document caret. There is no separate preview state. Vim mode and embedded video remain unsupported.

## Validate

```sh
cargo fmt --check --manifest-path app/native/Cargo.toml
cargo test --locked --manifest-path app/native/Cargo.toml
cargo clippy --manifest-path app/native/Cargo.toml --all-targets
```

### Text engine pin

iced-m3 requires iced 0.14.0. The `cosmic-text` dependency is pinned to upstream commit [`8cd21a315a7eee77fdbfb00e516fb1fe2bfbd4ab`](https://github.com/pop-os/cosmic-text/commit/8cd21a315a7eee77fdbfb00e516fb1fe2bfbd4ab), which fixes the ASCII fast path ignoring font changes inside words. Without it, revealing Markdown markers can switch bold content to the regular font and shift the entire line's baseline. Remove this dependency override once the iced version used by iced-m3 includes the fix. No vendored source or altered Markdown strings are required.

### Screenshot tests

The visual suite renders the production application at 640×480, 1024×768, 1280×800, and 1440×900 logical pixels:

- **21 full-window snapshots** cover compact notes and editor layouts, the split and full layouts, the dark editor, settings, an empty notebook, delimiter reveal, and Markdown blocks. Scrolled-list and open-menu snapshots cover all three resolutions, checking that the floating creation button stays anchored, the final note scrolls fully above it, and creation, import, and export clicks emit their intended actions. Sidebar snapshots cover M3 filter chips, nested tags, and selection in both themes. Pixel checks compare the visible centers of the FAB's vector icon and label in light and dark themes, catching font-baseline offsets that widget bounds alone miss.
- **49 interaction scenarios with 495 visual checkpoints** cover typing and deletion, delimiter reveal and hiding, arrow keys and preferred columns, Home/End on wrapped rows, Shift selection and replacement, double-click word selection, dragging across lines, task-box toggles followed by typing, table-cell clicks, copy/paste, Unicode and CRLF preservation, focus loss and return, read-only notes, resizing across pane breakpoints, and keyboard/wheel scrolling. Held-click regressions cover bold, italic, strikethrough, links, headings, inline code, code blocks, lists, quotes, and tables at 640×480 and 1440×900. They include stationary pointer events and slight hand movement before release, then verify typing at the original insertion point. Separate scenarios verify deliberate dragging and Shift-click selection. Undo/redo scenarios cover typing groups, cursor movement, selection replacement, new edits after undo, cut/paste, checkbox clicks, keyboard and toolbar formatting, and focus boundaries across all three resolutions. Additional regressions compare unchanged pixels around emphasis reveal, keep both code fences visible anywhere inside the block without moving its contents, align checkboxes at four editor font sizes, and edit highlighted inline tags in both themes. Nested-formatting regressions cross both outer edges and every internal caret position in both directions at all three resolutions, verify stable text placement while revealed, and cover selections touching the group and held clicks followed by typing. Bold inline-code regressions verify that revealing markers, moving into the word, and hiding markers leave surrounding text stationary at three resolutions and two font sizes. Metric checks also cover the original longer sentence and italic combinations. Empty-line regressions at all three resolutions and two font sizes verify identical surrounding pixels before and after typing a space or character, deleting back to empty, navigating through blank rows, and clicking at the far right of a blank row.

Block-prefix regressions cover revealed and rendered lists, ordered lists, quotes, and nested quotes at all three resolutions, including moving across line endings and deleting markers. Heading regressions cover moving and clicking across LF and CRLF endings and inserting an empty line; parser checks also cover CR and LFCR without changing authored source.

Scrolling regressions cover pixel and fractional trackpad input at 1× and 2× display scaling, horizontal gestures, wheel movement at two font sizes, thumb dragging, end clamping, track clicks, and preserved source/caret at 640×480, 1024×768, and 1440×900.

`src/ui/scroll_input.rs` normalizes physical trackpad pixels at the window boundary, including dialog and menu overlays. iced-winit 0.14 forwards winit’s physical deltas unchanged; remove this adapter when upstream handles display scaling. A scrolled Settings screenshot also verifies identical movement at 1× and 2×.

Sidebar focus regressions exercise all seven navigation filters and both tag chips at 1280×800 and 1440×900, with visible document checkboxes. They check that the editor loses keyboard focus on mouse-down, the caret and formatting markers stay hidden after release, and typing or arrow keys do not edit or move the document cursor. Clicking a document checkbox afterward must toggle only that task and resume typing at the saved caret. Child controls report capture through their own iced `Shell`, then merge their messages and effects into the parent; a sidebar capture cannot masquerade as a checkbox click.

`tests/support/editor_driver.rs` sends keyboard and pointer events through the real view and update function, retains the widget cache between events, and applies edits and focus operations before the next keystroke. Double-click button events arrive in one batch to avoid rendering delays exceeding the click threshold. Every checkpoint asserts exact Markdown, the cursor's UTF-8 byte position, selection, and a cropped screenshot of the editor. Fixtures have no live service context; unexpected external tasks fail the test. History unit tests use explicit timestamps for grouping boundaries and cover repeated characters, Unicode, CRLF, and history limits. Persistence tests use disposable accounts to verify undo through autosave, note switching, draft recovery, and failed draft writes.

Both renderers run the same 522 saved-screenshot checkpoints, including the exact source, cursor, selection, layout, and interaction assertions. Live-frame pixel checks also verify that unchanged text stays visible throughout wrapped reveal and hide transitions. The repository Cargo configuration defaults `ICED_TEST_BACKEND` to `tiny-skia`; override it with `wgpu` to exercise the desktop's GPU pipeline. Both render offscreen with bundled Roboto and Fira Mono at 2× scale. Settled interaction captures advance redraw time without sleeps; live-frame checks sample intermediate timestamps. Visual fixtures use characters covered by the bundled fonts. Neither mode opens a window or accesses a live account. The CPU suite needs no graphics adapter; a requested wgpu run fails if no adapter is available and cannot fall back to tiny-skia.

CPU captures must match the reviewed PNGs exactly. GPU captures compare to those same CPU baselines, without separate GPU golden files. The comparison preserves image dimensions and pixel coordinates: no resizing, blurring, or image alignment. Flat colors allow at most 3/255 per RGB channel; alpha must match exactly. Edge differences are restricted to a one-physical-pixel neighborhood (half a logical pixel), with up to 64/255 additional coverage difference only where both images contain an edge. This accommodates the different antialiasing of curved controls and shadow boundaries. Every 32×32 pixel tile also limits its mean signed color error to 4/255 per channel. There is no whole-image mismatch percentage that could hide a missing caret in a large blank editor.

`tests/visual_comparison.rs` deliberately damages real reviewed screenshots to verify that the allowance rejects a missing caret or glyph, a caret moved one logical pixel, a missing selection highlight, shifted or wrapped lines, clipped text, surface-color changes, opacity changes, and different image dimensions. It also checks harmless rounding and edge-coverage changes.

```sh
# Compare to the committed PNG baselines.
cargo test --locked --manifest-path app/native/Cargo.toml --test screenshots --test editor_interactions

# Run the same scenarios through wgpu (Metal on macOS).
ICED_TEST_BACKEND=wgpu cargo test --locked --manifest-path app/native/Cargo.toml --test screenshots --test editor_interactions -- --test-threads=1

# Intentionally regenerate, inspect the PNGs, then run the comparison again.
ICED_TEST_BACKEND=tiny-skia UPDATE_SCREENSHOTS=1 cargo test --manifest-path app/native/Cargo.toml --test screenshots --test editor_interactions
```

Missing baselines fail in comparison mode, and GPU runs reject `UPDATE_SCREENSHOTS=1`. Baselines live in `tests/screenshots/`, with interaction checkpoints grouped by scenario under `interactions/`. On a pixel or interaction assertion failure, the actual PNG goes to `target/screenshot-failures/`, which CI uploads. GPU comparison failures also save the expected PNG and a magnified-difference heatmap, with rejected pixels highlighted in pink. All current GPU captures are available in `target/screenshot-gpu/`. The `pnpm --filter @comet/app test:screenshots`, `test:screenshots:gpu`, and `test:screenshots:update` scripts run both visual suites. `just app-test-screenshots-gpu` is the GPU shortcut.

CI keeps the exact CPU suite on macOS and adds a required wgpu suite on Linux using [Mesa's Lavapipe software Vulkan adapter](https://docs.mesa3d.org/sourcetree.html). This executes iced's GPU shaders, compositing, and readback without requiring a paid hardware-GPU runner. It is renderer-pipeline coverage, not a hardware performance test; local Metal runs cover the Mac GPU, and the rendering benchmark separately requires hardware. CI prints its Vulkan adapter information and fails rather than skipping if the adapter cannot initialize.

To add a scenario, start an `EditorDriver` with a Markdown fixture and window size, use its input methods to perform the interaction, then call `check` with a unique checkpoint name and the expected source, cursor, and selection. Do not set the cursor or replace the source directly after boot. Regenerate only the affected scenario with a test-name filter, inspect every changed PNG, and run the comparison again.

### Rendering performance

Run `cargo run --release --locked --manifest-path app/native/Cargo.toml --example rendering_benchmark` to compare tiny-skia and wgpu on the same scrolling fixtures at 1280×800 and 2× scale. Pass `-- wgpu` or `-- tiny-skia` to measure one backend. An optional second argument saves inspection screenshots after timing, for example `-- both /tmp/comet-rendering`. The fixtures include a form using iced-m3 controls, a form inside an iced-m3 dialog, and the production Settings dialog. They use offline fixture data, never a live account.

The benchmark reports median and 95th-percentile repaint times after eight warmup frames and 30 measured frames. GPU measurements include command encoding, submission, and waiting for that submission to finish; software measurements use iced's partial redraw algorithm. Neither measurement includes screenshot readback, window presentation, or vsync, so these are repaint latencies rather than measured screen frame rates. Input and widget drawing time is reported separately. A hardware adapter is required for GPU measurements; the benchmark prints its name and backend and rejects CPU adapters.

## Distribution

`cargo build --release --manifest-path app/native/Cargo.toml` produces the native executable. Install `cargo-packager` 0.11.8, then run `python3 app/scripts/package-native.py` to create native installers. CI builds macOS arm64 and x86-64 app/DMG packages and Linux DEB/AppImage packages. The packaging script accepts the existing Apple signing and notarization environment variables. Signed release packaging requires the CI credentials; local verification can produce unsigned app bundles.

iced-m3 includes Roboto under OFL-1.1. Static Roboto emphasis faces and Fira Mono supplement its typography. The component and font notices in `licenses/` are included in native packages.
