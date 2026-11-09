import { useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
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
        focusRow(at + 1);
        return;
      case "ArrowUp":
        e.preventDefault();
        focusRow(at - 1);
        return;
      case "PageDown":
        e.preventDefault();
        focusRow(at + PAGE);
        return;
      case "PageUp":
        e.preventDefault();
        focusRow(at - PAGE);
        return;
      case "Home":
        e.preventDefault();
        focusRow(0);
        return;
      case "End":
        e.preventDefault();
        focusRow(ids.length - 1);
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
      aria-rowcount={(totalRows ?? rows.length) + 1}
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
              rowIndex={i + 2}
              selected={id === selectedId}
              wip={id === wipId}
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
