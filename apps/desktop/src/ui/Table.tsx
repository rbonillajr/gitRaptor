import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { cx, type CssVars } from "./cx";

export interface TableColumn {
  id: string;
  header: string;
  /** A `grid-template-columns` track; the screen gives it, it is not a token. */
  track: string;
  /** Default true; the graph column sets false so the hover does not hide the lanes. */
  paintHover?: boolean;
  /** The message column (dashed border on the WIP row). */
  primary?: boolean;
}

export interface TableProps<Row> {
  label: string;
  columns: ReadonlyArray<TableColumn>;
  rows: ReadonlyArray<Row>;
  getRowId: (row: Row) => string;
  renderCell: (row: Row, columnId: string) => ReactNode;
  /** `aria-label` of each row. */
  getRowLabel: (row: Row) => string;
  /** Total rows when the list is virtualised; drives `aria-rowcount`. */
  totalRows?: number;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onActivate: (id: string) => void;
  wipId?: string;
  /** More than one WIP row (the graph has one per worktree with changes). */
  wipIds?: ReadonlySet<string>;
  /** Absolute index of `rows[0]` in a virtualised list (default 0); `aria-rowindex` counts from it. */
  firstRowIndex?: number;
  /** When it changes and that row is painted, the row takes the focus (a virtual list scrolled it into view). */
  focusId?: string | null;
  /** A key that moves to a row outside the painted ones; `Home` is 0 and `End` is `totalRows - 1`. */
  onFocusOutside?: (absoluteIndex: number) => void;
  loading?: boolean;
  /** Engine stopped or disconnected: muted, no hover, keyboard still moves but selects nothing. */
  disabled?: boolean;
  /** The trigger of the Column settings popover. */
  settings?: ReactNode;
  className?: string;
}

export interface TableRowProps {
  id: string;
  label: string;
  rowIndex: number;
  selected: boolean;
  wip: boolean;
  tabbable: boolean;
  disabled: boolean;
  columns: ReadonlyArray<TableColumn>;
  children: (columnId: string) => ReactNode;
  onClick: () => void;
  ref?: Ref<HTMLDivElement>;
}

const PAGE = 10;
const CELL_FOCUSABLE = "button, a[href], input, select, textarea, [tabindex]";

/** The sticky header row; `settings` sits at its right edge. */
export function TableHeader({ columns, settings }: { columns: ReadonlyArray<TableColumn>; settings?: ReactNode }) {
  return (
    <div
      role="row"
      aria-rowindex={1}
      className="sticky top-0 z-(--gr-z-sticky) grid h-(--gr-size-graph-header) grid-cols-(--gr-table-columns) items-center border-b border-border-subtle text-text-muted [font:var(--gr-type-secondary)]"
    >
      {columns.map((c) => (
        <div key={c.id} role="columnheader" className="truncate px-(--gr-space-3)">
          {c.header}
        </div>
      ))}
      {settings ? (
        <div role="gridcell" className="absolute right-(--gr-space-2) flex items-center">
          {settings}
        </div>
      ) : null}
    </div>
  );
}

/** One grid row: a single Tab stop; the cells the screen renders go in as `children(columnId)`. */
export function TableRow({ id, label, rowIndex, selected, wip, tabbable, disabled, columns, children, onClick, ref }: TableRowProps) {
  return (
    <div
      ref={ref}
      role="row"
      data-row-id={id}
      aria-label={label}
      aria-rowindex={rowIndex}
      aria-selected={selected}
      tabIndex={tabbable ? 0 : -1}
      onClick={onClick}
      className={cx(
        "relative grid h-(--gr-size-graph-row) grid-cols-(--gr-table-columns) items-center cursor-pointer select-none gr-ring-inset gr-motion",
        disabled ? "text-text-muted" : "text-text-main",
        wip && "bg-accent-glow",
        selected && !wip && "bg-accent-glow",
      )}
    >
      {selected ? <span aria-hidden="true" className="pointer-events-none absolute inset-y-0 left-0 w-(--gr-size-ring) bg-accent" /> : null}
      {columns.map((c) => (
        <div
          key={c.id}
          role="gridcell"
          className={cx(
            "flex h-full min-w-0 items-center truncate px-(--gr-space-3) gr-motion",
            !disabled && !selected && !wip && c.paintHover !== false && "in-gr-hover:bg-surface-hover",
          )}
        >
          <span
            className={cx(
              "min-w-0 truncate",
              wip && c.primary && "rounded-sm border border-dashed border-accent-border px-(--gr-space-2)",
            )}
          >
            {children(c.id)}
          </span>
        </div>
      ))}
    </div>
  );
}

/**
 * The commit-graph grid (APG data grid with focus per row, D11): flat rows, one Tab stop, Right enters the
 * focusable cells of the row and Left or Escape comes back.
 */
export function Table<Row>({
  label,
  columns,
  rows,
  getRowId,
  renderCell,
  getRowLabel,
  totalRows,
  selectedId,
  onSelect,
  onActivate,
  wipId,
  wipIds,
  firstRowIndex = 0,
  focusId = null,
  onFocusOutside,
  loading = false,
  disabled = false,
  settings,
  className,
}: TableProps<Row>) {
  const { t } = useTranslation();
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const rowEls = useRef(new Map<string, HTMLDivElement>());
  const ids = rows.map(getRowId);

  const has = (id: string | null) => id !== null && ids.includes(id);
  const stopId = has(focusedId) ? focusedId : has(selectedId) ? selectedId : (ids[0] ?? null);
  const style: CssVars = { "--gr-table-columns": columns.map((c) => c.track).join(" ") };

  const focusRow = (index: number) => {
    const id = ids[Math.max(0, Math.min(index, ids.length - 1))];
    if (id === undefined) return;
    setFocusedId(id);
    rowEls.current.get(id)?.focus();
  };
  // A row the caller asked to focus (virtual list): focus it once, when it is painted.
  const focusedFor = useRef<string | null>(null);
  useEffect(() => {
    if (focusId === null) {
      focusedFor.current = null;
      return;
    }
    if (focusedFor.current === focusId) return;
    const el = rowEls.current.get(focusId);
    if (!el) return;
    focusedFor.current = focusId;
    el.focus();
  });
  const total = totalRows ?? rows.length;
  /** Moves to the absolute index `target`: a painted row takes the focus, another one is the caller's to scroll to. */
  const moveTo = (target: number) => {
    const absolute = Math.max(0, Math.min(target, total - 1));
    const local = absolute - firstRowIndex;
    if (onFocusOutside && (local < 0 || local >= ids.length)) onFocusOutside(absolute);
    else focusRow(local);
  };
  const choose = (id: string) => {
    if (!disabled) onSelect(id);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!(e.target instanceof HTMLElement)) return;
    const rowEl = e.target.closest<HTMLElement>("[data-row-id]");
    const id = rowEl?.dataset["rowId"];
    if (!rowEl || id === undefined) return;
    const at = ids.indexOf(id);

    if (e.target !== rowEl) {
      const cells = Array.from(rowEl.querySelectorAll<HTMLElement>(CELL_FOCUSABLE)).filter((el) => el !== rowEl);
      const i = cells.indexOf(e.target);
      if (e.key === "Escape") {
        e.preventDefault();
        rowEl.focus();
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        cells[i + 1]?.focus();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        (i <= 0 ? rowEl : cells[i - 1])?.focus();
      }
      return;
    }

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        moveTo(firstRowIndex + at + 1);
        return;
      case "ArrowUp":
        e.preventDefault();
        moveTo(firstRowIndex + at - 1);
        return;
      case "PageDown":
        e.preventDefault();
        moveTo(firstRowIndex + at + PAGE);
        return;
      case "PageUp":
        e.preventDefault();
        moveTo(firstRowIndex + at - PAGE);
        return;
      case "Home":
        e.preventDefault();
        moveTo(0);
        return;
      case "End":
        e.preventDefault();
        moveTo(total - 1);
        return;
      case "Enter":
        e.preventDefault();
        if (disabled) return;
        onSelect(id);
        onActivate(id);
        return;
      case "ArrowRight": {
        e.preventDefault();
        Array.from(rowEl.querySelectorAll<HTMLElement>(CELL_FOCUSABLE)).find((el) => el !== rowEl)?.focus();
        return;
      }
      default:
    }
  };

  return (
    <div
      role="grid"
      aria-label={label}
      aria-rowcount={total + 1}
      aria-colcount={columns.length}
      aria-busy={loading ? "true" : undefined}
      aria-disabled={disabled ? "true" : undefined}
      onKeyDown={onKeyDown}
      onFocus={(e) => {
        const id = e.target.closest<HTMLElement>("[data-row-id]")?.dataset["rowId"];
        if (id !== undefined) setFocusedId(id);
      }}
      style={style}
      className={cx("relative flex flex-col", className)}
    >
      <TableHeader columns={columns} {...(settings ? { settings } : {})} />
      {loading ? (
        <>
          {[0, 1, 2, 3].map((i) => (
            <div key={i} aria-hidden="true" className="mx-(--gr-space-3) mt-(--gr-space-2) h-(--gr-size-graph-row) rounded-sm bg-surface-hover" />
          ))}
          <span className="sr-only">{t("ui.table.loading")}</span>
        </>
      ) : (
        rows.map((row, i) => {
          const id = ids[i] ?? "";
          return (
            <TableRow
              key={id}
              ref={(el) => {
                if (el) rowEls.current.set(id, el);
                else rowEls.current.delete(id);
              }}
              id={id}
              label={getRowLabel(row)}
              rowIndex={firstRowIndex + i + 2}
              selected={id === selectedId}
              wip={id === wipId || wipIds?.has(id) === true}
              tabbable={id === stopId}
              disabled={disabled}
              columns={columns}
              onClick={() => choose(id)}
            >
              {(columnId) => renderCell(row, columnId)}
            </TableRow>
          );
        })
      )}
    </div>
  );
}
