/* eslint-disable @typescript-eslint/no-unused-vars -- skeleton: remove with the implementation */
import type { BlockRow, CopiedException } from "./blocks-model";

export interface GuardrailsViewProps {
  repoName: string;
  live: boolean;
  onBack: () => void;
  onOpenRow: (row: BlockRow) => void;
  copied: CopiedException;
}

/** SKELETON of the Guardrails center view (T005). */
export function GuardrailsView(_props: GuardrailsViewProps) {
  return null;
}
