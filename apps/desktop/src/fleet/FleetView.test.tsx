// DS-US-DSK-004 T005 (§ 9.4, TS column that needs no IPC): the fleet in the centre, in en and es, with axe at 1440 and 1200 px.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, type MockInstance, vi } from "vitest";
import "@/i18n";
import type { EngineMessage, RepoFleet } from "@/client/engine-types";
import { App } from "@/app/App";
import { useEngineStore } from "@/state/engine-store";
import { queryClient } from "@/state/query-client";
import { useUiStore } from "@/state/ui-store";
import { setViewportWidth } from "../../tests/setup";
import { FleetView } from "./FleetView";
import { HOUR, MIN, NOW, counted, pair, ready, session, tienda, worktree } from "./fixtures";
import { useAgentSlotsSync } from "./useAgentSlots";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
/** The colour slots have one writer (`GraphContent` in the app); the view only reads them, so the test mounts the writer. */
function SlotsSync({ fleet }: { fleet: RepoFleet | undefined }) {
  useAgentSlotsSync(fleet);
  return null;
}
const view = (fleet: RepoFleet | undefined, nowMs = NOW) => (
  <>
    <SlotsSync fleet={fleet} />
    <FleetView fleet={fleet} nowMs={nowMs} activity />
  </>
);
const cardsOf = () => screen.getAllByRole("article");
const card = (name: RegExp | string) => screen.getByRole("article", { name });

beforeEach(async () => {
  setViewportWidth(1440);
  await setLang("en");
});
afterEach(cleanup);

describe("FleetView cards", () => {
  it("one card per worktree with agent, state, files and ahead/behind", () => {
    render(view(tienda()));
    expect(screen.getByRole("list", { name: "Worktrees of tienda" })).toBeInTheDocument();
    expect(cardsOf().map((c) => c.getAttribute("aria-label"))).toEqual([
      "main worktree, No agent · tienda, main, no changes, 0 ahead and 0 behind main, 2 min ago",
      "Claude Code · feat-login, active, feat-login, no changes, 1 ahead and 0 behind main, 2 min ago",
      "Claude Code · feat-pagos, active, feat-pagos, 2 changed files, 3 ahead and 1 behind main, 2 min ago",
    ]);
    const pagos = within(card(/feat-pagos, active/));
    expect(pagos.getByText("Claude Code · feat-pagos")).toBeInTheDocument();
    expect(pagos.getByText("active")).toBeInTheDocument();
    expect(pagos.getByText("2 changed files")).toBeInTheDocument();
    expect(pagos.getByText("3 ahead and 1 behind main")).toBeInTheDocument();
    expect(within(cardsOf()[0] as HTMLElement).getByText("main worktree")).toBeInTheDocument();
  });

  it("a commit in feat-pagos updates its card without reload", () => {
    const { rerender } = render(view(tienda()));
    expect(card(/feat-pagos/)).toHaveTextContent("3 ahead and 1 behind main");
    const next = tienda({
      worktrees: tienda().repo.worktrees.map((w) =>
        w.folder === "feat-pagos" ? { ...w, status: ready({ head: { kind: "branch", name: "feat-pagos" }, counts: { staged: 0, unstaged: 1, untracked: 0 }, divergence: counted(4, 1) }), lastActivityUtcMs: NOW } : w,
      ),
    });
    rerender(view(next));
    expect(card(/feat-pagos/)).toHaveTextContent("4 ahead and 1 behind main");
    expect(card(/feat-pagos/)).toHaveTextContent("1 changed file");
    expect(card(/feat-pagos/)).toHaveTextContent("0 s ago");
  });

  it("antiquity follows the clock", () => {
    const { rerender } = render(view(tienda()));
    expect(card(/feat-pagos/)).toHaveTextContent("2 min ago");
    rerender(view(tienda(), NOW + 3 * MIN));
    expect(card(/feat-pagos/)).toHaveTextContent("5 min ago");
  });

  it("a predicted conflict puts the card first with its reason in the danger row", () => {
    render(view(tienda({ pairs: [pair("feat-login", "feat-pagos")] })));
    const names = cardsOf().map((c) => c.getAttribute("aria-label") ?? "");
    expect(names[1]).toMatch(/^Claude Code · feat-login/);
    expect(names[1]).toContain("predicted conflict with feat-pagos");
    expect(within(cardsOf()[1] as HTMLElement).getByText("⚡ predicted conflict with feat-pagos")).toBeInTheDocument();
    expect(cardsOf()[1]).toHaveAttribute("data-attention", "true");
  });

  it("an ended session shows for 24 hours, then the last agent", () => {
    const ended = (ago: number) => tienda({ sessions: [session("claude-1", "feat-pagos", { state: "ended", endedUtcMs: NOW - ago, endCause: "process-gone" })] });
    const { rerender } = render(view(ended(2 * HOUR)));
    expect(card(/feat-pagos/)).toHaveTextContent("ended 2 h ago");
    rerender(view(ended(25 * HOUR)));
    expect(card(/feat-pagos/)).toHaveTextContent("No agent · feat-pagos");
    expect(card(/feat-pagos/)).toHaveTextContent("last agent: Claude Code (ended 1 d ago)");
  });

  it("a detached worktree shows 7 digits or no hash", () => {
    const fleet = tienda({
      worktrees: [
        worktree("one", { status: ready({ head: { kind: "detached" } }), detachedAt: "0123456789abcdef0123456789abcdef01234567" }),
        worktree("two", { status: ready({ head: { kind: "detached" } }) }),
      ],
      sessions: [],
    });
    render(view(fleet));
    const one = card(/^No agent · one/);
    expect(within(one).getByText("0123456").tagName).toBe("CODE");
    expect(one).toHaveTextContent("0123456 (no branch)");
    expect(card(/^No agent · two/)).toHaveTextContent("(no branch)");
    expect(within(card(/^No agent · two/)).queryByText(/^[0-9a-f]{7}$/)).toBeNull();
  });

  it("a missing base says not computable, never zero, in the title and the row", () => {
    const fleet = tienda({ worktrees: [worktree("w", { status: ready({ divergence: { state: "base-missing" } }) })], sessions: [] });
    render(view(fleet));
    expect(screen.getByRole("heading", { name: "Fleet · ↑↓ not computable: main not found" })).toBeInTheDocument();
    expect(card(/^No agent · w/)).toHaveTextContent("not computable");
    expect(card(/^No agent · w/)).not.toHaveTextContent(/\b0 ahead/);
  });

  it("an unavailable worktree shows why and no metrics", () => {
    const fleet = tienda({ worktrees: [worktree("gone", { status: { state: "unavailable", reason: "missing" } })], sessions: [] });
    render(view(fleet));
    const c = card(/gone/);
    expect(c).toHaveTextContent("unavailable: folder missing");
    expect(c).toHaveTextContent("⚠");
    expect(c).not.toHaveTextContent(/ahead|changed|ago|no changes/);
  });

  it("branch text is rendered as text", () => {
    const hostile = '<img src=x onerror="alert(1)"><b>x</b>';
    const fleet = tienda({ worktrees: [worktree("w", { status: ready({ head: { kind: "branch", name: hostile } }) })], sessions: [] });
    const { container } = render(view(fleet));
    expect(screen.getByText(hostile)).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
  });

  it("each live agent gets its own lane colour and keeps it when another one arrives", () => {
    const first = tienda();
    const { rerender } = render(view(first));
    const colorOf = (folder: string) => within(card(new RegExp(`feat-${folder}`))).getByText(new RegExp(`Claude Code · feat-${folder}`)).className;
    const [pagos, login] = [colorOf("pagos"), colorOf("login")];
    expect(pagos).toMatch(/text-lane-\d/);
    expect(login).toMatch(/text-lane-\d/);
    expect(pagos.match(/text-lane-\d/)?.[0]).not.toBe(login.match(/text-lane-\d/)?.[0]);

    const more = tienda({
      worktrees: [...first.repo.worktrees, worktree("aaa")],
      sessions: [...first.sessions.values(), session("claude-0", "aaa", { startedUtcMs: NOW - 10 * HOUR })],
    });
    rerender(view(more));
    expect(colorOf("pagos")).toBe(pagos);
    expect(colorOf("login")).toBe(login);
  });

  it("an ended or absent agent is muted and takes no lane colour", () => {
    render(view(tienda({ sessions: [] })));
    for (const c of cardsOf()) expect(c.innerHTML).not.toMatch(/text-lane-/);
  });

  it("marks a shared worktree", () => {
    const fleet = tienda({ sessions: [session("claude-1", "feat-pagos"), session("claude-2", "feat-pagos", { agent: { kind: "other", name: "aider" } })] });
    render(view(fleet));
    expect(within(card(/feat-pagos/)).getByText("shared")).toBeInTheDocument();
    expect(card(/feat-pagos/).getAttribute("aria-label")).toContain("+1 shared");
  });
});

describe("FleetView title", () => {
  it("shows the base and how long ago the fetch was", () => {
    render(view(tienda()));
    expect(screen.getByRole("heading", { name: "Fleet · ↑↓ vs main (local copy, fetched 3 h ago)" })).toBeInTheDocument();
  });

  it("says never fetched", () => {
    render(view(tienda({ repo: { fetchedUtcMs: null } })));
    expect(screen.getByRole("heading", { name: "Fleet · ↑↓ vs main (local copy, never fetched)" })).toBeInTheDocument();
  });

  it("speaks Spanish", async () => {
    await setLang("es");
    render(view(tienda()));
    expect(screen.getByRole("heading", { name: "Flota · ↑↓ respecto a main (copia local, fetch hace 3 h)" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Copias de trabajo de tienda" })).toBeInTheDocument();
  });
});

describe("FleetView empty and loading states", () => {
  it("a repo without agents shows its main worktree and says no agent works there", () => {
    const fleet = tienda({ worktrees: [worktree("notas", { main: true, path: "/w/notas" })], sessions: [], repo: { repoId: "notas", name: "notas" } });
    render(view(fleet));
    expect(screen.getByText("No agents are working in notas yet. Open Claude Code in a worktree and it shows up here.")).toBeInTheDocument();
    expect(cardsOf()).toHaveLength(1);
    expect(cardsOf()[0]).toHaveTextContent("main worktree");
    expect(cardsOf()[0]).toHaveTextContent("No agent · notas");
  });

  it("the notice disappears when an agent is present or just ended", () => {
    const { rerender } = render(view(tienda({ sessions: [] })));
    expect(screen.getByText(/No agents are working in tienda/)).toBeInTheDocument();
    rerender(view(tienda({ sessions: [session("claude-1", "feat-pagos", { state: "inactive" })] })));
    expect(screen.queryByText(/No agents are working/)).toBeNull();
    rerender(view(tienda({ sessions: [session("claude-1", "feat-pagos", { state: "ended", endedUtcMs: NOW - HOUR, endCause: "process-gone" })] })));
    expect(screen.queryByText(/No agents are working/)).toBeNull();
  });

  it.each([null, false] as const)("unknown detection (%s) never says no agent", (detection) => {
    render(view(tienda({ sessions: [], detection })));
    expect(screen.queryByText(/No agents are working/)).toBeNull();
    expect(screen.queryByText(/No agent ·/)).toBeNull();
    expect(screen.getAllByText("agent not available")).toHaveLength(3);
  });

  it("without the repo snapshot shows three skeletons and is busy, never no agents", () => {
    const { container } = render(view(undefined));
    expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Loading the fleet…")).toBeInTheDocument();
    expect(container.querySelector("[aria-busy=true]")).not.toBeNull();
    expect(container.querySelectorAll("[aria-hidden=true]")).toHaveLength(3);
    expect(screen.queryByText(/No agents are working/)).toBeNull();
    expect(screen.queryByRole("article")).toBeNull();
  });
});

describe("FleetView attention announcement", () => {
  const announcement = () => screen.getByRole("status");

  it("announces a card that enters the attention group, not the ones already there", () => {
    const { rerender } = render(view(tienda({ pairs: [pair("feat-login", "feat-pagos")] })));
    expect(announcement()).toBeEmptyDOMElement();
    rerender(view(tienda({ pairs: [pair("feat-login", "feat-pagos")] }), NOW + 1000));
    expect(announcement()).toBeEmptyDOMElement();
    rerender(view(tienda({ pairs: [pair("feat-login", "feat-pagos"), pair("feat-login", "base")] })));
    expect(announcement()).toBeEmptyDOMElement();

    rerender(view(tienda()));
    rerender(view(tienda({ pairs: [pair("feat-pagos", "base")] })));
    expect(announcement()).toHaveTextContent("feat-pagos needs attention: predicted conflict");
  });

  it("announces in Spanish", async () => {
    await setLang("es");
    const { rerender } = render(view(tienda()));
    rerender(view(tienda({ pairs: [pair("feat-login", "base")] })));
    expect(announcement()).toHaveTextContent("feat-login pide atención: conflicto previsto");
  });

  it("does not announce when the repo changes", () => {
    const { rerender } = render(view(tienda()));
    const other = tienda({ repo: { repoId: "otra", name: "otra" }, pairs: [pair("feat-login", "base")] });
    rerender(view(other));
    expect(announcement()).toBeEmptyDOMElement();
  });
});

describe("FleetView accessibility", () => {
  const fixtures: Array<[string, RepoFleet | undefined]> = [
    ["loading", undefined],
    ["agents", tienda({ pairs: [pair("feat-login", "feat-pagos")] })],
    ["no agents", tienda({ sessions: [] })],
    ["unknown detection", tienda({ sessions: [], detection: null })],
    ["unavailable and detached", tienda({ worktrees: [worktree("gone", { status: { state: "unavailable", reason: "untrusted" } }), worktree("d", { status: ready({ head: { kind: "detached" } }), detachedAt: "abcdef0123" })] })],
  ];

  it.each(["en", "es"] as const)("axe finds nothing at 1440 and 1200 px in %s", async (lang) => {
    await setLang(lang);
    for (const width of [1440, 1200]) {
      setViewportWidth(width);
      for (const [name, fleet] of fixtures) {
        const { container, unmount } = render(view(fleet));
        const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
        expect(results.violations.map((v) => `${name} ${width} ${lang}: ${v.id}: ${v.help}`)).toEqual([]);
        unmount();
      }
    }
  });

  it("every card has a name and either a glyph or a state text (not colour alone)", () => {
    render(view(tienda({ pairs: [pair("feat-login", "base")] })));
    for (const c of cardsOf()) {
      expect(c.getAttribute("aria-label")).toBeTruthy();
      expect(c.textContent?.trim()).not.toBe("");
    }
    expect(card(/feat-login/)).toHaveTextContent(/●.*active/);
  });
});

// D4-FRESH (DS-US-DSK-004 T006): after the React commit that applied a batch with repo messages, in the first
// requestAnimationFrame the window measures `gr:fleet-delta` from the batch's `receivedAt` and marks
// `gr:fleet-delta-painted`: once per batch, never per message, never on a re-render. Driven through `App` with a
// fake `Channel`, because the plan does not say how the batch reaches `FleetView`.
describe("D4-FRESH fleet delta marks", () => {
  type WatchedChannel = { onmessage: (message: EngineMessage) => void };
  let channel: WatchedChannel | undefined;
  let measure: MockInstance<typeof performance.measure>;
  let mark: MockInstance<typeof performance.mark>;
  const deltas = () => measure.mock.calls.filter(([name]) => name === "gr:fleet-delta");
  const painted = () => mark.mock.calls.filter(([name]) => name === "gr:fleet-delta-painted");
  const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const frames = async () => {
    for (let i = 0; i < 3; i += 1) await frame();
  };
  const send = (...batch: EngineMessage[]) =>
    act(() => {
      for (const message of batch) channel?.onmessage(message);
    });
  const perf = { backendNs: 1_000_000 };
  const row = {
    repoId: "tienda",
    name: "tienda",
    path: "/w/tienda/.git",
    state: "observed" as const,
    tier: "active" as const,
    attention: { conflicts: { state: "counted" as const, count: 0 }, denials: { state: "counted" as const, count: 0 } },
  };

  beforeEach(() => {
    channel = undefined;
    queryClient.clear();
    useEngineStore.setState({ conn: { kind: "connecting" }, staleSinceMs: null, resync: null, session: null });
    useUiStore.setState({ selectedRepoId: null });
    measure = vi.spyOn(performance, "measure");
    mark = vi.spyOn(performance, "mark");
    mockIPC((cmd, args) => {
      if (cmd === "engine_watch") channel = (args as { params: { channel: WatchedChannel } }).params.channel;
      if (cmd === "app_info") return { version: "0.0.0", protocol: 9 };
      return {};
    });
  });
  afterEach(() => {
    cleanup();
    clearMocks();
    vi.restoreAllMocks();
  });

  it("measures each fleet delta once", async () => {
    render(<App />);
    await waitFor(() => expect(channel, "App must call engine_watch with a Channel").toBeDefined());
    await send(
      { type: "session", daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true },
      { type: "global", engine: { state: "observing", gitVersion: "2.45.1" }, daemon: { version: "0.1.0", protocol: 9 }, autostart: "unknown", repos: [row] },
      { type: "conn", state: { kind: "live" }, staleSinceMs: null, resync: null },
    );
    await waitFor(() => expect(useUiStore.getState().selectedRepoId, "the repo is opened by default").toBe("tienda"));
    await send({ type: "repo", fleet: tienda().repo });
    await screen.findByRole("list", { name: "Worktrees of tienda" });
    await frames();
    const baseDeltas = deltas().length;
    const basePainted = painted().length;

    // One batch of three repo messages: one measure, one painted mark.
    const base = tienda().repo;
    await send(
      { type: "worktrees", repoId: "tienda", worktrees: base.worktrees, fetchedUtcMs: 1, perf },
      { type: "repoSession", repoId: "tienda", session: session("claude-1", "feat-pagos"), perf },
      { type: "prediction", repoId: "tienda", pairs: [], removed: [], perf },
    );
    await waitFor(() => expect(deltas(), "one gr:fleet-delta measure for the batch").toHaveLength(baseDeltas + 1));
    await waitFor(() => expect(painted(), "and its gr:fleet-delta-painted mark").toHaveLength(basePainted + 1));
    const last = deltas().at(-1);
    expect(last?.[1], "the measure starts at the receivedAt of the batch").toEqual({ start: expect.any(Number) });
    expect((last?.[1] as { start: number }).start, "a time of this page, in the past").toBeLessThanOrEqual(performance.now());
    await frames();
    expect(deltas(), "three messages, still one measure").toHaveLength(baseDeltas + 1);

    // A re-render with no new batch (the clock ticking) measures nothing.
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await frames();
    expect(deltas(), "a re-render is not a delta").toHaveLength(baseDeltas + 1);
    expect(painted(), "nor a painted mark").toHaveLength(basePainted + 1);

    // The next batch is a new delta.
    await send({ type: "worktrees", repoId: "tienda", worktrees: base.worktrees, fetchedUtcMs: 2, perf });
    await waitFor(() => expect(deltas(), "the next batch measures once more").toHaveLength(baseDeltas + 2));
    await waitFor(() => expect(painted()).toHaveLength(basePainted + 2));

    // A batch without repo messages is not a fleet delta.
    await send({ type: "repoTier", repoId: "tienda", tier: "dormant" });
    await frames();
    expect(deltas(), "a global message measures nothing").toHaveLength(baseDeltas + 2);
  });
});
