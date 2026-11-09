import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./styles/app.css";

async function boot(root: HTMLElement) {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("catalog")) {
    const { Catalog } = await import("./dev/catalog/Catalog");
    createRoot(root).render(
      <StrictMode>
        <Catalog />
      </StrictMode>,
    );
    return;
  }
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from index.html");
void boot(root);
