# Commands and palette sources

[catalogue.ts](catalogue.ts) holds lightweight metadata. IDs are stable preference keys; labels may change. Register availability/execution together in the window's `CommandRegistry`; toolbars dispatch through `useCommandAction`. Handlers read current state and dispose with their owner. Keep the provider free of store, transport, renderer and worker imports.

## Search sources

Open sources once per palette session. Search lazily, respect `AbortSignal`, reuse in-flight loads across queries and dispose on close. Results must revalidate their originating review before acting.

- `invalidateSources()` retires captured review data; `refresh()` updates availability/shortcuts without refetching.
- Limit each source to 20 results and the palette to 60 rows; nonempty queries debounce 120 ms.
- Keep current-file/commit searches snapshot-based; History/Hosting sources own their lazy reads. Drop result closures on close.

## Shortcuts and labels

`Mod` means Command on macOS, Control elsewhere. Overrides customize modifier bindings; Vim aliases/chords remain. Clear removes the binding; Reset restores defaults. Linux Open repository uses Ctrl+Shift+O because Ctrl+O is jump-back.

Preserve typing/modal/terminal, composition and AltGr isolation. Reserve editing/OS shortcuts and compare physical collisions across inactive scopes too. Desktop preferences are native-revisioned; `mdiff.shortcuts.v1` remains the shared storage key.

Use stable `command(id, EnglishLabel)` definitions, named placeholders and [localization rules](../i18n/README.md). Run `make messages` from the repository root after changing extracted strings; register feature catalogues with the extractor.
