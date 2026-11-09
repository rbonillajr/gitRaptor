import { useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { cx, type CssVars } from "./cx";
import { useTypeahead } from "./hooks/useTypeahead";
import { ChevronRight } from "./icons";

export type TreeNodeKind = "repo" | "group" | "item";

export interface TreeNode {
  id: string;
  kind: TreeNodeKind;
  label: string;
  /** The full phrase a screen reader reads; defaults to `label`. */
  accessibleName?: string;
  leading?: ReactNode;
  meta?: ReactNode;
  /** `aria-disabled`, muted text, no actions. */
  disabled?: boolean;
  children?: ReadonlyArray<TreeNode>;
}

export interface TreeProps {
  label: string;
  nodes: ReadonlyArray<TreeNode>;
  expanded: ReadonlySet<string>;
  onExpandedChange: (next: ReadonlySet<string>) => void;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onActivate: (id: string) => void;
  loading?: boolean;
  className?: string;
}

export interface TreeRowProps {
  node: TreeNode;
  level: number;
  posInSet: number;
  setSize: number;
  expanded: boolean | undefined;
  selected: boolean;
  tabbable: boolean;
  onChevronClick: () => void;
  onRowClick: () => void;
  ref?: Ref<HTMLDivElement>;
}

interface FlatRow {
  node: TreeNode;
  level: number;
  posInSet: number;
  setSize: number;
  parentId: string | null;
}

const HEIGHT: Record<TreeNodeKind, string> = {
  repo: "h-(--gr-size-tree-repo) [font:var(--gr-type-body-strong)]",
  item: "h-(--gr-size-tree-branch) [font:var(--gr-type-body)]",
  group: "h-(--gr-size-tree-group) [font:var(--gr-type-caption)]",
};

const isParent = (node: TreeNode) => (node.children?.length ?? 0) > 0;

/** Visible rows in depth-first order: a node's children only follow it when it is expanded. */
function flatten(nodes: ReadonlyArray<TreeNode>, expanded: ReadonlySet<string>, level = 1, parentId: string | null = null): FlatRow[] {
  return nodes.flatMap((node, i) => {
    const row: FlatRow = { node, level, posInSet: i + 1, setSize: nodes.length, parentId };
    return isParent(node) && expanded.has(node.id) ? [row, ...flatten(node.children ?? [], expanded, level + 1, node.id)] : [row];
  });
}

/** One `role="treeitem"` row: chevron, leading glyph, ellipsised label and a right-hand meta slot. */
export function TreeRow({ node, level, posInSet, setSize, expanded, selected, tabbable, onChevronClick, onRowClick, ref }: TreeRowProps) {
  const style: CssVars = { paddingInlineStart: "calc((var(--gr-tree-level) - 1) * var(--gr-space-4) + var(--gr-space-2))", "--gr-tree-level": level };
  const lineStyle = (k: number): CssVars => ({
    "--gr-tree-level": k,
    left: "calc(var(--gr-tree-level) * var(--gr-space-4) + var(--gr-space-2) + var(--gr-space-2))",
  });
  const lines = Array.from({ length: level - 1 }, (_, k) => k);
  return (
    <div
      ref={ref}
      role="treeitem"
      data-node-id={node.id}
      aria-label={node.accessibleName ?? node.label}
      aria-level={level}
      aria-posinset={posInSet}
      aria-setsize={setSize}
      aria-expanded={expanded}
      aria-selected={selected}
      aria-disabled={node.disabled ? "true" : undefined}
      tabIndex={tabbable ? 0 : -1}
      onClick={onRowClick}
      style={style}
      className={cx(
        "relative mx-(--gr-space-2) flex shrink-0 items-center gap-(--gr-space-2) rounded-sm pe-(--gr-space-2) cursor-pointer select-none gr-ring-inset gr-motion",
        HEIGHT[node.kind],
        node.kind === "group" || node.disabled ? "text-text-muted" : "text-text-main",
        selected ? "bg-accent-glow text-text-main" : "gr-hover:bg-surface-hover gr-hover:text-text-main",
        node.disabled && "cursor-default",
      )}
    >
      {lines.map((k) => (
        <span
          key={k}
          aria-hidden="true"
          style={lineStyle(k)}
          className="pointer-events-none absolute inset-y-0 w-px bg-border-subtle"
        />
      ))}
      {selected ? <span aria-hidden="true" className="pointer-events-none absolute inset-y-0 left-0 w-(--gr-size-ring) bg-accent" /> : null}
      <span
        aria-hidden="true"
        onClick={
          isParent(node)
            ? (e) => {
                e.stopPropagation();
                onChevronClick();
              }
            : undefined
        }
        className={cx("inline-flex size-(--gr-space-4) shrink-0 items-center justify-center text-text-muted gr-motion-transform", expanded && "rotate-90")}
      >
        {isParent(node) ? <ChevronRight /> : null}
      </span>
      {node.leading}
      <span className="min-w-0 flex-1 truncate">{node.label}</span>
      {node.meta ? <span className="shrink-0 text-text-muted [font:var(--gr-type-mono)]">{node.meta}</span> : null}
    </div>
  );
}

/**
 * The sidebar tree (APG tree, D10): visible rows are flat direct children of `role="tree"`, each carrying
 * level, set size and position. One Tab stop; selection does not follow the focus.
 */
export function Tree({ label, nodes, expanded, onExpandedChange, selectedId, onSelect, onActivate, loading = false, className }: TreeProps) {
  const { t } = useTranslation();
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const rowEls = useRef(new Map<string, HTMLDivElement>());
  const visible = flatten(nodes, expanded);
  const typeahead = useTypeahead(() => visible.map((r) => r.node.label));

  const has = (id: string | null) => id !== null && visible.some((r) => r.node.id === id);
  const stopId = has(focusedId) ? focusedId : has(selectedId) ? selectedId : (visible[0]?.node.id ?? null);

  const toggle = (id: string) => {
    const next = new Set(expanded);
    if (!next.delete(id)) next.add(id);
    onExpandedChange(next);
  };
  const focusRow = (id: string | undefined) => {
    if (id === undefined) return;
    setFocusedId(id);
    rowEls.current.get(id)?.focus();
  };
  const select = (row: FlatRow) => {
    if (!row.node.disabled) onSelect(row.node.id);
  };
  const rowClick = (row: FlatRow) => {
    if (row.node.disabled) return;
    onSelect(row.node.id);
    if (row.node.kind !== "item" && isParent(row.node)) toggle(row.node.id);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const id = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>("[data-node-id]")?.dataset["nodeId"] : undefined;
    const at = visible.findIndex((r) => r.node.id === id);
    const row = visible[at];
    if (!row) return;
    const open = isParent(row.node) && expanded.has(row.node.id);

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusRow(visible[at + 1]?.node.id);
        return;
      case "ArrowUp":
        e.preventDefault();
        focusRow(visible[at - 1]?.node.id);
        return;
      case "Home":
        e.preventDefault();
        focusRow(visible[0]?.node.id);
        return;
      case "End":
        e.preventDefault();
        focusRow(visible[visible.length - 1]?.node.id);
        return;
      case "ArrowRight":
        e.preventDefault();
        if (isParent(row.node)) {
          if (!open) toggle(row.node.id);
          else focusRow(visible[at + 1]?.node.id);
        }
        return;
      case "ArrowLeft":
        e.preventDefault();
        if (open) toggle(row.node.id);
        else if (row.parentId !== null) focusRow(row.parentId);
        return;
      case "Enter":
        e.preventDefault();
        if (row.node.disabled) return;
        onSelect(row.node.id);
        onActivate(row.node.id);
        if (row.node.kind !== "item" && isParent(row.node)) toggle(row.node.id);
        return;
      case " ":
        e.preventDefault();
        select(row);
        return;
      case "*": {
        e.preventDefault();
        const next = new Set(expanded);
        for (const sibling of visible) if (sibling.parentId === row.parentId && isParent(sibling.node)) next.add(sibling.node.id);
        onExpandedChange(next);
        return;
      }
      default:
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          const hit = typeahead(e.key, at);
          if (hit !== null) focusRow(visible[hit]?.node.id);
        }
    }
  };

  return (
    <div
      role="tree"
      aria-label={label}
      aria-busy={loading ? "true" : undefined}
      onKeyDown={onKeyDown}
      onFocus={(e) => {
        const id = e.target.closest<HTMLElement>("[data-node-id]")?.dataset["nodeId"];
        if (id !== undefined) setFocusedId(id);
      }}
      className={cx("flex flex-col", className)}
    >
      {loading ? (
        <>
          {[0, 1, 2, 3].map((i) => (
            <div key={i} aria-hidden="true" className="mx-(--gr-space-2) mb-(--gr-space-1) h-(--gr-size-tree-branch) rounded-sm bg-surface-hover" />
          ))}
          <span className="sr-only">{t("ui.tree.loading")}</span>
        </>
      ) : (
        visible.map((r) => (
          <TreeRow
            key={r.node.id}
            ref={(el) => {
              if (el) rowEls.current.set(r.node.id, el);
              else rowEls.current.delete(r.node.id);
            }}
            node={r.node}
            level={r.level}
            posInSet={r.posInSet}
            setSize={r.setSize}
            expanded={isParent(r.node) ? expanded.has(r.node.id) : undefined}
            selected={r.node.id === selectedId}
            tabbable={r.node.id === stopId}
            onChevronClick={() => toggle(r.node.id)}
            onRowClick={() => rowClick(r)}
          />
        ))
      )}
    </div>
  );
}
