// TS-DSK-003 T007: StatePanel shows a title, an optional message and an optional action.
import { cleanup, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, describe, expect, it } from "vitest";
import { StatePanel } from "./StatePanel";

afterEach(cleanup);

describe("StatePanel", () => {
  it("renders the title as a heading, the message and the action", () => {
    render(<StatePanel title="Engine stopped" message="Nothing is recorded." action={<button type="button">Retry</button>} />);
    expect(screen.getByRole("heading", { name: "Engine stopped" })).toBeInTheDocument();
    expect(screen.getByText("Nothing is recorded.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("omits the message and the action when not given", () => {
    const { container } = render(<StatePanel title="Only a title" />);
    expect(container.querySelectorAll("button")).toHaveLength(0);
    expect(screen.getAllByText(/./)).toHaveLength(1);
  });

  it("marks its tone", () => {
    const { container, rerender } = render(<StatePanel title="t" />);
    expect(container.firstElementChild).toHaveAttribute("data-tone", "neutral");
    rerender(<StatePanel title="t" tone="error" />);
    expect(container.firstElementChild).toHaveAttribute("data-tone", "error");
  });

  it("has no axe violations", async () => {
    const { container } = render(<StatePanel title="Title" message="Message" action={<button type="button">Go</button>} tone="error" />);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
