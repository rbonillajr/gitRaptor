// DS-US-DSK-005 D21: ⌥⌘T belongs to the graph view and is ignored while a text field has the focus.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { GraphView } from "./GraphView";
import { entryOf, modelOf, tienda, tiendaGraph } from "./fixtures";
import { useGraphPrefs } from "./graph-prefs";

beforeEach(async () => {
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Macintosh");
  useGraphPrefs.setState({ view: "graph" });
  await act(async () => void (await i18n.changeLanguage("en")));
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const chord = (target: Element | Document = document.body) => fireEvent.keyDown(target, { key: "t", code: "KeyT", metaKey: true, altKey: true });

describe("graph view shortcut", () => {
  it("is ignored while a text field has the focus", () => {
    render(
      <>
        <input aria-label="search" />
        <GraphView model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />
      </>,
    );
    chord(screen.getByLabelText("search"));
    expect(useGraphPrefs.getState().view).toBe("graph");
    chord();
    expect(useGraphPrefs.getState().view).toBe("table");
  });

  it("without the command key it does nothing", () => {
    render(<GraphView model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />);
    fireEvent.keyDown(document.body, { key: "t", code: "KeyT", altKey: true });
    fireEvent.keyDown(document.body, { key: "t", code: "KeyT", metaKey: true });
    expect(useGraphPrefs.getState().view).toBe("graph");
  });
});
