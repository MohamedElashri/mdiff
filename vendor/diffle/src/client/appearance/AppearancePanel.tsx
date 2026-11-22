import { languageChoice, storeLanguage, type LanguageChoice } from '@/client/i18n/locale.js';
import { uiText } from '@/client/i18n/runtime.js';
import { useEffect, useId, useState, useSyncExternalStore } from 'react';
import { THEMES, type ThemeId } from './catalogue.js';
import { APP_FONTS, CODE_FONTS, DEFAULTS, setAppearance, useAppearance } from './preferences.js';
import { applyTheme, getTheme, storeTheme, subscribeTheme, type ThemeChoice } from '@/client/theme.js';
export function AppearancePanel({ showHeading = true }: { showHeading?: boolean }) {
  const preferences = useAppearance();
  const [language, setLanguage] = useState(languageChoice);
  const [languageError, setLanguageError] = useState(false);
  const densityHint = useId();
  const mode = useSyncExternalStore(subscribeTheme, getTheme, getTheme);
  const changeMode = (value: ThemeChoice) => {
    storeTheme(value);
    applyTheme(value);
  };
  return (
    <section className="appearance-panel" aria-label={uiText('Appearance')}>
      <div className="appearance-heading">
        {showHeading && <h3>{uiText('Appearance')}</h3>}
        <button
          type="button"
          onClick={() => {
            setAppearance(DEFAULTS);
            changeMode('system');
          }}
        >
          {uiText('Reset')}
        </button>
      </div>
      <label className="appearance-language">
        {uiText('Language')}
        <select
          aria-label={uiText('Language')}
          value={language}
          onChange={(event) => {
            const choice = event.target.value as LanguageChoice;
            if (storeLanguage(choice)) {
              setLanguage(choice);
              setLanguageError(false);
            } else setLanguageError(true);
          }}
        >
          <option value="system">{uiText('System (English)')}</option>
          <option value="en">English</option>
          <option value="fr">{uiText('Français (preview; partial translation)')}</option>
          {import.meta.env.DEV && <option value="qps-ploc">Pseudo-localization (development)</option>}
        </select>
        <small className="appearance-hint" role="status">
          {uiText('Restart mdiff to apply the language. Save your work before restarting.')}
        </small>
        {languageError && <small role="alert">{uiText('Language could not be saved.')}</small>}
      </label>
      <fieldset className="appearance-mode">
        <legend>{uiText('Color mode')}</legend>
        {(['system', 'light', 'dark'] as const).map((value) => (
          <label key={value}>
            <input type="radio" name="appearance-mode" checked={mode === value} onChange={() => changeMode(value)} />
            {value === 'system' ? uiText('System') : value === 'light' ? uiText('Light') : uiText('Dark')}
          </label>
        ))}
      </fieldset>
      <div className="appearance-grid">
        <label className="appearance-density">
          {uiText('Interface density')}
          <select
            aria-label={uiText('Interface density')}
            aria-describedby={densityHint}
            value={preferences.density}
            onChange={(e) => setAppearance({ density: e.target.value === 'compact' ? 'compact' : 'default' })}
          >
            <option value="default">{uiText('Default')}</option>
            <option value="compact">{uiText('Compact')}</option>
          </select>
          <small id={densityHint} className="appearance-hint">
            {uiText('Adjusts controls and file headers. Code size stays independent.')}
          </small>
        </label>
        {(['light', 'dark'] as const).map((kind) => (
          <label key={kind}>
            {kind === 'light' ? uiText('Light theme') : uiText('Dark theme')}{' '}
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
          {uiText('Interface font')}
          <select value={preferences.appFont} onChange={(e) => setAppearance({ appFont: e.target.value })}>
            {APP_FONTS.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          {uiText('Interface size')}
          <SizeField
            min={12}
            max={18}
            value={preferences.appSize}
            onChange={(value) => setAppearance({ appSize: value })}
          />
        </label>
        <label>
          {uiText('Code font')}
          <select value={preferences.codeFont} onChange={(e) => setAppearance({ codeFont: e.target.value })}>
            {CODE_FONTS.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          {uiText('Code size')}
          <SizeField
            min={11}
            max={22}
            value={preferences.codeSize}
            onChange={(value) => setAppearance({ codeSize: value })}
          />
        </label>
        <label>
          {uiText('Code weight')}
          <select
            value={preferences.codeWeight}
            onChange={(e) => setAppearance({ codeWeight: Number(e.target.value) })}
          >
            <option value={400}>{uiText('Regular')}</option>
            <option value={500}>{uiText('Medium')}</option>
            <option value={600}>{uiText('Semibold')}</option>
          </select>
        </label>
        <label className="appearance-check">
          <input
            type="checkbox"
            checked={preferences.ligatures}
            onChange={(e) => setAppearance({ ligatures: e.target.checked })}
          />
          {uiText('Code ligatures')}
        </label>
      </div>
      <div className="appearance-preview">
        <span>{uiText('Review changes')}</span>
        <code>
          <span style={{ color: 'var(--kind-callable)' }}>const</span> review = changes.filter(item =&gt; item.ready);
          <br />
          <span style={{ color: 'var(--fg-2)' }}>// 0 O 1 l I · {'{} [] () != => =>'}</span>
        </code>
      </div>
      <p className="appearance-hint">
        {uiText('Saved automatically. System mode uses your selected light and dark themes.')}
      </p>
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
