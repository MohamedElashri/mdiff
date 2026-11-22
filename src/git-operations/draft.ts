import { useCallback, useEffect, useRef, useState } from "react";
import { gitApi } from "./api";
export function useCommitDraft(
  context: string | null,
  flushRef: { current: () => Promise<unknown> },
  purpose?: string,
) {
  const [message, setMessage] = useState(""),
    [loaded, setLoaded] = useState(false),
    [error, setError] = useState("");
  const data = useRef({
    context,
    purpose,
    revision: 0,
    message: "",
    baseline: "",
  });
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const flush = useCallback(() => {
    const task = queue.current
      .catch(() => {})
      .then(async () => {
        const state = data.current;
        if (!state.context || state.message === state.baseline) return;
        const saved = state.purpose
          ? await gitApi.saveDraft(
              state.context,
              state.revision,
              state.message,
              state.purpose,
            )
          : await gitApi.saveDraft(
              state.context,
              state.revision,
              state.message,
            );
        if (data.current === state) {
          state.revision = saved.revision;
          state.baseline = saved.message;
        }
      });
    queue.current = task;
    return task;
  }, []);
  useEffect(() => {
    flushRef.current = flush;
    return () => {
      if (flushRef.current === flush) flushRef.current = async () => {};
    };
  }, [flush, flushRef]);
  const load = useCallback(async () => {
    const state = { context, purpose, revision: 0, message: "", baseline: "" };
    data.current = state;
    setLoaded(false);
    setError("");
    setMessage("");
    if (!context) {
      setLoaded(true);
      return;
    }
    try {
      const saved = await (purpose
        ? gitApi.draft(context, purpose)
        : gitApi.draft(context));
      if (data.current !== state) return;
      Object.assign(state, {
        revision: saved.revision,
        message: saved.message,
        baseline: saved.message,
      });
      setMessage(saved.message);
      setLoaded(true);
    } catch (error) {
      if (data.current === state) setError(String(error));
    }
  }, [context, purpose]);
  useEffect(() => {
    void load();
    return () => {
      data.current = {
        context: null,
        purpose: undefined,
        revision: 0,
        message: "",
        baseline: "",
      };
    };
  }, [load]);
  return {
    message,
    loaded,
    error,
    load,
    flush,
    baseline: () => data.current.baseline,
    revision: () => data.current.revision,
    change: (message: string) => {
      data.current.message = message;
      setMessage(message);
    },
  };
}
