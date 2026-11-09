// TS-DSK-003 T007: one test per row of the centre table (§ La forma) and of the pill/band table.
import { describe, expect, it } from "vitest";
import type { ConnState, GlobalDto, SessionInfo } from "@/client/engine-types";
import { engineView, type EngineViewInput } from "./engine-view";

const GLOBAL = (state: GlobalDto["engine"]["state"]): GlobalDto => ({
  engine: { state, gitVersion: "2.45.0" },
  daemon: { version: "0.1.0", protocol: 1 },
  autostart: "unknown",
  repos: [],
});
const SESSION = (restricted: boolean): SessionInfo => ({ daemonVersion: "0.1.0", protocol: 1, restricted, activity: true, discovery: true });
const INCOMPATIBLE: ConnState = {
  kind: "incompatible",
  which: "daemon-older",
  daemonVersion: "0.0.9",
  daemonPath: "/opt/raptor",
  stopCommand: null,
  action: "none",
};

const view = (over: Partial<EngineViewInput>) =>
  engineView({ conn: { kind: "live" }, staleSinceMs: null, resync: null, session: null, global: GLOBAL("observing"), openRepoTier: null, locale: "en-US", ...over });

/** Compile-time exhaustiveness: a new `ConnState` kind fails here until it has a sample and a test. */
const SAMPLES = {
  connecting: { kind: "connecting" },
  starting: { kind: "starting" },
  syncing: { kind: "syncing" },
  live: { kind: "live" },
  reconnecting: { kind: "reconnecting", attempt: 2, retryInMs: 2500 },
  stopped: { kind: "stopped", reason: "not-running" },
  incompatible: INCOMPATIBLE,
  rejected: { kind: "rejected" },
  unsupported: { kind: "unsupported" },
} satisfies Record<ConnState["kind"], ConnState>;

describe("engineView pill and band", () => {
  it("covers every ConnState kind with a pill", () => {
    for (const conn of Object.values(SAMPLES)) {
      const v = view({ conn, global: undefined });
      expect(v.pill.key, conn.kind).toMatch(/^engine\.pill\./);
      expect(v.pill.symbol.length, conn.kind).toBeGreaterThan(0);
    }
  });

  it("row 1: connecting, and syncing without data, show the connecting pill and no band", () => {
    for (const conn of [SAMPLES.connecting, SAMPLES.syncing]) {
      expect(view({ conn, global: undefined })).toMatchObject({ pill: { symbol: "◌", tone: "muted", key: "engine.pill.connecting" }, band: null });
    }
  });

  it("row 2: starting", () => {
    expect(view({ conn: SAMPLES.starting, global: undefined })).toMatchObject({ pill: { symbol: "◌", key: "engine.pill.starting" }, band: null });
  });

  it("row 3: reconnecting rounds the wait up and names the local time since the data is stale", () => {
    const staleSinceMs = new Date(2026, 10, 9, 14, 5).getTime();
    const v = view({ conn: SAMPLES.reconnecting, staleSinceMs });
    expect(v.pill).toEqual({ symbol: "✖", tone: "danger", key: "engine.pill.disconnected", values: { seconds: 3 } });
    expect(v.band).toEqual({
      key: "engine.band.disconnected",
      values: { time: new Date(staleSinceMs).toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit" }) },
      action: "retryNow",
    });
  });

  it("row 4: stopped offers start; launch-failed adds its sentence", () => {
    expect(view({ conn: SAMPLES.stopped, global: undefined })).toMatchObject({
      pill: { symbol: "✖", key: "engine.pill.stopped" },
      band: { key: "engine.band.stopped", action: "start" },
    });
    expect(view({ conn: SAMPLES.stopped, global: undefined }).band).not.toHaveProperty("detailKey");
    expect(view({ conn: { kind: "stopped", reason: "launch-failed" }, global: undefined }).band).toMatchObject({
      key: "engine.band.stopped",
      detailKey: "engine.band.launchFailed",
    });
  });

  it("row 5: incompatible has its pill and no band (the panel carries it)", () => {
    expect(view({ conn: INCOMPATIBLE })).toMatchObject({ pill: { symbol: "✖", tone: "danger", key: "engine.pill.incompatible" }, band: null });
  });

  it("row 6: rejected and unsupported have their pill and no band", () => {
    expect(view({ conn: SAMPLES.rejected })).toMatchObject({ pill: { key: "engine.pill.rejected" }, band: null });
    expect(view({ conn: SAMPLES.unsupported })).toMatchObject({ pill: { key: "engine.pill.unsupported" }, band: null });
  });

  it("row 7: a restricted session shows the restricted pill and band", () => {
    expect(view({ session: SESSION(true) })).toMatchObject({
      pill: { symbol: "◇", key: "engine.pill.restricted" },
      band: { key: "engine.band.restricted" },
    });
  });

  it("row 8: waiting for git names the minimum version and has no band", () => {
    expect(view({ global: GLOBAL("waiting-for-git") })).toMatchObject({
      pill: { symbol: "⚠", tone: "warning", key: "engine.pill.waitingForGit", values: { min: "2.38" } },
      band: null,
    });
  });

  it("row 9: syncing with data, or a resync reason, shows catching up (with the reason)", () => {
    expect(view({ conn: SAMPLES.syncing })).toMatchObject({ pill: { symbol: "◐", key: "engine.pill.catchingUp" }, band: { key: "engine.band.catchingUp" } });
    expect(view({ conn: SAMPLES.syncing }).band).not.toHaveProperty("detailKey");
    const reasons = {
      "slow-consumer": "engine.resync.slowConsumer",
      "replay-unavailable": "engine.resync.replayUnavailable",
      "daemon-restarted": "engine.resync.daemonRestarted",
      "scope-closed": "engine.resync.scopeClosed",
    } as const;
    for (const [resync, detailKey] of Object.entries(reasons) as Array<[keyof typeof reasons, string]>) {
      expect(view({ resync }).band, resync).toMatchObject({ key: "engine.band.catchingUp", detailKey });
    }
  });

  it("row 10: an open repo waking shows catching up", () => {
    expect(view({ openRepoTier: "waking" })).toMatchObject({ pill: { key: "engine.pill.catchingUp" }, band: { key: "engine.band.catchingUp" } });
  });

  it("row 11: an open repo dormant shows idle with no band", () => {
    expect(view({ openRepoTier: "dormant" })).toMatchObject({ pill: { symbol: "◐", key: "engine.pill.idle" }, band: null });
  });

  it("row 12: anything else is live", () => {
    expect(view({ openRepoTier: "active" })).toMatchObject({ pill: { symbol: "●", tone: "success", key: "engine.pill.live", values: {} }, band: null });
  });

  it("precedence: restricted beats waiting-for-git, and waiting-for-git beats resync", () => {
    expect(view({ session: SESSION(true), global: GLOBAL("waiting-for-git") }).pill.key).toBe("engine.pill.restricted");
    expect(view({ global: GLOBAL("waiting-for-git"), resync: "slow-consumer" }).pill.key).toBe("engine.pill.waitingForGit");
  });
});

describe("engineView centre", () => {
  it("row 1: incompatible, rejected and unsupported give the error panel, with or without data", () => {
    for (const conn of [INCOMPATIBLE, SAMPLES.rejected, SAMPLES.unsupported]) {
      expect(view({ conn }).center.kind, conn.kind).toBe("error");
      expect(view({ conn, global: undefined }).center.kind, conn.kind).toBe("error");
    }
  });

  it("row 2: stopped with no global snapshot gives the stopped panel", () => {
    expect(view({ conn: SAMPLES.stopped, global: undefined }).center).toEqual({ kind: "stopped", stale: false });
  });

  it("row 3: no global snapshot in any other state is the skeleton, never empty", () => {
    for (const conn of [SAMPLES.connecting, SAMPLES.starting, SAMPLES.syncing, SAMPLES.live, SAMPLES.reconnecting]) {
      expect(view({ conn, global: undefined }).center, conn.kind).toEqual({ kind: "skeleton", stale: false });
    }
  });

  it("row 4: a restricted session gives the restricted panel", () => {
    expect(view({ session: SESSION(true) }).center.kind).toBe("restricted");
  });

  it("row 5: waiting-for-git gives panel a0", () => {
    expect(view({ global: GLOBAL("waiting-for-git") }).center.kind).toBe("waiting-for-git");
  });

  it("row 6: no-repos gives screen a1", () => {
    expect(view({ global: GLOBAL("no-repos") }).center.kind).toBe("no-repos");
  });

  it("row 7: anything else is the content", () => {
    expect(view({}).center).toEqual({ kind: "content", stale: false });
    expect(view({ session: SESSION(false) }).center.kind).toBe("content");
  });

  it("rows 5 to 7 are stale while the connection is not live, and only those", () => {
    expect(view({ conn: SAMPLES.reconnecting }).center).toEqual({ kind: "content", stale: true });
    expect(view({ conn: SAMPLES.syncing, global: GLOBAL("no-repos") }).center).toEqual({ kind: "no-repos", stale: true });
    expect(view({ conn: SAMPLES.reconnecting, session: SESSION(true) }).center).toEqual({ kind: "restricted", stale: true });
    expect(view({ conn: INCOMPATIBLE }).center.stale).toBe(false);
  });
});
