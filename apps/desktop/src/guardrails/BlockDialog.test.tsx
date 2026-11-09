/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// D7-DIALOG (DS-US-DSK-007 T005, D14, D17, § 9.4): the two-step block dialog on a BlockRow. Step 1 (d1) explains who,
// what, why and which rule; step 2 (d4) shows the exact command with Copy. Only CopyCommand touches the clipboard.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { BlockEntryDto, OperationDto, PendingExceptionDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import type { GuardBlocks } from "@/state/engine-cache";
import { BlockDialog } from "./BlockDialog";
import type { BlockRow } from "./blocks-model";

const CMD = "'/Applications/GitRaptor.app/Contents/MacOS/raptor' 'guard' 'exec' '--' 'git' 'push' '--force-with-lease' 'origin' 'feat-pagos:main'";
const FORCE: OperationDto = { kind: "push", remote: "origin", refs: [{ name: "main", change: "force" }] };
const entry = (over: Partial<BlockEntryDto> = {}): BlockEntryDto => ({
  decisionId: "d1",
  kind: "denial",
  atMs: Date.UTC(2026, 10, 9, 11, 59),
  lastMs: Date.UTC(2026, 10, 9, 11, 59),
  count: 1,
  actor: { kind: "claude-code" },
  worktree: { folder: "feat-pagos", path: "/w/tienda/feat-pagos" },
  branch: "feat-pagos",
  operation: FORCE,
  rules: [{ rule: "minimum.force-push", level: "minimum", cause: null }],
  layer: "hooks",
  detail: "full",
  exception: { state: "offered", command: CMD },
  ...over,
});
const ROW: BlockRow = {
  id: "d1",
  group: "blocks",
  kind: "denial",
  title: "Blocked: Claude Code tried to rewrite the history of main on the server (origin)",
  nothingHappened: "Nothing happened. main is unchanged on your machine and on the server.",
  attempted: "A force push of feat-pagos onto main on the server (origin), at 11:59.",
  why: "It replaces the server's history. Commits others pushed that aren't on your machine would be lost for everyone.",
  stoppedBy: ["no force-push (the minimum default set)"],
  technical: ["Rule: minimum.force-push", "Level: minimum", "Layer: hooks", "Actor: Claude Code", "Decision: d1"],
  exception: { command: CMD },
  hint: null,
  ctaDisabled: null,
  a11yLabel: "Blocked: Claude Code tried to rewrite the history of main on the server (origin), 11:59",
};
const ready = (entries: BlockEntryDto[], pending: PendingExceptionDto | null = null): GuardBlocks => ({
  state: "ready",
  repoId: "tienda",
  protection: { state: "full" },
  sinceMs: 0,
  summary: { blocked: entries.length, exceptions: 0, exceptionsRejected: 0 },
  entries,
  pending,
  unlogged: [],
});

let client: QueryClient;
let invoked: string[] = [];
let writeText: ReturnType<typeof vi.fn>;
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const seed = (blocks: GuardBlocks) => client.setQueryData(engineKeys.guard("tienda"), blocks);

function show(row: BlockRow = ROW, props: { fromToast?: boolean; onClose?: () => void; onCopied?: (c: unknown) => void } = {}) {
  return render(
    <QueryClientProvider client={client}>
      <BlockDialog row={row} repoId="tienda" onClose={props.onClose ?? (() => {})} onCopied={props.onCopied ?? (() => {})} {...(props.fromToast ? { fromToast: true } : {})} />
    </QueryClientProvider>,
  );
}
const dialog = () => screen.getByRole("dialog");
const doItMyself = () => screen.getByRole("button", { name: /I'll do it myself/ });
const toStepTwo = () => fireEvent.click(doItMyself());

beforeEach(async () => {
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  invoked = [];
  mockIPC((cmd) => {
    invoked.push(cmd);
    return {};
  });
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  seed(ready([entry()]));
  document.documentElement.removeAttribute("data-theme");
  await setLang("en");
});
afterEach(() => {
  cleanup();
  clearMocks();
});

describe("D7-DIALOG BlockDialog step 1 (d1)", () => {
  it("D7-DIALOG: explains who tried what, the rule that stopped it and when", () => {
    show();
    const d = within(dialog());
    expect(dialog(), "an info dialog named by the block title").toHaveAccessibleName(/^Blocked: Claude Code tried to rewrite the history of main/);
    expect(d.getByText(ROW.nothingHappened!), "Nothing happened").toBeInTheDocument();
    expect(d.getByText("What it tried"), "section What it tried").toBeInTheDocument();
    expect(d.getByText(ROW.attempted), "the attempt with the time").toBeInTheDocument();
    expect(d.getByText("Why it's dangerous")).toBeInTheDocument();
    expect(d.getByText(ROW.why!)).toBeInTheDocument();
    expect(d.getByText("What stopped it")).toBeInTheDocument();
    expect(d.getByText(ROW.stoppedBy[0]!), "the rule text").toBeInTheDocument();
    expect(d.getByText("What you can do"), "section What you can do").toBeInTheDocument();
    expect(d.getByText("Technical details"), "a disclosure with the technical details").toBeInTheDocument();
    for (const line of ROW.technical) expect(dialog().textContent, `technical line ${line}`).toContain(line);
  });

  it("D7-DIALOG: the block symbol in the title is hidden from screen readers because the title already says Blocked", () => {
    show();
    const heading = within(dialog()).getByRole("heading", { level: 2 });
    const symbol = [...heading.querySelectorAll('[aria-hidden="true"]')].find((n) => n.textContent?.includes("⛔"));
    expect(symbol, "the ⛔ of the title is aria-hidden").toBeDefined();
    expect(heading, "the accessible name starts with the text").toHaveAccessibleName(/^Blocked:/);
  });

  it("D7-DIALOG: the footer has one button, Got it, and I'll do it myself sits in the body", () => {
    show();
    const gotIt = screen.getByRole("button", { name: "Got it" });
    const footer = gotIt.parentElement!;
    expect(within(footer).getAllByRole("button"), "the footer holds only Got it").toHaveLength(1);
    expect(footer.contains(doItMyself()), "I'll do it myself… is in the body, not the footer").toBe(false);
    expect(dialog().contains(doItMyself())).toBe(true);
  });

  it("D7-DIALOG: the initial focus is on Got it", () => {
    show();
    expect(document.activeElement, "focus starts on the safe button").toBe(screen.getByRole("button", { name: "Got it" }));
  });

  it("D7-DIALOG: See in the log only shows when it opens from the toast", () => {
    show();
    expect(screen.queryByText("See in the log"), "not from the toast").toBeNull();
    cleanup();
    show(ROW, { fromToast: true });
    expect(screen.getByRole("button", { name: "See in the log" }), "from the toast").toBeInTheDocument();
  });

  it("D7-DIALOG: a commit block shows why there is no exception", () => {
    const why = "The exception only works for push, deleting a local branch and rebase.";
    show({ ...ROW, kind: "denial", exception: { notOffered: why }, why: null, nothingHappened: null });
    expect(screen.queryByRole("button", { name: /I'll do it myself/ }), "no CTA without an offered exception").toBeNull();
    expect(within(dialog()).getByText(why), "the reason is the text of the NotOffered motive").toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Got it" })).toBeInTheDocument();
  });

  it("D7-DIALOG: with the engine down the CTA is disabled and says why", () => {
    show({ ...ROW, ctaDisabled: "engine-down" });
    const cta = doItMyself();
    expect(cta, "the button is aria-disabled or disabled").toSatisfy((b: HTMLElement) => b.getAttribute("aria-disabled") === "true" || (b as HTMLButtonElement).disabled);
    expect(within(dialog()).getByText("The engine is stopped: the exception needs it."), "grd.exc.engineDown").toBeInTheDocument();
    fireEvent.click(cta);
    expect(screen.queryByText("Confirm from your terminal"), "a disabled CTA does not advance").toBeNull();
  });

  it("D7-DIALOG: Esc closes without any call to the engine", () => {
    const onClose = vi.fn();
    show(ROW, { onClose });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape", code: "Escape" });
    expect(onClose, "Esc closes the dialog").toHaveBeenCalled();
    expect(invoked, "closing never calls the engine").toEqual([]);
  });

  it("D7-DIALOG: Got it closes the dialog", () => {
    const onClose = vi.fn();
    show(ROW, { onClose });
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("D7-DIALOG: untrusted text is rendered as text", () => {
    const hostile = '<img src=x onerror="alert(1)"> ‮evil';
    const { baseElement } = show({ ...ROW, title: `Blocked: ${hostile}`, attempted: hostile, stoppedBy: [hostile], technical: [hostile] });
    expect(baseElement.querySelector("img"), "engine text never becomes HTML").toBeNull();
    expect(dialog().textContent, "the markup shows as characters").toContain("<img src=x");
  });
});

describe("D7-DIALOG BlockDialog step 2 (d4)", () => {
  it("D7-DIALOG: I'll do it myself shows the exact command and copies it", async () => {
    const onCopied = vi.fn();
    show(ROW, { onCopied });
    toStepTwo();
    const d = within(dialog());
    expect(d.getByRole("heading", { level: 2, name: "Confirm from your terminal" }), "step 2 title").toBeInTheDocument();
    expect(d.getByText(CMD, { selector: "code" }), "the exact command as code").toBeInTheDocument();
    await act(async () => void fireEvent.click(d.getByRole("button", { name: "Copy" })));
    expect(writeText, "Copy writes the exact command and nothing else").toHaveBeenCalledWith(CMD);
    expect(onCopied, "the copied operation and rules are remembered (D12)").toHaveBeenCalledWith({
      repoId: "tienda",
      operation: FORCE,
      rules: ["minimum.force-push"],
    });
  });

  it("D7-DIALOG: step 2 explains the reason, who does it, the server, where the terminal is and what to check", () => {
    show();
    toStepTwo();
    const text = dialog().textContent ?? "";
    expect(text, "grd.exc.reason").toContain("This version of GitRaptor can't ask for Touch ID yet");
    expect(text, "grd.exc.who").toContain("You do it, not Claude Code. It runs once; the rule stays on afterwards.");
    expect(text, "grd.exc.remote for a push").toContain("GitRaptor can't undo this on the server");
    expect(text, "grd.exc.terminal").toContain("run this command in your own terminal");
    expect(text, "grd.exc.where").toContain("Where's the terminal?");
    expect(text, "grd.exc.review").toContain("Check the command");
    expect(text, "grd.exc.cancelHow").toContain("you have 10 seconds to cancel: press Ctrl-C in that terminal.");
  });

  it("D7-DIALOG: who names the agent type only, never the folder", () => {
    show();
    toStepTwo();
    const who = [...dialog().querySelectorAll("p, li, div")].find((n) => n.textContent?.startsWith("You do it, not"));
    expect(who, "the who line exists").toBeDefined();
    expect(who!.textContent, "the agent type, not the folder").not.toContain("feat-pagos");
  });

  it("D7-DIALOG: a push says which source branch was inferred", () => {
    const hint = "Inferred source branch: feat-pagos (the one checked out in that folder when it was blocked). Check in the terminal's plan that it's the one you want to push.";
    show({ ...ROW, hint });
    toStepTwo();
    expect(within(dialog()).getByText(hint), "the inferred source branch line (M-04)").toBeInTheDocument();
  });

  it("D7-DIALOG: a branch delete says how to force it on purpose", () => {
    const hint = "If git says the branch isn't merged and you still want to delete it, change -d to -D.";
    show({ ...ROW, hint, exception: { command: "'raptor' 'guard' 'exec' '--' 'git' 'branch' '-d' 'feat-x'" } });
    toStepTwo();
    expect(within(dialog()).getByText(hint), "the -d to -D hint (M-05)").toBeInTheDocument();
  });

  it("D7-DIALOG: a rebase shows the form without a copy button", () => {
    const form = "raptor guard exec -- git rebase …";
    show({ ...ROW, exception: { rebaseForm: form } });
    toStepTwo();
    const d = within(dialog());
    expect(d.getByText(form, { selector: "code" }), "the form as code").toBeInTheDocument();
    expect(d.queryByRole("button", { name: "Copy" }), "a form is not a command to paste").toBeNull();
    expect(dialog().textContent, "grd.exc.rebaseHint").toContain("GitRaptor doesn't log which base the agent rebased onto");
  });

  it("D7-DIALOG: tells how to cancel in the terminal and never offers a cancel button", () => {
    show();
    toStepTwo();
    expect(within(dialog()).queryByRole("button", { name: /cancel|cancelar/i }), "closing is not cancelling: no cancel button").toBeNull();
    expect(dialog().textContent).toContain("Ctrl-C");
    expect(screen.getAllByRole("button", { name: "Close" }).some((b) => b.hasAttribute("data-autofocus")), "the footer button is Close").toBe(true);
  });

  it("D7-DIALOG: the footer button of step 2 is Close and it only closes", () => {
    const onClose = vi.fn();
    show(ROW, { onClose });
    toStepTwo();
    const close = screen.getAllByRole("button", { name: "Close" }).find((b) => b.hasAttribute("data-autofocus"));
    expect(close, "the footer Close is the acknowledge button").toBeDefined();
    fireEvent.click(close!);
    expect(onClose).toHaveBeenCalled();
    expect(invoked, "closing sends nothing to the engine (D12)").toEqual([]);
  });

  it("D7-DIALOG: focus moves to Close when the step changes", () => {
    show();
    toStepTwo();
    const active = document.activeElement as HTMLElement;
    expect(active.textContent, "the focus is on Close, not lost on body").toBe("Close");
  });

  it("D7-DIALOG: Esc closes step 2 without any call to the engine", () => {
    const onClose = vi.fn();
    show(ROW, { onClose });
    toStepTwo();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape", code: "Escape" });
    expect(onClose).toHaveBeenCalled();
    expect(invoked).toEqual([]);
  });

  it("D7-DIALOG: an announced exception of this operation shows inside step 2", () => {
    seed(
      ready([entry()], {
        appliesAtMs: Date.now() + 10_000,
        windowMs: 10_000,
        operation: FORCE,
        rules: ["minimum.force-push"],
        cancelCommand: "'raptor' 'guard' 'cancel' '/w/tienda'",
      }),
    );
    show();
    toStepTwo();
    expect(dialog().textContent, "PendingException is shown in the step").toMatch(/Exception announced in a terminal: runs in \d+ s/);
  });

  it("D7-DIALOG: the outcome text replaces the instructions once the exception is logged", () => {
    seed(ready([entry(), entry({ decisionId: "d2", kind: "exception", exception: null, atMs: Date.now(), lastMs: Date.now() })]));
    show();
    toStepTwo();
    expect(dialog().textContent, "the outcome of the operation is told").toMatch(/Done with an exception/);
  });
});

describe("D7-DIALOG BlockDialog accessibility", () => {
  const axeOf = async () => {
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    return results.violations.map((v) => `${v.id}: ${v.help}`);
  };

  for (const lang of ["en", "es"] as const) {
    for (const theme of ["light", "dark"] as const) {
      it(`D7-DIALOG: axe finds no violations in step 1 and step 2 (${lang}, ${theme})`, async () => {
        await setLang(lang);
        document.documentElement.setAttribute("data-theme", theme);
        show();
        expect(screen.getByRole("dialog"), "painted before axe").toBeInTheDocument();
        expect(await axeOf(), `step 1 ${lang} ${theme}`).toEqual([]);
        fireEvent.click(screen.getByRole("button", { name: lang === "en" ? /I'll do it myself/ : /Lo haré yo/ }));
        expect(screen.getByRole("heading", { level: 2, name: lang === "en" ? "Confirm from your terminal" : "Confirma desde tu terminal" }), "step 2 painted").toBeInTheDocument();
        expect(await axeOf(), `step 2 ${lang} ${theme}`).toEqual([]);
      });
    }
  }
});
