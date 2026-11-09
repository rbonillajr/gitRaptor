// DS-US-DSK-004 D10: the 1 s clock only runs while the window is visible.
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNow } from "./useNow";

function Clock() {
  return <output>{useNow()}</output>;
}
const shown = () => Number(screen.getByRole("status").textContent);

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
  act(() => void document.dispatchEvent(new Event("visibilitychange")));
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
  setVisibility("visible");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useNow", () => {
  it("ticks every second while visible", () => {
    render(<Clock />);
    expect(shown()).toBe(1_000_000);
    act(() => void vi.advanceTimersByTime(3000));
    expect(shown()).toBe(1_003_000);
  });

  it("useNow stops while hidden", () => {
    render(<Clock />);
    setVisibility("hidden");
    expect(vi.getTimerCount()).toBe(0);
    act(() => void vi.advanceTimersByTime(10_000));
    expect(shown()).toBe(1_000_000);
  });

  it("refreshes at once and resumes when visible again", () => {
    render(<Clock />);
    setVisibility("hidden");
    act(() => void vi.advanceTimersByTime(10_000));
    setVisibility("visible");
    expect(shown()).toBe(1_010_000);
    expect(vi.getTimerCount()).toBe(1);
    act(() => void vi.advanceTimersByTime(1000));
    expect(shown()).toBe(1_011_000);
  });

  it("clears its timer and listener on unmount", () => {
    const { unmount } = render(<Clock />);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("never starts a timer when mounted hidden", () => {
    setVisibility("hidden");
    render(<Clock />);
    expect(vi.getTimerCount()).toBe(0);
  });
});
