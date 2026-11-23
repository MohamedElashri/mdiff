import { useEffect, useRef, useState, type ReactNode } from "react";
import { Heart, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
} from "../../vendor/diffle/src/client/ui/dialog";
import { uiText } from "../../vendor/diffle/src/client/i18n/runtime";
import "./preferences.css";
export type PreferenceSection = {
  id: string;
  label: string;
  saveMode?: "manual";
  content: ReactNode;
};
export function PreferencesDialog({
  section,
  onSection,
  sections,
  dirty,
  error,
  onClose,
}: {
  section: string;
  onSection: (section: string) => void;
  sections: PreferenceSection[];
  dirty: Record<string, boolean>;
  error: string;
  onClose: () => void;
}) {
  const [visited, setVisited] = useState(() => new Set([section]));
  const closeButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    setVisited((old) => new Set([...old, section]));
  }, [section]);
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        className="settings preferences-window"
        aria-labelledby="preferences-title"
        initialFocus={closeButton}
      >
        <header>
          <h2 id="preferences-title">{uiText("Preferences")}</h2>
          <div className="preferences-header-actions">
            <a
              className="preferences-tip"
              data-in-about={section === "about"}
              href="https://ko-fi.com/melashri"
            >
              <Heart size={15} aria-hidden="true" />
              {uiText("Leave a tip on Ko-fi")}
            </a>
            <button
              ref={closeButton}
              aria-label={uiText("Close preferences")}
              onClick={onClose}
            >
              <X size={18} />
            </button>
          </div>
        </header>
        {error && (
          <p role="alert" className="settings-error">
            {error}
          </p>
        )}
        <div className="preferences-layout">
          <nav
            aria-label={uiText("Preference sections")}
            role="tablist"
            aria-orientation="vertical"
          >
            {sections.map((s, index) => (
              <button
                key={s.id}
                id={`preference-tab-${s.id}`}
                role="tab"
                aria-selected={section === s.id}
                aria-controls={`preference-panel-${s.id}`}
                tabIndex={section === s.id ? 0 : -1}
                onClick={() => onSection(s.id)}
                onKeyDown={(event) => {
                  const next =
                    event.key === "ArrowDown"
                      ? (index + 1) % sections.length
                      : event.key === "ArrowUp"
                        ? (index + sections.length - 1) % sections.length
                        : event.key === "Home"
                          ? 0
                          : event.key === "End"
                            ? sections.length - 1
                            : -1;
                  if (next >= 0) {
                    event.preventDefault();
                    onSection(sections[next]!.id);
                    event.currentTarget.parentElement
                      ?.querySelectorAll<HTMLButtonElement>("button")
                      [next]?.focus();
                  }
                }}
              >
                {s.label}
                {dirty[s.id] && (
                  <span
                    className="preferences-dirty"
                    aria-label={uiText("Unsaved changes")}
                  >
                    •
                  </span>
                )}
              </button>
            ))}
          </nav>
          <div className="preferences-panels">
            {sections
              .filter((s) => s.id === section || visited.has(s.id))
              .map((s) => (
                <div
                  key={s.id}
                  role="tabpanel"
                  id={`preference-panel-${s.id}`}
                  aria-labelledby={`preference-tab-${s.id}`}
                  hidden={s.id !== section}
                >
                  <h3 className="preferences-section-title">{s.label}</h3>
                  {s.saveMode === "manual" && (
                    <p className="preference-persistence">
                      {uiText("Changes apply when you save this section.")}
                    </p>
                  )}
                  {s.content}
                </div>
              ))}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
