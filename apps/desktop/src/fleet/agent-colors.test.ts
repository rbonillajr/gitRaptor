// DS-US-DSK-004 D7: stable colour slots per present session.
import { describe, expect, it } from "vitest";
import { assignSlots } from "./agent-colors";
import { session } from "./fixtures";

const s = (id: string, startedUtcMs: number) => session(id, "w", { startedUtcMs });
const slotsOf = (m: Map<string, number>) => Object.fromEntries(m);

describe("assignSlots", () => {
  it("hands out the lowest free slots in (started, id) order", () => {
    const m = assignSlots(new Map(), [s("c", 3), s("a", 1), s("b", 1)]);
    expect(slotsOf(m)).toEqual({ a: 1, b: 2, c: 3 });
  });

  it("a present session keeps its slot", () => {
    const first = assignSlots(new Map(), [s("a", 1), s("b", 2)]);
    const next = assignSlots(first, [s("b", 2), s("a", 1), s("older", 0)]);
    expect(slotsOf(next)).toEqual({ a: 1, b: 2, older: 3 });
  });

  it("slots are reused after an end", () => {
    const first = assignSlots(new Map(), [s("a", 1), s("b", 2), s("c", 3)]);
    const afterEnd = assignSlots(first, [s("a", 1), s("c", 3)]);
    expect(slotsOf(afterEnd)).toEqual({ a: 1, c: 3 });
    expect(slotsOf(assignSlots(afterEnd, [s("a", 1), s("c", 3), s("d", 4)]))).toEqual({ a: 1, c: 3, d: 2 });
  });

  it("the eighth agent repeats slot 7", () => {
    const present = Array.from({ length: 9 }, (_, i) => s(`s${i}`, i));
    const m = assignSlots(new Map(), present);
    expect([...m.values()]).toEqual([1, 2, 3, 4, 5, 6, 7, 7, 7]);
  });

  it("forgets sessions that are no longer present and does not mutate its input", () => {
    const prev = new Map([["gone", 1]]);
    const m = assignSlots(prev, [s("a", 1)]);
    expect(slotsOf(m)).toEqual({ a: 1 });
    expect(slotsOf(prev)).toEqual({ gone: 1 });
  });
});
