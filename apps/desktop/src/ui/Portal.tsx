import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

const PortalContext = createContext<HTMLElement | null>(null);

/** Sets where overlays render. Without it they go to `document.body`; the dev catalog gives each theme panel its own. */
export function UiPortalProvider({ container, children }: { container: HTMLElement | null; children: ReactNode }) {
  return <PortalContext value={container}>{children}</PortalContext>;
}

/** The provider's container, or `document.body`. */
export function usePortalContainer(): HTMLElement {
  return useContext(PortalContext) ?? document.body;
}

/** Renders `children` in its own `<div data-gr-portal>` inside the container; removes it on unmount. */
export function Portal({ children }: { children: ReactNode }) {
  const container = usePortalContainer();
  const [node, setNode] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    const el = document.createElement("div");
    el.setAttribute("data-gr-portal", "");
    container.appendChild(el);
    setNode(el);
    return () => {
      el.remove();
      setNode(null);
    };
  }, [container]);

  return node ? createPortal(children, node) : null;
}
