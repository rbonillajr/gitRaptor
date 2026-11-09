import { create } from "zustand";

/** How many blocks of the open repo the person has not seen in this run: the ⛔N of the sidebar row (D11). In memory only. */
export const useGuardUnseen = create<{ count: number; set: (count: number) => void }>()((set) => ({
  count: 0,
  set: (count) => set({ count }),
}));
