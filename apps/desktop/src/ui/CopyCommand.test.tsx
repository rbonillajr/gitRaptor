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

// D8-BAND (DS-US-DSK-008 D22, WCAG 4.1.3): the result of the copy is announced and onCopied follows a successful write.
describe("CopyCommand announces the result (D22)", () => {
  const click = () => act(async () => void fireEvent.click(screen.getByRole("button", { name: /^(Copy|Copied)$/ })));

  it("a successful copy says Copied for 2 s and announces it once in a status region", async () => {
    vi.useFakeTimers();
    try {
      render(<CopyCommand command="raptor guard confirm" />);
      const region = screen.getByRole("status");
      expect(region.textContent, "the status region exists from the mount and is empty").toBe("");
      await click();
      expect(screen.getByRole("button", { name: "Copied" }), "the button says Copied").toBeInTheDocument();
      expect(screen.getByRole("status").textContent, "the region announces it once").toBe("Copied");
      act(() => void vi.advanceTimersByTime(1999));
      expect(screen.getByRole("button", { name: "Copied" }), "still Copied before 2 s").toBeInTheDocument();
      act(() => void vi.advanceTimersByTime(1));
      expect(screen.getByRole("button", { name: "Copy" }), "back to Copy at 2 s").toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("the announcement follows the language", async () => {
    await setLang("es");
    render(<CopyCommand command="x" />);
    await act(async () => void fireEvent.click(screen.getByRole("button", { name: "Copiar" })));
    expect(screen.getByRole("status").textContent).toBe("Copiado");
  });

  it("a failed copy announces it", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    render(<CopyCommand command="x" />);
    await click();
    expect(screen.getByRole("status").textContent, "the region says it failed").toBe("Couldn't copy. Select the command and copy it by hand.");
    expect(screen.getByRole("button", { name: "Copy" }), "the button stays Copy").toBeInTheDocument();
  });

  it("onCopied runs only after a successful write", async () => {
    const onCopied = vi.fn();
    let release: () => void = () => {};
    writeText.mockReturnValue(new Promise<void>((resolve) => (release = resolve)));
    render(<CopyCommand command="raptor guard confirm" onCopied={onCopied} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    expect(onCopied, "not before the write resolves").not.toHaveBeenCalled();
    await act(async () => release());
    expect(onCopied).toHaveBeenCalledExactlyOnceWith("raptor guard confirm");
    cleanup();
    onCopied.mockClear();
    writeText.mockRejectedValue(new Error("denied"));
    render(<CopyCommand command="x" onCopied={onCopied} />);
    await click();
    expect(onCopied, "never after a failed write").not.toHaveBeenCalled();
  });
});
