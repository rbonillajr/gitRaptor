// DS-US-DSK-005 T006 (D6): the colour slots of the agents have one writer and are read by the fleet and the graph.
import { act, cleanup, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { RepoFleet } from "@/client/engine-types";
import { FleetView } from "./FleetView";
import { HOUR, NOW, session, tienda, worktree } from "./fixtures";
import { useAgentSlot, useAgentSlotsSync } from "./useAgentSlots";

afterEach(cleanup);

/** Empties the registry the way a repo change does. */
beforeEach(() => {
  renderHook(() => useAgentSlotsSync(tienda({ repo: { repoId: "reset" }, sessions: [] }))).unmount();
});

describe("useAgentSlots", () => {
  it("gives each present session the lowest free slot in start order", () => {
    const { result } = renderHook(() => {
      useAgentSlotsSync(tienda());
      return [useAgentSlot("claude-1"), useAgentSlot("claude-2")];
    });
    expect(result.current).toEqual([1, 2]);
  });

  it("a session keeps its slot when another one arrives, and an unknown one has none", () => {
    const first = tienda();
    const more = tienda({
      worktrees: [...first.repo.worktrees, worktree("aaa")],
      sessions: [...first.sessions.values(), session("claude-0", "aaa", { startedUtcMs: NOW - 10 * HOUR })],
    });
    const { result, rerender } = renderHook(({ fleet }: { fleet: RepoFleet }) => {
      useAgentSlotsSync(fleet);
      return [useAgentSlot("claude-1"), useAgentSlot("claude-0"), useAgentSlot("nobody")];
    }, { initialProps: { fleet: first } });
    expect(result.current).toEqual([1, null, null]);
    rerender({ fleet: more });
    expect(result.current[0], "the first agent keeps its colour").toBe(1);
    expect(result.current[1], "the new one takes the next free slot").toBe(3);
  });

  it("an ended session gives its slot back", () => {
    const fleet = tienda();
    const ended = tienda({ sessions: [session("claude-1", "feat-pagos", { state: "ended", endedUtcMs: NOW }), session("claude-2", "feat-login")] });
    const { result, rerender } = renderHook(({ f }: { f: RepoFleet }) => {
      useAgentSlotsSync(f);
      return useAgentSlot("claude-1");
    }, { initialProps: { f: fleet } });
    expect(result.current).toBe(1);
    rerender({ f: ended });
    expect(result.current).toBeNull();
  });

  it("the registry is forgotten when the repo changes", () => {
    const { result, rerender } = renderHook(({ f }: { f: RepoFleet }) => {
      useAgentSlotsSync(f);
      return useAgentSlot("claude-2");
    }, { initialProps: { f: tienda({ sessions: [session("claude-1", "feat-pagos"), session("claude-2", "feat-login")] }) } });
    expect(result.current).toBe(2);
    rerender({ f: tienda({ repo: { repoId: "notas" }, sessions: [session("claude-2", "feat-login")] }) });
    expect(result.current, "in the new repo it is the first agent").toBe(1);
  });

  it("opening the detail does not reassign colour slots", () => {
    const fleet = tienda();
    // The center writes the slots; the detail only reads them: mounting its FleetView changes no slot.
    const center = renderHook(() => {
      useAgentSlotsSync(fleet);
      return [useAgentSlot("claude-1"), useAgentSlot("claude-2")];
    });
    const before = [...center.result.current];
    act(() => void render(<FleetView fleet={fleet} nowMs={NOW} activity />));
    expect(screen.getAllByRole("article")).toHaveLength(3);
    center.rerender();
    expect(center.result.current).toEqual(before);
  });
});
