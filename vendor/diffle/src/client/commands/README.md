# Commands and palette sources

`catalogue.ts` is lightweight metadata shared by the desktop launcher and review UI. IDs are stable preference keys: rename labels without renaming IDs. Register execution and availability together with the window's `CommandRegistry`; toolbar controls use `useCommandAction` to dispatch through the same handler. Dispose registrations when their owner unmounts. A review registration reads current state at execution rather than capturing an old snapshot.

Keep the provider free of store, transport, renderer and worker imports. The palette and shortcut editor load on demand. This registry does not replace the review runtime or make its existing singleton store safe for concurrent sessions; that work belongs to the session-isolation phase.

Search sources open once per palette session. Do expensive work inside `search`, respect its query `AbortSignal`, cap results at 20 per data source and dispose session resources on close. The explicit Commands category can show the entire lightweight command catalogue, within the 60-row display cap. Reuse an in-flight load across query changes, but cancel it on disposal. `invalidateSources()` retires captured review data; ordinary `refresh()` only updates availability/shortcuts and must not repeat ref requests. Every result action must validate that its originating review still owns the action. The palette debounces nonempty queries by 120ms, validates latest-query cancellation, retains at most 60 rows and drops result closures on close. Current files/commits come from the loaded snapshot; full history and provider searches are later sources.

Modifier shortcuts persist under `mdiff.shortcuts.v1`. `Mod` means Command on macOS and Control elsewhere. Vim aliases, counts and chords stay in the existing keymap; customization adds/replaces modifier bindings. Clear removes that binding, not the Vim alias. Reset restores platform defaults. Ctrl+O remains jump-back on Linux, so Open repository uses Ctrl+Shift+O there. The existing Find file input shortcut is retained; other commands yield to typing, modal dialogs, terminals, composition and AltGr. Editing/OS shortcuts are reserved. Collisions compare physical platform bindings, including inactive command scopes, so opening a review cannot silently change a custom binding's meaning.

New command/palette strings use `i18n/messages.ts` definitions and the catalogue's `command(id, EnglishLabel)` helper. Keep IDs stable and English fallbacks complete; interpolate named values as plain text and use `{ one, other }` for the current English plural forms. Run this from `vendor/diffle` to extract the initial catalogue:

```sh
TSX_TSCONFIG_PATH=tsconfig.client.json node --import tsx scripts/extract-messages.ts > /tmp/mdiff-en.json
```

Register future feature catalogues in that script. It rejects duplicate IDs and imports no DOM, store or transport code. `translate` accepts a locale and partial translation catalogue; missing entries fall back to English. Language selection, all Intl plural categories, source extraction and a partial French preview are described in [interface localization](../i18n/README.md). Shortcut tokens and bindings remain independent of language.
