// TS-DSK-003 T007 (§ 9.4): every engine state the band and the centre can paint, in en and es, with axe in both themes.
// The components take props only; the view model comes from the pure `engineView`.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { CandidateDto, ConnState, GlobalDto, SessionInfo } from "@/client/engine-types";
import { EngineBand } from "./EngineBand";
import { EngineCenter } from "./EngineCenter";
import { engineView, type EngineViewInput } from "./engine-view";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const global = (state: GlobalDto["engine"]["state"], gitVersion: string | null = "2.45.1"): GlobalDto => ({
  engine: { state, gitVersion },
  daemon: { version: "0.1.0", protocol: 1 },
  autostart: "unknown",
  repos: [],
});
const session = (restricted: boolean): SessionInfo => ({ daemonVersion: "0.1.0", protocol: 1, restricted, activity: true, discovery: true });
const incompatible = (over: Partial<Extract<ConnState, { kind: "incompatible" }>>): ConnState => ({
  kind: "incompatible",
  which: "daemon-older",
  daemonVersion: "0.0.9",
  daemonPath: "/opt/gitraptor/raptor",
  stopCommand: "'/opt/gitraptor/raptor' daemon stop",
  action: "copy-stop",
  ...over,
});

interface Scenario {
  name: string;
  input: Partial<EngineViewInput>;
  candidates?: CandidateDto[];
}
const BASE: EngineViewInput = { conn: { kind: "live" }, staleSinceMs: null, resync: null, session: null, global: global("observing"), openRepoTier: null, locale: "en-US" };
const STALE_AT = new Date(2026, 10, 9, 14, 5).getTime();

const SCENARIOS: Scenario[] = [
  { name: "connecting skeleton", input: { conn: { kind: "connecting" }, global: undefined } },
  { name: "stopped", input: { conn: { kind: "stopped", reason: "not-running" }, global: undefined } },
  { name: "stopped, launch failed", input: { conn: { kind: "stopped", reason: "launch-failed" }, global: undefined } },
  { name: "reconnecting", input: { conn: { kind: "reconnecting", attempt: 1, retryInMs: 2000 }, staleSinceMs: STALE_AT } },
  { name: "incompatible with stop command", input: { conn: incompatible({}) } },
  { name: "incompatible without stop command", input: { conn: incompatible({ stopCommand: null }) } },
  { name: "incompatible, version unknown", input: { conn: incompatible({ daemonVersion: null, daemonPath: null, stopCommand: null, action: "none" }) } },
  { name: "incompatible, own engine", input: { conn: incompatible({ stopCommand: null, daemonPath: null, action: "start" }) } },
  { name: "incompatible, app older", input: { conn: incompatible({ which: "app-older", daemonVersion: "9.9.0", stopCommand: null, action: "update-app" }) } },
  { name: "rejected", input: { conn: { kind: "rejected" } } },
  { name: "unsupported", input: { conn: { kind: "unsupported" } } },
  { name: "waiting for git", input: { global: global("waiting-for-git", null) } },
  {
    name: "no repos with candidates",
    input: { global: global("no-repos") },
    candidates: [{ name: "alpha", path: "/Users/me/code/alpha" }],
  },
  { name: "no repos without candidates", input: { global: global("no-repos") } },
  { name: "restricted", input: { session: session(true) } },
  { name: "catching up", input: { conn: { kind: "syncing" }, resync: "slow-consumer" } },
  { name: "content", input: {} },
];

function Harness({ input, candidates = [], onRetry = () => {}, pending = false }: Scenario & { onRetry?: (start: boolean) => void; pending?: boolean }) {
  const full = { ...BASE, ...input };
  const view = engineView(full);
  return (
    <div>
      {view.band ? <EngineBand band={view.band} onRetry={onRetry} pending={pending} /> : null}
      <EngineCenter
        center={view.center}
        conn={full.conn}
        gitVersion={full.global?.engine.gitVersion ?? null}
        candidates={candidates}
        onRetry={onRetry}
        pending={pending}
      />
    </div>
  );
}
const show = (s: Scenario, extra: { onRetry?: (start: boolean) => void; pending?: boolean } = {}) =>
  render(<Harness {...s} {...extra} />);
const scenario = (name: string): Scenario => {
  const s = SCENARIOS.find((x) => x.name === name);
  if (!s) throw new Error(`no scenario ${name}`);
  return s;
};

let writeText: ReturnType<typeof vi.fn>;
beforeEach(async () => {
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  await setLang("en");
});
afterEach(cleanup);

describe("engine states", () => {
  it("connecting shows a loading skeleton, never an empty state", () => {
    const { container } = show(scenario("connecting skeleton"));
    expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true");
    expect(container.querySelector("[data-center]")).toHaveAttribute("data-center", "skeleton");
    expect(screen.queryByText("Nothing to show yet.")).toBeNull();
  });

  it("stopped engine says no history is recorded and offers a retry plus the command to start it", () => {
    const onRetry = vi.fn();
    show(scenario("stopped"), { onRetry });
    expect(screen.getByText("The GitRaptor engine is not running: no history is being recorded for your repos.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Engine stopped" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start engine" }), "the app cannot start the engine yet").toBeNull();
    expect(screen.getAllByText("raptor daemon start").every((el) => el.tagName === "CODE")).toBe(true);
    const [retry] = screen.getAllByRole("button", { name: "Retry now" });
    if (!retry) throw new Error("no retry button");
    fireEvent.click(retry);
    expect(onRetry).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("a failed launch adds its sentence to the band", () => {
    show(scenario("stopped, launch failed"));
    expect(within(screen.getByRole("status")).getByText(/It could not be started\./)).toBeInTheDocument();
  });

  it("copies the start command", async () => {
    show(scenario("stopped"));
    const [copy] = screen.getAllByRole("button", { name: "Copy" });
    if (!copy) throw new Error("no copy button");
    await act(async () => void fireEvent.click(copy));
    expect(writeText).toHaveBeenCalledWith("raptor daemon start");
  });

  it("lost connection dims the fleet and says since when", async () => {
    const time = (lang: string) => new Date(STALE_AT).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
    const { container, unmount } = show(scenario("reconnecting"));
    expect(screen.getByRole("status")).toHaveTextContent(`Lost the connection to the engine. What you see may be out of date since ${time("en-US")}.`);
    expect(container.querySelector("[data-center]")).toHaveAttribute("data-stale", "true");
    unmount();

    await setLang("es");
    const input = { ...scenario("reconnecting").input, locale: "es-ES" };
    const es = show({ name: "es", input });
    expect(screen.getByRole("status")).toHaveTextContent(`desactualizado desde las ${time("es-ES")}.`);
    expect(es.container.querySelector("[data-center]")).toHaveAttribute("data-stale", "true");
  });

  it("the retry button calls back with start=false and is aria-disabled while a retry is pending", () => {
    const onRetry = vi.fn();
    const { unmount } = show(scenario("reconnecting"), { onRetry });
    fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
    expect(onRetry).toHaveBeenCalledExactlyOnceWith(false);
    unmount();

    const pendingRetry = vi.fn();
    show(scenario("reconnecting"), { onRetry: pendingRetry, pending: true });
    const button = screen.getByRole("button", { name: "Retry now" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(pendingRetry).not.toHaveBeenCalled();
  });

  it("live data is not dimmed", () => {
    const { container } = show(scenario("content"));
    expect(container.querySelector("[data-center]")).not.toHaveAttribute("data-stale");
    expect(screen.getByText("Nothing to show yet.")).toBeInTheDocument();
  });

  it("incompatible engine says how to update, with the stop command and a Copy button", async () => {
    show(scenario("incompatible with stop command"));
    expect(screen.getByRole("heading", { name: "Incompatible engine" })).toBeInTheDocument();
    expect(screen.getByText(/The running engine \(0\.0\.9, from \/opt\/gitraptor\/raptor\) is older than this app/)).toBeInTheDocument();
    expect(screen.getByText("'/opt/gitraptor/raptor' daemon stop").tagName).toBe("CODE");
    await act(async () => void fireEvent.click(screen.getByRole("button", { name: "Copy" })));
    expect(writeText).toHaveBeenCalledWith("'/opt/gitraptor/raptor' daemon stop");
  });

  it("incompatible engine without a stop command explains why there is none", () => {
    show(scenario("incompatible without stop command"));
    expect(screen.getByText(/can't be copied safely/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it("incompatible engine of unknown version uses the generic text", () => {
    show(scenario("incompatible, version unknown"));
    expect(screen.getByText(/The running engine is older than this app\. Update the GitRaptor installation/)).toBeInTheDocument();
  });

  it("the app's own older engine offers a retry and the start command, not a start button", () => {
    const onRetry = vi.fn();
    show(scenario("incompatible, own engine"), { onRetry });
    expect(screen.getByText(/this app's own, from an older version/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start engine" })).toBeNull();
    expect(screen.getByText("raptor daemon start").tagName).toBe("CODE");
    fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
    expect(onRetry).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("an app older than the engine asks to update the app", () => {
    show(scenario("incompatible, app older"));
    expect(screen.getByText("This app is older than the running engine (9.9.0). Update the app.")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("rejected and unsupported explain themselves", () => {
    const { unmount } = show(scenario("rejected"));
    expect(screen.getByRole("heading", { name: "Engine channel refused" })).toBeInTheDocument();
    expect(screen.getByText(/runtime folder is not private/)).toBeInTheDocument();
    unmount();
    show(scenario("unsupported"));
    expect(screen.getByRole("heading", { name: "No engine channel" })).toBeInTheDocument();
    expect(screen.getByText("This platform has no engine channel yet.")).toBeInTheDocument();
  });

  it("waiting for Git names the minimum version", () => {
    show(scenario("waiting for git"));
    expect(screen.getByRole("heading", { name: "Waiting for Git" })).toBeInTheDocument();
    expect(screen.getByText(/Git 2\.38 or later not found/)).toBeInTheDocument();
  });

  it("no repos shows the welcome with discovered repos", () => {
    show(scenario("no repos with candidates"));
    expect(screen.getByRole("heading", { name: "Work with AI agents without fear of losing your code" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "See your agents" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recover what went wrong" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Stop what's dangerous" })).toBeInTheDocument();
    expect(screen.getByText(/Everything stays on your computer/)).toBeInTheDocument();
    expect(screen.getByText("Engine running")).toBeInTheDocument();
    expect(screen.getByText("Git 2.45.1 found")).toBeInTheDocument();
    expect(screen.getByText("raptor repo add <path>").tagName).toBe("CODE");
    expect(screen.getByRole("heading", { name: "Repos GitRaptor found on your disk" })).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("/Users/me/code/alpha")).toBeInTheDocument();
  });

  it("no repos without candidates hides the discovered list; es shows the localized command", async () => {
    await setLang("es");
    show(scenario("no repos without candidates"));
    expect(screen.queryByRole("heading", { name: "Repos que GitRaptor encontró en tu disco" })).toBeNull();
    expect(screen.getByText("raptor repo add <ruta>").tagName).toBe("CODE");
  });

  it("restricted view shows the agent banner and no repos", () => {
    show(scenario("restricted"));
    expect(screen.getByRole("status")).toHaveTextContent("An agent opened this window.");
    expect(screen.getByRole("heading", { name: "Restricted view" })).toBeInTheDocument();
    expect(screen.getByText(/shows no repos because an agent opened it/)).toBeInTheDocument();
  });

  it("catching up band names the reason", () => {
    show(scenario("catching up"));
    const band = screen.getByRole("status");
    expect(band).toHaveTextContent("Checking recent changes; the view may take a few seconds.");
    expect(band).toHaveTextContent("The app fell behind the engine.");
  });

  it("engine text is rendered as text", () => {
    const evil = "<img src=x onerror=alert(1)>‮\u001b]52;c;AAAA";
    const input = { conn: incompatible({ daemonVersion: evil, daemonPath: evil, stopCommand: null, action: "none" }) };
    const { container, unmount } = show({ name: "evil", input });
    expect(container.textContent).toContain(evil);
    expect(container.querySelector("img")).toBeNull();
    unmount();

    const repos = show({ name: "evil", input: { global: global("no-repos", evil) }, candidates: [{ name: evil, path: evil }] });
    expect(repos.container.textContent).toContain(evil);
    expect(screen.getAllByText(evil)).toHaveLength(2);
    expect(repos.container.querySelector("img")).toBeNull();
  });

  it.each(["en", "es"] as const)("never shows a raw translation key in %s", async (lang) => {
    await setLang(lang);
    for (const s of SCENARIOS) {
      const { container, unmount } = show(s);
      expect(container.textContent, `${lang}/${s.name}`).not.toMatch(/\b(engine|empty|shell|ui)\.[a-z][A-Za-z]*(\.[a-z][A-Za-z0-9]*)*\b/);
      unmount();
    }
  });

  it.each([
    ["en", "dark"],
    ["en", "light"],
    ["es", "dark"],
    ["es", "light"],
  ] as const)("every state has no axe violations in %s / %s", async (lang, theme) => {
    await setLang(lang);
    for (const s of SCENARIOS) {
      const { container, unmount } = render(
        <div data-theme={theme}>
          <Harness {...s} />
        </div>,
      );
      const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
      expect(results.violations.map((v) => `${s.name}: ${v.id}: ${v.help}`)).toEqual([]);
      unmount();
    }
  });
});
