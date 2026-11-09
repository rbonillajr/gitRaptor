/* eslint-disable @typescript-eslint/no-unused-vars -- skeleton: remove with the implementation */
import type { BlockRow, CopiedException } from "./blocks-model";

export interface BlockDialogProps {
  row: BlockRow;
  repoId: string;
  fromToast?: boolean;
  onClose: () => void;
  onCopied: (copied: NonNullable<CopiedException>) => void;
}

/** SKELETON of the two-step block dialog, d1 and d4 (T005). */
export function BlockDialog(_props: BlockDialogProps) {
  return null;
}
