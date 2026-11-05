import { useEffect, useState, useSyncExternalStore } from 'react';
import { THEMES, type ThemeId } from './catalogue.js';
import { APP_FONTS, CODE_FONTS, DEFAULTS, setAppearance, useAppearance } from './preferences.js';
import { applyTheme, getTheme, storeTheme, subscribeTheme, type ThemeChoice } from '@/client/theme.js';
export function AppearancePanel({ showHeading = true }: { showHeading?: boolean }) {
  const preferences = useAppearance();
  const mode = useSyncExternalStore(subscribeTheme, getTheme, getTheme);
  const changeMode = (value: ThemeChoice) => {
    storeTheme(value);
    applyTheme(value);
  };
  return (
    <section className="appearance-panel" aria-label="Appearance">
      <div className="appearance-heading">
        {showHeading && <h3>Appearance</h3>}
        <button
          type="button"
          onClick={() => {
            setAppearance(DEFAULTS);
            changeMode('system');
          }}
        >
          Reset
        </button>
      </div>
      <fieldset className="appearance-mode">
        <legend>Color mode</legend>
        {(['system', 'light', 'dark'] as const).map((value) => (
          <label key={value}>
            <input type="radio" name="appearance-mode" checked={mode === value} onChange={() => changeMode(value)} />
            {value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark'}
          </label>
        ))}
      </fieldset>
      <div className="appearance-grid">
        {(['light', 'dark'] as const).map((kind) => (
          <label key={kind}>
            {kind === 'light' ? 'Light theme' : 'Dark theme'}{' '}
            <select value={preferences[kind]} onChange={(e) => setAppearance({ [kind]: e.target.value as ThemeId })}>
              {THEMES.filter((t) => t.mode === kind).map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
            <div className="theme-swatches" aria-hidden="true">
              {(() => {
                const t = THEMES.find((t) => t.id === preferences[kind])!;
                return [t.bg, t.fg, t.accent].map((c, i) => <span key={i} style={{ background: c }} />);
              })()}
            </div>
          </label>
        ))}
        <label>
          Interface font
          <select value={preferences.appFont} onChange={(e) => setAppearance({ appFont: e.target.value })}>
            {APP_FONTS.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Interface size
          <SizeField
            min={12}
            max={18}
            value={preferences.appSize}
            onChange={(value) => setAppearance({ appSize: value })}
          />
        </label>
        <label>
          Code font
          <select value={preferences.codeFont} onChange={(e) => setAppearance({ codeFont: e.target.value })}>
            {CODE_FONTS.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Code size
          <SizeField
            min={11}
            max={22}
            value={preferences.codeSize}
            onChange={(value) => setAppearance({ codeSize: value })}
          />
        </label>
        <label>
          Code weight
          <select
            value={preferences.codeWeight}
            onChange={(e) => setAppearance({ codeWeight: Number(e.target.value) })}
          >
            <option value={400}>Regular</option>
            <option value={500}>Medium</option>
            <option value={600}>Semibold</option>
          </select>
        </label>
        <label className="appearance-check">
          <input
            type="checkbox"
            checked={preferences.ligatures}
            onChange={(e) => setAppearance({ ligatures: e.target.checked })}
          />
          Code ligatures
        </label>
      </div>
      <div className="appearance-preview">
        <span>Review changes</span>
        <code>
          <span style={{ color: 'var(--kind-callable)' }}>const</span> review = changes.filter(item =&gt; item.ready);
          <br />
          <span style={{ color: 'var(--fg-2)' }}>// 0 O 1 l I · {'{} [] () != => =>'}</span>
        </code>
      </div>
      <p className="appearance-hint">Saved automatically. System mode uses your selected light and dark themes.</p>
    </section>
  );
}

function SizeField({
  value,
  min,
  max,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const commit = () => {
    const parsed = Number(text);
    const next = text.trim() && Number.isFinite(parsed) ? Math.min(max, Math.max(min, Math.round(parsed))) : value;
    setText(String(next));
    onChange(next);
  };
  return (
    <input
      type="number"
      min={min}
      max={max}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          e.stopPropagation();
          e.currentTarget.blur();
        }
      }}
    />
  );
}
