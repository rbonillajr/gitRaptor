import type { ParseKeys } from "i18next";
import type { ConnState, GlobalDto, ResyncReason, SessionInfo, Tier } from "@/client/engine-types";

/** Minimum Git version the engine needs; the same literal as the TUI's `MIN_GIT` (checked on the Rust side). */
export const MIN_GIT = "2.38";

export type PillTone = "success" | "warning" | "danger" | "muted";
export type CenterKind = "error" | "stopped" | "skeleton" | "restricted" | "waiting-for-git" | "no-repos" | "content";

export interface EngineViewInput {
  conn: ConnState;
  staleSinceMs: number | null;
  resync: ResyncReason | null;
  session: SessionInfo | null;
  global: GlobalDto | undefined;
  openRepoTier: Tier | null;
  /** BCP 47 tag for the local time of the disconnected band; defaults to the runtime's. */
  locale?: string;
}

export interface EngineView {
  pill: { symbol: string; tone: PillTone; key: ParseKeys; values: Record<string, string | number> };
  band: {
    key: ParseKeys;
    values: Record<string, string | number>;
    /** Second sentence of the band (`engine.band.launchFailed`, `engine.resync.<reason>`). */
    detailKey?: ParseKeys;
    action?: "retryNow" | "start";
  } | null;
  /** `stale`: rows 5 to 7 of the centre table while the connection is not live. */
  center: { kind: CenterKind; stale: boolean };
}

const RESYNC_KEY: Record<ResyncReason, ParseKeys> = {
  "slow-consumer": "engine.resync.slowConsumer",
  "replay-unavailable": "engine.resync.replayUnavailable",
  "daemon-restarted": "engine.resync.daemonRestarted",
  "scope-closed": "engine.resync.scopeClosed",
};

const pill = (symbol: string, tone: PillTone, key: ParseKeys, values: Record<string, string | number> = {}): EngineView["pill"] => ({
  symbol,
  tone,
  key,
  values,
});

function localTime(ms: number | null, locale: string | undefined): string {
  if (ms === null) return "—";
  return new Date(ms).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
}

/** Which of the centre states paints, evaluated in the order of the Dev Spec table (§ La forma). */
function centerKind(input: EngineViewInput): CenterKind {
  const { conn, session, global } = input;
  if (conn.kind === "incompatible" || conn.kind === "rejected" || conn.kind === "unsupported") return "error";
  if (global === undefined) return conn.kind === "stopped" ? "stopped" : "skeleton";
  if (session?.restricted) return "restricted";
  if (global.engine.state === "waiting-for-git") return "waiting-for-git";
  if (global.engine.state === "no-repos") return "no-repos";
  return "content";
}

/** Pill and band, evaluated in the order of the Dev Spec table (T007). */
function pillAndBand(input: EngineViewInput): Pick<EngineView, "pill" | "band"> {
  const { conn, session, global, resync, openRepoTier } = input;
  const hasData = global !== undefined;

  if (conn.kind === "connecting" || (conn.kind === "syncing" && !hasData)) {
    return { pill: pill("◌", "muted", "engine.pill.connecting"), band: null };
  }
  if (conn.kind === "starting") return { pill: pill("◌", "muted", "engine.pill.starting"), band: null };
  if (conn.kind === "reconnecting") {
    return {
      pill: pill("✖", "danger", "engine.pill.disconnected", { seconds: Math.ceil(conn.retryInMs / 1000) }),
      band: {
        key: "engine.band.disconnected",
        values: { time: localTime(input.staleSinceMs, input.locale) },
        action: "retryNow",
      },
    };
  }
  if (conn.kind === "stopped") {
    return {
      pill: pill("✖", "danger", "engine.pill.stopped"),
      band: {
        key: "engine.band.stopped",
        values: {},
        ...(conn.reason === "launch-failed" ? { detailKey: "engine.band.launchFailed" } : {}),
        action: "start",
      },
    };
  }
  if (conn.kind === "incompatible") return { pill: pill("✖", "danger", "engine.pill.incompatible"), band: null };
  if (conn.kind === "rejected") return { pill: pill("✖", "danger", "engine.pill.rejected"), band: null };
  if (conn.kind === "unsupported") return { pill: pill("✖", "danger", "engine.pill.unsupported"), band: null };

  if (session?.restricted) {
    return { pill: pill("◇", "warning", "engine.pill.restricted"), band: { key: "engine.band.restricted", values: {} } };
  }
  if (global?.engine.state === "waiting-for-git") {
    return { pill: pill("⚠", "warning", "engine.pill.waitingForGit", { min: MIN_GIT }), band: null };
  }
  if ((conn.kind === "syncing" && hasData) || resync !== null) {
    return {
      pill: pill("◐", "warning", "engine.pill.catchingUp"),
      band: {
        key: "engine.band.catchingUp",
        values: {},
        ...(resync !== null ? { detailKey: RESYNC_KEY[resync] } : {}),
      },
    };
  }
  if (openRepoTier === "waking") {
    return { pill: pill("◐", "warning", "engine.pill.catchingUp"), band: { key: "engine.band.catchingUp", values: {} } };
  }
  if (openRepoTier === "dormant") return { pill: pill("◐", "muted", "engine.pill.idle"), band: null };
  return { pill: pill("●", "success", "engine.pill.live"), band: null };
}

/** The only place that decides what the engine state paints: the pill, the band and the centre. Pure. */
export function engineView(input: EngineViewInput): EngineView {
  const kind = centerKind(input);
  const dataCenter = kind === "restricted" || kind === "waiting-for-git" || kind === "no-repos" || kind === "content";
  return { ...pillAndBand(input), center: { kind, stale: dataCenter && input.conn.kind !== "live" } };
}
