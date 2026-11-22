import { useEffect, useRef, useState, type ComponentType } from "react";
import { Wrench } from "lucide-react";
import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
export function RepositoryTools() {
  const [Menu, setMenu] = useState<ComponentType | null>(null),
    [loading, setLoading] = useState(false),
    [error, setError] = useState("");
  const pending = useRef(false),
    live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const open = async () => {
    if (pending.current) return;
    pending.current = true;
    setLoading(true);
    setError("");
    try {
      const module = await import("./RepositoryToolsMenu");
      if (live.current) setMenu(() => module.default);
    } catch (e) {
      if (live.current) setError(String(e));
    } finally {
      pending.current = false;
      if (live.current) setLoading(false);
    }
  };
  if (Menu) return <Menu />;
  return (
    <div className="repository-tools-trigger">
      <button
        id="repository-tools-toggle"
        aria-label={uiText("Repository tools")}
        title={uiText("Repository tools")}
        aria-busy={loading}
        disabled={loading}
        onClick={() => void open()}
      >
        <Wrench size={15} />
        <span className="action-label">
          {loading ? uiText("Loading…") : uiText("Tools")}
        </span>
      </button>
      {error && (
        <div role="alert">
          <p>
            {uiText("Repository tools could not be loaded: {error}", { error })}
          </p>
          <button onClick={() => void open()}>{uiText("Retry")}</button>
        </div>
      )}
    </div>
  );
}
