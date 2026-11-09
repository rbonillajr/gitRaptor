import { useCallback, useRef } from "react";

const WINDOW_MS = 500;

/**
 * Collects printable characters typed within 500 ms of each other and returns the index of the first label
 * that starts with them (case-insensitive), searching after `current` and wrapping; `null` when none does.
 */
export function useTypeahead(labels: () => string[]): (key: string, current: number) => number | null {
  const buffer = useRef({ text: "", at: 0 });
  const labelsRef = useRef(labels);
  labelsRef.current = labels;

  return useCallback((key, current) => {
    if (key.length !== 1) return null;
    const now = Date.now();
    const state = buffer.current;
    state.text = now - state.at > WINDOW_MS ? key.toLowerCase() : state.text + key.toLowerCase();
    state.at = now;

    const all = labelsRef.current();
    for (let step = 1; step <= all.length; step += 1) {
      const i = (current + step + all.length) % all.length;
      if (all[i]?.toLowerCase().startsWith(state.text)) return i;
    }
    return null;
  }, []);
}
