import type { Ref } from "react";

export interface ConflictBandProps {
  variant: "row" | "badge";
  level: "conflict" | "overlap";
  aLabel: string;
  bLabel: string;
  firstFile: string | null;
  moreText: string | null;
  freshnessText: string;
  stale: boolean;
  isNew: boolean;
  a11yLabel: string;
  onOpen?: () => void;
  openRef?: Ref<HTMLButtonElement>;
}

/** Skeleton (T005). */
export function ConflictBand(props: ConflictBandProps): null {
  void props;
  return null;
}
