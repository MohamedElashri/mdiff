# Interface language

**Preferences → Appearance → Language** offers English and a partial French preview. System selects English. Missing translations, native diagnostics and Git/provider/user content keep their source text. RTL is unsupported.

Language applies on full launch/page reload. Close all desktop windows before relaunching; native tray/notification labels last until process exit. OS dialogs follow the OS language. Storage failures fall back to English and report failed saves.

## Translating

- Use `uiText('English source', { name: value })` or stable `message(id, source)` definitions. Named IDs take precedence over source keys.
- Keep paths, refs, IDs, code, user content and shortcut tokens outside translation. Interpolation is plain text; changed placeholders fall back to English.
- Plurals accept all six `Intl.PluralRules` categories and require `other`. Missing translations use English rules; missing translated categories use translated `other`.
- Run `make messages` from the root after string changes; `make typecheck` checks catalogue freshness. The generated English catalogue is not imported at runtime.
- Edit reviewed French entries in [translations/fr.ts](translations/fr.ts). Extraction does not imply complete diagnostic coverage.

Development-only pseudo-localization expands templates while preserving placeholders/shortcut hints. Restart/reload after selecting it; production ignores a saved pseudo locale. Inspect narrow windows, large text, density, dialogs and focus rings. It does not simulate RTL.

Dates use interface locale/local time; Git date filters use local calendar days. Shortcut matching remains language-independent. Reduced motion disables decorative animations in desktop and standalone UI. See [validation](../../../../../VALIDATION.md).
