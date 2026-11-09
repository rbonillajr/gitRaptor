import { useEffect, useState } from "react";

/** `false` on the first commit, `true` from the next animation frame: an overlay mounts at `opacity-0` and fades in. */
export function useFadeIn(): boolean {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const frame = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(frame);
  }, []);
  return shown;
}
