/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// D7-VIEW (DS-US-DSK-007 T005, D11, D13, D15, § 9.4): the Guardrails center view and the toast of a new block. The view
// reads the cache entry of the chosen repo (engineKeys.guard); the toast is raised by EngineCenter, which mounts
// useGuardBlocks, so those tests mount EngineCenter inside a ToastProvider and feed it channel messages.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { BlockEntryDto, EngineMessage, PendingExceptionDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { EngineCenter } from "@/engine/EngineCenter";
import { tienda } from "@/fleet/fixtures";
import { applyEngineBatch, type GuardBlocks } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { ToastProvider } from "@/ui";
import { GuardrailsView } from "./GuardrailsView";

const NOW = Date.UTC(2026, 10, 9, 12, 0, 0);
const CMD = "'/Applications/GitRaptor.app/Contents/MacOS/raptor' 'guard' 'exec' '--' 'git' 'push' 'origin' 'feat-pagos:main'";
const entry = (id: string, over: Partial<BlockEntryDto> = {}): BlockEntryDto => ({
  decisionId: id,
  kind: "denial",
  atMs: NOW - 60_000,
  lastMs: NOW - 60_000,
  count: 1,
  actor: { kind: "claude-code" },
  worktree: { folder: "feat-pagos", path: "/w/tienda/feat-pagos" },
  branch: "feat-pagos",
  operation: { kind: "push", remote: "origin", refs: [{ name: "main", change: "force" }] },
  rules: [{ rule: "minimum.force-push", level: "minimum", cause: null }],
  layer: "hooks",
  detail: "full",
  exception: { state: "offered", command: CMD },
  ...over,
});
type Blocks = Extract<EngineMessage, { type: "guardBlocks" }>["blocks"];
const blocks = (entries: BlockEntryDto[], over: Partial<Blocks> = {}): Blocks => ({
  repoId: "tienda",
  protection: { state: "full" },
  sinceMs: NOW - 7 * 86_400_000,
  summary: { blocked: entries.filter((e) => e.kind === "denial").length, exceptions: entries.filter((e) => e.kind === "exception").length, exceptionsRejected: 0 },
  entries,
  pending: null,
  unlogged: [],
  ...over,
});
const readyOf = (b: Blocks): GuardBlocks => ({ state: "ready", ...b });

let client: QueryClient;
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const seed = (value: GuardBlocks) => act(() => void client.setQueryData(engineKeys.guard("tienda"), value));
const flush = (ms = 0) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));

interface ViewProps {
  live?: boolean;
  onBack?: () => void;
  onOpenRow?: (row: { id: string }) => void;
  copied?: Parameters<typeof GuardrailsView>[0]["copied"];
}
function showView(props: ViewProps = {}) {
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <GuardrailsView
          repoName="tienda"
          live={props.live ?? true}
          onBack={props.onBack ?? (() => {})}
          onOpenRow={(props.onOpenRow ?? (() => {})) as never}
          copied={props.copied ?? null}
        />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  vi.setSystemTime(NOW);
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  useEngineStore.setState({
    conn: { kind: "live" },
    staleSinceMs: null,
    resync: null,
    session: { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true },
  });
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "guardrails" } });
  mockIPC(() => ({}));
  document.documentElement.removeAttribute("data-theme");
  await setLang("en");
});
afterEach(() => {
  cleanup();
  clearMocks();
  vi.useRealTimers();
});

describe("D7-VIEW GuardrailsView", () => {
  it("D7-VIEW: lists the block with time, agent, operation and rule", async () => {
    seed(readyOf(blocks([entry("d1")])));
    showView();
    await flush();
    const rowButton = screen.getByRole("button", { name: /Blocked: Claude Code tried to rewrite the history of main on the server \(origin\)/ });
    expect(rowButton, "the row is a button named by its visible phrase").toBeInTheDocument();
    const text = document.body.textContent ?? "";
    expect(text, "the time").toMatch(/\d{1,2}:\d{2}/);
    expect(text, "the actor with its folder").toMatch(/Claude Code · feat-pagos|feat-pagos/);
    expect(text, "the rule").toMatch(/no force-push/);
  });

  it("D7-VIEW: a click on a row hands it to onOpenRow", async () => {
    seed(readyOf(blocks([entry("d1")])));
    const onOpenRow = vi.fn();
    showView({ onOpenRow });
    await flush();
    fireEvent.click(screen.getByRole("button", { name: /Blocked: Claude Code tried to rewrite/ }));
    expect(onOpenRow, "opens the dialog of that row").toHaveBeenCalledWith(expect.objectContaining({ id: "d1" }));
  });

  it("D7-VIEW: the header names the repo and goes back to the fleet", async () => {
    seed(readyOf(blocks([entry("d1")])));
    const onBack = vi.fn();
    showView({ onBack });
    await flush();
    expect(screen.getByRole("heading", { name: /tienda/ }), "the header names the repo").toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back to the fleet" }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("D7-VIEW: Esc goes back", async () => {
    seed(readyOf(blocks([entry("d1")])));
    const onBack = vi.fn();
    showView({ onBack });
    await flush();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape", code: "Escape" });
    expect(onBack, "Esc returns to the fleet").toHaveBeenCalledTimes(1);
  });

  it("D7-VIEW: the summary says how many were blocked and the exceptions", async () => {
    seed(readyOf(blocks([entry("d1"), entry("d2"), entry("d3"), entry("e1", { kind: "exception", exception: null })])));
    showView();
    await flush();
    expect(document.body.textContent, "grd.summary").toMatch(/3 blocked in the last \d+ days · 1 exceptions/);
  });

  it("D7-VIEW: shows the pending exception with its countdown", async () => {
    const pending: PendingExceptionDto = {
      appliesAtMs: NOW + 10_000,
      windowMs: 10_000,
      operation: { kind: "push", remote: "origin", refs: [{ name: "main", change: "force" }] },
      rules: ["minimum.force-push"],
      cancelCommand: "'raptor' 'guard' 'cancel' '/w/tienda'",
    };
    seed(readyOf(blocks([entry("d1")], { pending })));
    showView();
    await flush();
    expect(document.body.textContent, "the announced exception with the seconds left").toMatch(/Exception announced in a terminal: runs in 10 s/);
  });

  it("D7-VIEW: shows the exception once it is logged", async () => {
    seed(readyOf(blocks([entry("d1"), entry("e1", { kind: "exception", exception: null, lastMs: NOW - 1_000, atMs: NOW - 1_000 })])));
    showView();
    await flush();
    expect(document.body.textContent, "grd.exc.done with the operation and the skipped rules").toMatch(/Done with an exception: .*main.*; skipped .*force-push/);
  });

  it("D7-VIEW: a cancelled exception says nothing ran and stays in the log", async () => {
    seed(readyOf(blocks([entry("c1", { kind: "exception-cancelled", exception: null })])));
    showView();
    await flush();
    expect(document.body.textContent, "grd.exc.cancelled").toContain("Cancelled: nothing ran. It's in the log.");
  });

  it("D7-VIEW: a rate-limited block goes in the group logged without detail", async () => {
    seed(readyOf(blocks([entry("r1", { detail: "rate-limited", exception: { state: "not-offered", reason: "rate-limited" } })])));
    showView();
    await flush();
    expect(screen.getByText("Logged without detail (too many at once)"), "grd.unverified group").toBeInTheDocument();
  });

  it("D7-VIEW: periods without a log are told", async () => {
    seed(readyOf(blocks([entry("d1")], { unlogged: [{ fromMs: NOW - 3 * 3_600_000, toMs: NOW - 2 * 3_600_000 }] })));
    showView();
    await flush();
    expect(document.body.textContent, "grd.unlogged").toContain("the engine wasn't running: what the hooks blocked then isn't logged");
  });

  it("D7-VIEW: nothing blocked with protection says protection is on", async () => {
    seed(readyOf(blocks([])));
    showView();
    await flush();
    expect(screen.getByText("Nothing blocked in tienda. Protection is on."), "grd.empty").toBeInTheDocument();
  });

  it("D7-VIEW: nothing blocked without protection says it is not protected and offers no button", async () => {
    seed(readyOf(blocks([], { protection: { state: "unprotected" } })));
    showView();
    await flush();
    expect(screen.getByText("tienda isn't protected."), "grd.empty.unprotected").toBeInTheDocument();
    expect(document.body.textContent, "it must not claim protection").not.toContain("Protection is on");
    expect(screen.queryAllByRole("button").map((b) => b.textContent), "protecting is US-DSK-002: only Back").toEqual(["Back to the fleet"]);
  });

  it("D7-VIEW: before the first read it shows a busy skeleton and never nothing blocked", async () => {
    showView();
    await flush();
    expect(document.querySelector('[aria-busy="true"]'), "Skeleton while there is no data").not.toBeNull();
    expect(document.body.textContent, "loading is not empty").not.toMatch(/Nothing blocked/);
  });

  it("D7-VIEW: an unavailable list says so and that protection is still on", async () => {
    seed({ state: "unavailable", cause: "log-unavailable", protection: { state: "full" }, entries: [], readAtMs: null });
    showView();
    await flush();
    expect(document.body.textContent, "grd.unavailable.active").toContain("The block list of tienda isn't available right now. Protection is still on: the hooks decide without this list.");
  });

  it("D7-VIEW: with unknown protection it says protection does not depend on the list", async () => {
    seed({ state: "unavailable", cause: "log-unavailable", protection: null, entries: [], readAtMs: null });
    showView();
    await flush();
    expect(document.body.textContent, "grd.unavailable.unknown").toContain("Protection doesn't depend on this list");
    expect(document.body.textContent, "unknown protection is never claimed active").not.toContain("Protection is still on");
  });

  it("D7-VIEW: never says nothing blocked when the list is unavailable", async () => {
    for (const protection of [{ state: "full" as const }, null]) {
      seed({ state: "unavailable", cause: "not-observed", protection, entries: [], readAtMs: null });
      showView();
      await flush();
      expect(document.body.textContent, "an unavailable list is not an empty one").not.toMatch(/Nothing blocked/);
      expect(document.body.textContent, "something must be painted").toMatch(/isn't available right now|isn't watching/);
      cleanup();
    }
  });

  it("D7-VIEW: an unavailable list keeps the last entries with the stale notice", async () => {
    seed({ state: "unavailable", cause: "log-unavailable", protection: { state: "full" }, entries: [entry("d1")], readAtMs: NOW - 90_000 });
    showView();
    await flush();
    expect(screen.getByRole("button", { name: /Blocked: Claude Code tried to rewrite/ }), "the last entries stay below the notice").toBeInTheDocument();
    expect(document.body.textContent, "grd.stale").toMatch(/Last read .+ ago/);
  });

  it("D7-VIEW: untrusted text is rendered as text", async () => {
    seed(readyOf(blocks([entry("d1", { branch: '<img src=x onerror="alert(1)">', worktree: { folder: "evil‮folder", path: "/w/x" } })])));
    const { container } = showView();
    await flush();
    expect(container.querySelector("img"), "engine text never becomes HTML").toBeNull();
    expect(container.textContent, "the markup shows as characters").toContain("<img src=x");
  });

  for (const lang of ["en", "es"] as const) {
    for (const theme of ["light", "dark"] as const) {
      it(`D7-VIEW: axe finds no violations (${lang}, ${theme})`, async () => {
        await setLang(lang);
        document.documentElement.setAttribute("data-theme", theme);
        seed(
          readyOf(
            blocks([entry("d1"), entry("r1", { detail: "rate-limited", exception: { state: "not-offered", reason: "rate-limited" } })], {
              unlogged: [{ fromMs: NOW - 3_600_000, toMs: null }],
              pending: { appliesAtMs: NOW + 10_000, windowMs: 10_000, operation: { kind: "rebase" }, rules: ["minimum.force-push"], cancelCommand: null },
            }),
          ),
        );
        const { container } = showView({ copied: { repoId: "tienda", operation: { kind: "commit" }, rules: ["x"] } });
        await flush();
        expect(container.querySelectorAll("li, tr, button").length, "painted before axe").toBeGreaterThan(1);
        const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
        expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
      });
    }
  }
});

describe("D7-VIEW toast of a new block (EngineCenter)", () => {
  const perf = { backendNs: 1 };
  const batchBase: EngineMessage[] = [
    {
      type: "global",
      engine: { state: "observing", gitVersion: "2.45.1" },
      daemon: { version: "0.1.0", protocol: 9 },
      autostart: "unknown",
      repos: [{ repoId: "tienda", name: "tienda", path: "/w/tienda/.git", state: "observed", tier: "active", attention: { conflicts: { state: "counted", count: 0 }, denials: { state: "counted", count: 0 } } }],
    },
    { type: "repo", fleet: { ...tienda().repo, repoId: "tienda", pairs: [] } },
    { type: "prediction", repoId: "tienda", pairs: [], removed: [], perf },
  ];
  const guard = (entries: BlockEntryDto[]): EngineMessage => ({ type: "guardBlocks", blocks: blocks(entries) });
  const apply = async (...batch: EngineMessage[]) => {
    act(() => applyEngineBatch(client, batch));
    await flush();
  };
  const mountCenter = async () => {
    useUiStore.setState({ selectedRepoId: "tienda", center: { view: "main" } });
    act(() => applyEngineBatch(client, batchBase));
    render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <EngineCenter center={{ kind: "content", stale: false }} conn={{ kind: "live" }} gitVersion="2.45.1" candidates={[]} onRetry={() => {}} />
        </ToastProvider>
      </QueryClientProvider>,
    );
    await flush();
  };
  const old = entry("old", { lastMs: NOW - 3_600_000, atMs: NOW - 3_600_000 });
  const fresh = (id: string) => entry(id, { lastMs: NOW + 1_000, atMs: NOW + 1_000 });
  const alerts = () => screen.queryAllByRole("alert").filter((a) => a.hasAttribute("data-toast"));

  it("D7-VIEW: history at the first read opens no toast", async () => {
    await mountCenter();
    await apply(guard([old]));
    expect(alerts(), "what was blocked before the app looked is not new").toHaveLength(0);
  });

  it("D7-VIEW: a new block opens one persistent alert toast", async () => {
    await mountCenter();
    await apply(guard([old]));
    await apply(guard([old, fresh("n1")]));
    expect(alerts(), "one toast for the new block").toHaveLength(1);
    expect(alerts()[0], "the toast carries the title").toHaveTextContent(/Blocked: Claude Code tried to rewrite the history of main/);
    await flush(60_000);
    expect(alerts(), "an error toast stays until someone closes it").toHaveLength(1);
  });

  it("D7-VIEW: a new block shows an error toast that stays and is announced as an alert", async () => {
    await setLang("es");
    await mountCenter();
    await apply(guard([old]));
    await apply(guard([old, fresh("n1")]));
    expect(alerts(), "one toast for the new block").toHaveLength(1);
    const toast = alerts()[0]!;
    expect(toast, "role alert").toHaveAttribute("role", "alert");
    expect(toast.textContent, "srLabel replaces the hidden kind prefix with the block label").toContain("Bloqueo");
    expect(toast.textContent, "and not the generic Error prefix").not.toMatch(/\bError\b/);
  });

  it("D7-VIEW: many blocks show one toast", async () => {
    await mountCenter();
    await apply(guard([old]));
    await apply(guard([old, fresh("n1"), fresh("n2")]));
    expect(alerts(), "one single toast").toHaveLength(1);
    expect(alerts()[0], "grd.toast.many").toHaveTextContent("2 operations were blocked");
  });

  it("D7-VIEW: View opens the dialog of that row", async () => {
    await mountCenter();
    await apply(guard([old]));
    await apply(guard([old, fresh("n1")]));
    expect(alerts(), "precondition: the toast is there").toHaveLength(1);
    fireEvent.click(within(alerts()[0]!).getByRole("button", { name: /View|Ver/ }));
    await flush();
    expect(screen.getByRole("dialog"), "the block dialog of the row").toHaveAccessibleName(/Blocked: Claude Code tried to rewrite/);
    expect(alerts(), "opening the dialog dismisses the toast").toHaveLength(0);
  });

  it("D7-VIEW: opening the view dismisses it", async () => {
    await mountCenter();
    await apply(guard([old]));
    await apply(guard([old, fresh("n1")]));
    expect(alerts(), "precondition: the toast is there").toHaveLength(1);
    act(() => useUiStore.getState().openGuardrails());
    await flush();
    expect(alerts(), "opening the Guardrails view retires the toast").toHaveLength(0);
    expect(screen.getByRole("button", { name: /Blocked: Claude Code tried to rewrite/ }), "and the view shows the rows").toBeInTheDocument();
  });
});
