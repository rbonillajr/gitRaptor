/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-HOOKS (TS-DSK-003 T003): focus trap, roving focus, dismiss, anchored position and type-ahead.
// Signatures and behaviour: Dev Spec § Hooks.
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAnchoredPosition } from "./useAnchoredPosition";
import { useDismiss } from "./useDismiss";
import { useFocusTrap } from "./useFocusTrap";
import { useRovingFocus, type RovingOptions } from "./useRovingFocus";
import { useTypeahead } from "./useTypeahead";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const focused = () => document.activeElement;

function Trap({ active, initial }: { active: boolean; initial?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref, active, initial ? { initialFocus: () => document.getElementById("second") } : {});
  return (
    <div>
      <button type="button" id="outside">outside</button>
      <div ref={ref}>
        <button type="button" id="first">first</button>
        <button type="button" id="second">second</button>
        <button type="button" id="last">last</button>
      </div>
    </div>
  );
}

describe("TS003-HOOKS useFocusTrap", () => {
  it("TS003-HOOKS: activating focuses the first focusable and Tab / Shift+Tab cycle inside", () => {
    const { rerender } = render(<Trap active={false} />);
    act(() => document.getElementById("outside")!.focus());
    rerender(<Trap active />);
    expect(focused()?.id, "the trap must focus the first focusable on activation").toBe("first");
    act(() => document.getElementById("last")!.focus());
    fireEvent.keyDown(focused()!, { key: "Tab" });
    expect(focused()?.id, "Tab on the last element must wrap to the first").toBe("first");
    fireEvent.keyDown(focused()!, { key: "Tab", shiftKey: true });
    expect(focused()?.id, "Shift+Tab on the first element must wrap to the last").toBe("last");
  });

  it("TS003-HOOKS: initialFocus wins and deactivating returns the focus to the previous element", () => {
    const { rerender } = render(<Trap active={false} initial />);
    act(() => document.getElementById("outside")!.focus());
    rerender(<Trap active initial />);
    expect(focused()?.id, "initialFocus() must receive the focus").toBe("second");
    rerender(<Trap active={false} initial />);
    expect(focused()?.id, "the trap must return the focus to the element active before it").toBe("outside");
  });

  it("TS003-HOOKS: an element with data-autofocus is focused before the first focusable", () => {
    function Auto() {
      const ref = useRef<HTMLDivElement>(null);
      useFocusTrap(ref, true);
      return (
        <div ref={ref}>
          <button type="button" id="a">a</button>
          <button type="button" id="b" data-autofocus>b</button>
        </div>
      );
    }
    render(<Auto />);
    expect(focused()?.id, "[data-autofocus] must take the initial focus").toBe("b");
  });
});

function Roving(props: { options: RovingOptions; disabledIndex?: number; probe?: (api: ReturnType<typeof useRovingFocus>) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const api = useRovingFocus(ref, "[data-item]", props.options);
  props.probe?.(api);
  return (
    <div ref={ref} role="toolbar" aria-label="t" onKeyDown={(e) => api.onKeyDown(e as never)}>
      {["a", "b", "c", "d"].map((id, i) => (
        <button key={id} type="button" id={id} data-item aria-disabled={i === props.disabledIndex ? "true" : undefined}>
          {id}
        </button>
      ))}
    </div>
  );
}
const press = (key: string) => fireEvent.keyDown(focused()!, { key });

describe("TS003-HOOKS useRovingFocus", () => {
  it("TS003-HOOKS: arrows move the focus with wrap-around when loop is true", () => {
    const onMove = vi.fn();
    render(<Roving options={{ orientation: "vertical", loop: true, skipDisabled: true, onMove }} />);
    act(() => document.getElementById("a")!.focus());
    press("ArrowDown");
    expect(focused()?.id, "ArrowDown must move to the next item").toBe("b");
    expect(onMove, "onMove must report the new index").toHaveBeenCalledWith(1);
    press("ArrowUp");
    press("ArrowUp");
    expect(focused()?.id, "ArrowUp on the first item must wrap to the last when loop").toBe("d");
    press("ArrowDown");
    expect(focused()?.id, "ArrowDown on the last item must wrap to the first when loop").toBe("a");
  });

  it("TS003-HOOKS: without loop the focus stops at the ends; Home and End jump to them", () => {
    render(<Roving options={{ orientation: "vertical", loop: false, skipDisabled: false }} />);
    act(() => document.getElementById("a")!.focus());
    press("ArrowUp");
    expect(focused()?.id, "no wrap without loop").toBe("a");
    press("End");
    expect(focused()?.id, "End goes to the last item").toBe("d");
    press("ArrowDown");
    expect(focused()?.id, "no wrap without loop").toBe("d");
    press("Home");
    expect(focused()?.id, "Home goes to the first item").toBe("a");
  });

  it("TS003-HOOKS: skipDisabled jumps over aria-disabled items, and false lands on them", () => {
    const { unmount } = render(<Roving disabledIndex={1} options={{ orientation: "vertical", loop: true, skipDisabled: true }} />);
    act(() => document.getElementById("a")!.focus());
    press("ArrowDown");
    expect(focused()?.id, "the disabled item b must be skipped").toBe("c");
    unmount();
    render(<Roving disabledIndex={1} options={{ orientation: "vertical", loop: true, skipDisabled: false }} />);
    act(() => document.getElementById("a")!.focus());
    press("ArrowDown");
    expect(focused()?.id, "with skipDisabled false the disabled item is focusable").toBe("b");
  });

  it("TS003-HOOKS: orientation decides which arrows move; horizontal ignores ArrowDown", () => {
    render(<Roving options={{ orientation: "horizontal", loop: true, skipDisabled: true }} />);
    act(() => document.getElementById("a")!.focus());
    press("ArrowDown");
    expect(focused()?.id, "ArrowDown must not move a horizontal group").toBe("a");
    press("ArrowRight");
    expect(focused()?.id, "ArrowRight moves a horizontal group").toBe("b");
  });

  it("TS003-HOOKS: focusIndex focuses an item and leaves a single Tab stop", () => {
    let api!: ReturnType<typeof useRovingFocus>;
    render(<Roving options={{ orientation: "both", loop: true, skipDisabled: true }} probe={(a) => (api = a)} />);
    act(() => api.focusIndex(2));
    expect(focused()?.id, "focusIndex(2) must focus the third item").toBe("c");
    const stops = screen.getAllByRole("button").filter((b) => b.tabIndex === 0);
    expect(stops.map((b) => b.id), "exactly one item keeps tabindex=0 (the active one)").toEqual(["c"]);
    expect(api.activeIndex, "activeIndex follows the focus").toBe(2);
  });
});

function Dismiss(props: { active?: boolean; onDismiss: (r: string) => void; options?: Parameters<typeof useDismiss>[3] }) {
  const a = useRef<HTMLDivElement>(null);
  const b = useRef<HTMLDivElement>(null);
  useDismiss([a, b], props.active ?? true, props.onDismiss, props.options);
  return (
    <div>
      <button type="button" id="far">far</button>
      <div ref={a}><button type="button" id="in-a">a</button></div>
      <div ref={b}><button type="button" id="in-b">b</button></div>
    </div>
  );
}

describe("TS003-HOOKS useDismiss", () => {
  it("TS003-HOOKS: Escape calls onDismiss('escape')", () => {
    const onDismiss = vi.fn();
    render(<Dismiss onDismiss={onDismiss} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onDismiss, "Escape must dismiss").toHaveBeenCalledWith("escape");
  });

  it("TS003-HOOKS: pointerdown outside dismisses; inside any listed element does not", () => {
    const onDismiss = vi.fn();
    render(<Dismiss onDismiss={onDismiss} />);
    fireEvent.pointerDown(document.getElementById("in-a")!);
    fireEvent.pointerDown(document.getElementById("in-b")!);
    expect(onDismiss, "pointerdown inside an excluded element must not dismiss").not.toHaveBeenCalled();
    fireEvent.pointerDown(document.getElementById("far")!);
    expect(onDismiss, "pointerdown outside must dismiss").toHaveBeenCalledWith("pointer-outside");
  });

  it("TS003-HOOKS: inactive hook and unmounted hook do not call onDismiss", () => {
    const onDismiss = vi.fn();
    const { rerender, unmount } = render(<Dismiss active={false} onDismiss={onDismiss} />);
    fireEvent.keyDown(document, { key: "Escape" });
    rerender(<Dismiss active onDismiss={onDismiss} />);
    unmount();
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.pointerDown(document.body);
    expect(onDismiss, "listeners must be removed when inactive or unmounted").not.toHaveBeenCalled();
  });

  it("TS003-HOOKS: options turn each reason off; focusOutside only fires when enabled", () => {
    const onDismiss = vi.fn();
    render(<Dismiss onDismiss={onDismiss} options={{ escape: false, pointerOutside: false }} />);
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.pointerDown(document.body);
    act(() => document.getElementById("in-a")!.focus());
    fireEvent.focusOut(document.getElementById("in-a")!, { relatedTarget: document.getElementById("far") });
    expect(onDismiss, "disabled reasons and focusOutside default must not fire").not.toHaveBeenCalled();
    cleanup();
    render(<Dismiss onDismiss={onDismiss} options={{ focusOutside: true }} />);
    act(() => document.getElementById("in-a")!.focus());
    fireEvent.focusOut(document.getElementById("in-a")!, { relatedTarget: document.getElementById("far") });
    expect(onDismiss, "focus moving outside must dismiss when focusOutside").toHaveBeenCalledWith("focus-outside");
  });
});

function rect(left: number, top: number, width: number, height: number): DOMRect {
  return { left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) } as DOMRect;
}

function Anchored(props: { anchorRect: DOMRect; placement: Parameters<typeof useAnchoredPosition>[3]["placement"] }) {
  const anchor = useRef<HTMLButtonElement>(null);
  const floating = useRef<HTMLDivElement>(null);
  const [open] = useState(true);
  useAnchoredPosition(anchor, floating, open, { placement: props.placement, offset: 8, padding: 16 });
  return (
    <>
      <button type="button" ref={(el) => { anchor.current = el; if (el) el.getBoundingClientRect = () => props.anchorRect; }}>anchor</button>
      <div
        data-testid="floating"
        ref={(el) => {
          floating.current = el;
          if (el) el.getBoundingClientRect = () => rect(0, 0, 200, 100);
        }}
      />
    </>
  );
}

describe("TS003-HOOKS useAnchoredPosition", () => {
  it("TS003-HOOKS: places below the anchor with position fixed and data-side=bottom", () => {
    vi.stubGlobal("innerWidth", 1024);
    vi.stubGlobal("innerHeight", 768);
    render(<Anchored anchorRect={rect(50, 100, 100, 30)} placement="bottom-start" />);
    const el = screen.getByTestId("floating");
    expect(el.style.position, "the floating element must be position: fixed").toBe("fixed");
    expect(el.getAttribute("data-side"), "enough room below: side bottom").toBe("bottom");
    expect(el.style.top, "top = anchor bottom + offset").toBe("138px");
    expect(el.style.left, "bottom-start aligns to the anchor's left edge").toBe("50px");
    vi.unstubAllGlobals();
  });

  it("TS003-HOOKS: flips to the top when it does not fit below", () => {
    vi.stubGlobal("innerWidth", 1024);
    vi.stubGlobal("innerHeight", 768);
    render(<Anchored anchorRect={rect(50, 700, 100, 30)} placement="bottom-start" />);
    const el = screen.getByTestId("floating");
    expect(el.getAttribute("data-side"), "no room below: flip to top").toBe("top");
    expect(el.style.top, "top = anchor top - offset - floating height").toBe("592px");
    vi.unstubAllGlobals();
  });

  it("TS003-HOOKS: shifts horizontally to stay inside the padding", () => {
    vi.stubGlobal("innerWidth", 1024);
    vi.stubGlobal("innerHeight", 768);
    render(<Anchored anchorRect={rect(1000, 100, 20, 30)} placement="bottom-start" />);
    expect(screen.getByTestId("floating").style.left, "left clamps to innerWidth - padding - width").toBe("808px");
    vi.unstubAllGlobals();
  });
});

describe("TS003-HOOKS useTypeahead", () => {
  it("TS003-HOOKS: two keys within 500 ms search for the combined prefix, case-insensitively", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useTypeahead(() => ["Pull", "Fetch", "Push"]));
    expect(result.current("F", -1), "'f' matches Fetch").toBe(1);
    vi.advanceTimersByTime(200);
    expect(result.current("e", -1), "'fe' still matches Fetch").toBe(1);
    expect(result.current("z", -1), "'fez' matches nothing").toBeNull();
  });

  it("TS003-HOOKS: the buffer resets after 500 ms and the search starts after the current index", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useTypeahead(() => ["Fetch", "Pull", "Feature"]));
    expect(result.current("f", -1)).toBe(0);
    vi.advanceTimersByTime(600);
    expect(result.current("e", -1), "after 500 ms the prefix restarts: 'e' matches nothing").toBeNull();
    vi.advanceTimersByTime(600);
    expect(result.current("f", 0), "the search begins after the current index").toBe(2);
  });
});
