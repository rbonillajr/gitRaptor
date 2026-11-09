// TS-DSK-003 T007: CopyCommand shows the command as <code> and copies it with navigator.clipboard.writeText.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { CopyCommand } from "./CopyCommand";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
let writeText: ReturnType<typeof vi.fn>;

beforeEach(async () => {
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  await setLang("en");
});
afterEach(cleanup);

describe("CopyCommand", () => {
  it("shows the command as code text", () => {
    render(<CopyCommand command="raptor daemon start" />);
    const code = screen.getByText("raptor daemon start");
    expect(code.tagName).toBe("CODE");
  });

  it.each([
    ["en", "Copy"],
    ["es", "Copiar"],
  ] as const)("labels the button in %s", async (lang, label) => {
    await setLang(lang);
    render(<CopyCommand command="x" />);
    expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
  });

  it("copies exactly the command on click", async () => {
    render(<CopyCommand command={"/opt/my app/raptor daemon stop"} />);
    await act(async () => void fireEvent.click(screen.getByRole("button", { name: "Copy" })));
    expect(writeText).toHaveBeenCalledExactlyOnceWith("/opt/my app/raptor daemon stop");
  });

  it("survives a clipboard that refuses", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    render(<CopyCommand command="x" />);
    await act(async () => void fireEvent.click(screen.getByRole("button", { name: "Copy" })));
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
  });

  it("has no axe violations", async () => {
    const { container } = render(<CopyCommand command="raptor daemon start" />);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
