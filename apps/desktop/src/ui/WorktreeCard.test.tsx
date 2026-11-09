// DS-US-DSK-004 T005: WorktreeCard paints the texts it is given; CardSkeleton is hidden from assistive tech.
import { cleanup, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, describe, expect, it } from "vitest";
import "@/i18n";
import { CardSkeleton } from "./CardSkeleton";
import { WorktreeCard, type WorktreeCardProps } from "./WorktreeCard";

afterEach(cleanup);

const base: WorktreeCardProps = {
  symbol: "●",
  agentLabel: "Claude Code · feat-pagos",
  colorSlot: 2,
  muted: false,
  shared: false,
  stateText: "active",
  main: false,
  branchText: "feat-pagos",
  branchCommit: null,
  filesText: "2 changed files",
  syncText: "3 ahead and 1 behind main",
  activityText: "2 min ago",
  attentionText: null,
  unavailableText: null,
  a11yLabel: "Claude Code · feat-pagos, active, feat-pagos, 2 changed files, 3 ahead and 1 behind main, 2 min ago",
};

describe("WorktreeCard", () => {
  it("is an article named by the phrase it is given, and shows every part", () => {
    render(<WorktreeCard {...base} />);
    const card = screen.getByRole("article", { name: base.a11yLabel });
    for (const text of ["Claude Code · feat-pagos", "active", "feat-pagos", "2 changed files", "3 ahead and 1 behind main", "2 min ago"]) {
      expect(within(card).getByText(text)).toBeInTheDocument();
    }
  });

  it("the agent name takes its lane colour, muted when muted or without a slot", () => {
    const { rerender } = render(<WorktreeCard {...base} colorSlot={5} />);
    expect(screen.getByText("Claude Code · feat-pagos").className).toContain("text-lane-5");
    rerender(<WorktreeCard {...base} colorSlot={5} muted />);
    expect(screen.getByText("Claude Code · feat-pagos").className).toContain("text-text-muted");
    rerender(<WorktreeCard {...base} colorSlot={null} />);
    expect(screen.getByText("Claude Code · feat-pagos").className).toContain("text-text-muted");
  });

  it("hides the glyph from assistive tech and omits it when empty", () => {
    const { rerender } = render(<WorktreeCard {...base} />);
    expect(screen.getByText("●")).toHaveAttribute("aria-hidden", "true");
    rerender(<WorktreeCard {...base} symbol="" />);
    expect(screen.queryByText("●")).toBeNull();
  });

  it("marks main, shared and attention", () => {
    render(<WorktreeCard {...base} main shared attentionText="⚡ predicted conflict with main" />);
    expect(screen.getByText("main worktree")).toBeInTheDocument();
    expect(screen.getByText("shared")).toBeInTheDocument();
    expect(screen.getByText("⚡ predicted conflict with main")).toBeInTheDocument();
    expect(screen.getByRole("article")).toHaveAttribute("data-attention", "true");
  });

  it("sets a detached hash in code", () => {
    render(<WorktreeCard {...base} branchText="abc1234 (no branch)" branchCommit="abc1234" />);
    expect(screen.getByText("abc1234").tagName).toBe("CODE");
  });

  it("does not make code of a branch that merely looks like a hash", () => {
    render(<WorktreeCard {...base} branchText="abc1234" branchCommit="zzz" />);
    expect(screen.getByText("abc1234").tagName).not.toBe("CODE");
  });

  it("an unavailable worktree has the reason and no branch or metrics", () => {
    render(<WorktreeCard {...base} symbol="⚠" branchText={null} filesText={null} syncText={null} activityText={null} unavailableText="unavailable: folder missing" />);
    expect(screen.getByText("unavailable: folder missing")).toBeInTheDocument();
    expect(screen.queryByText("2 changed files")).toBeNull();
  });

  it("renders text as text", () => {
    const { container } = render(<WorktreeCard {...base} branchText="<b>x</b>" />);
    expect(screen.getByText("<b>x</b>")).toBeInTheDocument();
    expect(container.querySelector("b")).toBeNull();
  });

  it("has no buttons or links", () => {
    render(<WorktreeCard {...base} />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("link")).toBeNull();
  });

  it("has no axe violations", async () => {
    const { container } = render(
      <ul>
        <li>
          <WorktreeCard {...base} main attentionText="⚡ predicted conflict with main" />
        </li>
      </ul>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});

describe("CardSkeleton", () => {
  it("is hidden from assistive tech and has no text", () => {
    const { container } = render(<CardSkeleton />);
    expect(container.firstElementChild).toHaveAttribute("aria-hidden", "true");
    expect(container).toHaveTextContent("");
  });
});
