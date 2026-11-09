import { cx } from "./cx";

/** The shape of a `WorktreeCard` while the fleet loads: static bars, hidden from assistive tech (the list says "loading"). */
export function CardSkeleton({ className }: { className?: string }) {
  return (
    <div aria-hidden="true" className={cx("flex flex-col gap-(--gr-space-2) rounded-md bg-surface-control p-(--gr-space-3)", className)}>
      <span className="block h-(--gr-space-3) w-full rounded-sm bg-surface-hover" />
      <span className="block h-(--gr-space-3) w-full rounded-sm bg-surface-hover" />
      <span className="block h-(--gr-space-3) w-full rounded-sm bg-surface-hover" />
    </div>
  );
}
