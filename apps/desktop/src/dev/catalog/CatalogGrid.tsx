import { useState, type ReactNode } from "react";
import {
  Button,
  Chip,
  CopyCommand,
  Dialog,
  DialogSurface,
  Gear,
  IconButton,
  Menu,
  MenuSurface,
  Pill,
  Popover,
  PopoverSurface,
  RepoBadge,
  SegmentedControl,
  Skeleton,
  StatePanel,
  StatusDot,
  StatusPill,
  Table,
  Toggle,
  Tooltip,
  TooltipSurface,
  ToastSurface,
  Tree,
  useToast,
  type MenuEntry,
  type PillKind,
  type StatusTone,
  type TableColumn,
  type ToastKind,
  type TreeNode,
} from "@/ui";

type State = "rest" | "hover" | "focus" | "pressed" | "disabled";
const STATES: ReadonlyArray<State> = ["rest", "hover", "focus", "pressed", "disabled"];

/** `data-force` paints hover, focus and pressed without the user; disabled is a prop. */
const forced = (state: State) => (state === "hover" || state === "focus" || state === "pressed" ? { "data-force": state } : {});

function Section({ name, children }: { name: string; children: ReactNode }) {
  return (
    <section className="gr-cat-section">
      <h2>{name}</h2>
      {children}
    </section>
  );
}

function Surface({ control = false, children }: { control?: boolean; children: ReactNode }) {
  return <div className={control ? "gr-cat-surface gr-cat-surface-control" : "gr-cat-surface"}>{children}</div>;
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="gr-cat-row">
      <span className="gr-cat-label">{label}</span>
      {children}
    </div>
  );
}

const TREE: TreeNode[] = [
  {
    id: "repo-a",
    kind: "repo",
    label: "gitraptor",
    leading: <RepoBadge repoId="gitraptor" name="gitraptor" />,
    meta: "3",
    children: [
      {
        id: "worktrees",
        kind: "group",
        label: "Worktrees",
        children: [
          { id: "wt-1", kind: "item", label: "claude-1", accessibleName: "claude-1, active", leading: <StatusDot tone="active" decorative />, meta: "↑4" },
          { id: "wt-2", kind: "item", label: "feat-payments-with-a-very-long-branch-name-that-needs-an-ellipsis", leading: <StatusDot tone="conflict" decorative /> },
          { id: "wt-3", kind: "item", label: "missing-folder", disabled: true, leading: <StatusDot tone="inactive" decorative /> },
        ],
      },
      { id: "branches", kind: "group", label: "Local branches", children: [{ id: "br-1", kind: "item", label: "main" }] },
    ],
  },
  { id: "repo-b", kind: "repo", label: "another-repo", leading: <RepoBadge repoId="another-repo" name="another-repo" />, children: [{ id: "remotes", kind: "group", label: "Remotes", children: [] }] },
];

const COLUMNS_6: TableColumn[] = [
  { id: "branch", header: "Branch / Tag", track: "8rem" },
  { id: "graph", header: "Graph", track: "8rem", paintHover: false },
  { id: "msg", header: "Commit message", track: "minmax(10rem, 1fr)", primary: true },
  { id: "author", header: "Author", track: "8rem" },
  { id: "date", header: "Date / Time", track: "7rem" },
  { id: "sha", header: "Sha", track: "5rem" },
];
const COLUMNS_3 = COLUMNS_6.filter((c) => ["graph", "msg", "sha"].includes(c.id));
const COMMITS = [
  { id: "c1", msg: "Merge branch feat-payments" },
  { id: "c2", msg: "Fix the very long commit message that does not fit in its column and must be cut with an ellipsis" },
  { id: "c3", msg: "Initial commit" },
];

const CELL: Record<string, string> = { branch: "main", graph: "●─●", author: "Ana", date: "Nov 9, 10:30", sha: "a1b2c3d" };

function TableDemo({ columns, wipId, disabled, loading }: { columns: TableColumn[]; wipId?: string; disabled?: boolean; loading?: boolean }) {
  const [selected, setSelected] = useState<string | null>("c2");
  return (
    <div className="gr-cat-scroll">
      <Table<(typeof COMMITS)[number]>
        label="Commit graph"
        columns={columns}
        rows={COMMITS}
        getRowId={(r) => r.id}
        getRowLabel={(r) => r.msg}
        renderCell={(r, col) => (col === "msg" ? r.msg : (CELL[col] ?? ""))}
        selectedId={selected}
        onSelect={setSelected}
        onActivate={setSelected}
        {...(wipId ? { wipId } : {})}
        {...(disabled ? { disabled } : {})}
        {...(loading ? { loading } : {})}
        settings={<IconButton variant="ghost" size="sm" label="Column settings" icon={<Gear size="sm" />} />}
      />
    </div>
  );
}

const MENU_ENTRIES: MenuEntry[] = [
  { type: "item", id: "fetch", label: "Fetch all remotes", hint: "⌘F", onSelect: () => {} },
  { type: "item", id: "pull", label: "Pull (fast-forward)", disabled: true, disabledReason: "Available in UI-2", onSelect: () => {} },
  { type: "separator", id: "sep" },
  { type: "radio", id: "date", label: "Sort by date", checked: true, onSelect: () => {} },
  { type: "radio", id: "name", label: "Sort by name", checked: false, onSelect: () => {} },
  { type: "group", id: "view", label: "View", entries: [{ type: "checkbox", id: "tags", label: "Show tags", checked: true, onCheckedChange: () => {} }] },
];

function Switches() {
  const [a, setA] = useState(true);
  const [b, setB] = useState(false);
  const [c, setC] = useState(true);
  return (
    <>
      <Toggle label="Author" labelPosition="start" checked={a} onCheckedChange={setA} />
      <Toggle label="Date / Time" labelPosition="start" checked={b} onCheckedChange={setB} />
      <Toggle label="Sha" labelPosition="start" checked={c} onCheckedChange={setC} />
    </>
  );
}

function ToastTriggers() {
  const toast = useToast();
  const kinds: ToastKind[] = ["success", "info", "warning", "error"];
  return (
    <Row label="live">
      {kinds.map((kind) => (
        <Button
          key={kind}
          size="sm"
          onClick={() => toast.show({ kind, message: `A ${kind} toast`, ...(kind === "success" ? { action: { label: "Undo", onAction: () => {} } } : {}) })}
        >
          {`Show ${kind}`}
        </Button>
      ))}
    </Row>
  );
}

/** Every primitive: rows are variants and sizes, columns are states (spec § 5). */
export function CatalogGrid() {
  const [segment, setSegment] = useState<"graph" | "table">("graph");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set(["repo-a", "worktrees"]));
  const [selectedNode, setSelectedNode] = useState<string | null>("wt-1");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [infoOpen, setInfoOpen] = useState(false);
  const [chip, setChip] = useState(true);
  const [on, setOn] = useState(true);

  return (
    <>
      <Section name="Button">
        <Surface>
          {(["primary", "secondary", "danger", "link"] as const).flatMap((variant) =>
            (["md", "sm"] as const).map((size) => (
              <Row key={`${variant}-${size}`} label={`${variant} ${size}`}>
                {STATES.map((s) => (
                  <Button key={s} variant={variant} size={size} disabled={s === "disabled"} {...forced(s)}>
                    {s}
                  </Button>
                ))}
              </Row>
            )),
          )}
          <Row label="pill + icon + pending">
            <Button variant="primary" shape="pill" icon={<Gear size="sm" />}>
              Integrate changes
            </Button>
            <Button pending>Saving</Button>
          </Row>
        </Surface>
        <Surface control>
          <Row label="on control">
            <Button>Secondary</Button>
            <Button variant="danger">Danger</Button>
          </Row>
        </Surface>
      </Section>

      <Section name="IconButton">
        <Surface>
          {(["default", "ghost"] as const).flatMap((variant) =>
            (["md", "sm"] as const).map((size) => (
              <Row key={`${variant}-${size}`} label={`${variant} ${size}`}>
                {STATES.map((s) => (
                  <IconButton key={s} variant={variant} size={size} label={`${variant} ${size} ${s}`} icon={<Gear />} disabled={s === "disabled"} {...forced(s)} />
                ))}
              </Row>
            )),
          )}
        </Surface>
      </Section>

      <Section name="Chip">
        <Surface>
          <Row label="states">
            <Chip label="feat-pagos" />
            <Chip label="a1b2c3d" mono />
            <Chip label="↑4" mono />
            <Chip label="selected" selected={chip} onToggle={setChip} />
            <Chip label="removable" onRemove={() => {}} />
            <Chip label="disabled" disabled />
          </Row>
          <Row label="long label">
            <Chip label="feat/US-DSK-001-a-very-long-branch-name-that-needs-an-ellipsis" />
          </Row>
        </Surface>
        <Surface control>
          <Row label="on control">
            <Chip label="feat-pagos" />
            <Chip label="removable" onRemove={() => {}} />
          </Row>
        </Surface>
      </Section>

      <Section name="Pill">
        <Surface>
          <Row label="kinds">
            {(["reversible", "confirm", "irreversible", "live"] as PillKind[]).map((kind) => (
              <Pill key={kind} kind={kind} />
            ))}
          </Row>
        </Surface>
        <Surface control>
          <Row label="on control">
            <Pill kind="reversible" />
            <Pill kind="live" />
          </Row>
        </Surface>
      </Section>

      <Section name="StatusPill">
        <Surface>
          <Row label="tones">
            <StatusPill symbol="●" tone="success" label="Live" />
            <StatusPill symbol="◐" tone="warning" label="Catching up…" />
            <StatusPill symbol="✖" tone="danger" label="Engine stopped" />
            <StatusPill symbol="◌" tone="muted" label="Connecting to the engine…" />
          </Row>
        </Surface>
      </Section>

      <Section name="StatePanel">
        <Surface>
          <StatePanel title="Engine stopped" message="No history is being recorded for your repos." action={<Button size="sm">Retry now</Button>} />
          <StatePanel tone="error" title="Incompatible engine" message="The running engine is older than this app." />
        </Surface>
      </Section>

      <Section name="CopyCommand">
        <Surface>
          <Row label="command">
            <CopyCommand command="raptor daemon start" />
          </Row>
        </Surface>
      </Section>

      <Section name="Skeleton">
        <Surface>
          <Skeleton />
        </Surface>
      </Section>

      <Section name="StatusDot">
        <Surface>
          <Row label="tones">
            {(["active", "inactive", "conflict", "live"] as StatusTone[]).map((tone) => (
              <StatusDot key={tone} tone={tone} />
            ))}
          </Row>
        </Surface>
      </Section>

      <Section name="RepoBadge">
        <Surface>
          <Row label="lanes">
            {["gitraptor", "another-repo", "docs", "design-system", "api", "a-repository-with-a-long-name"].map((name) => (
              <RepoBadge key={name} repoId={name} name={name} />
            ))}
          </Row>
        </Surface>
      </Section>

      <Section name="SegmentedControl">
        <Surface>
          <Row label="rest">
            <SegmentedControl
              label="View"
              options={[
                { value: "graph", label: "Graph" },
                { value: "table", label: "Table" },
              ]}
              value={segment}
              onChange={setSegment}
            />
          </Row>
          <Row label="disabled">
            <SegmentedControl
              label="View disabled"
              options={[
                { value: "graph", label: "Graph" },
                { value: "table", label: "Table" },
              ]}
              value="graph"
              onChange={() => {}}
              disabled
            />
          </Row>
        </Surface>
      </Section>

      <Section name="Toggle">
        <Surface>
          <Row label="states">
            <Toggle label="Off" checked={false} onCheckedChange={() => {}} />
            <Toggle label="On" checked={on} onCheckedChange={setOn} />
            <Toggle label="Disabled" checked disabled onCheckedChange={() => {}} />
          </Row>
          <Row label="description">
            <Toggle label="With description" description="Always visible" checked onCheckedChange={() => {}} />
          </Row>
        </Surface>
      </Section>

      <Section name="Tree">
        <Surface>
          <div className="gr-cat-narrow">
            <Tree label="Repositories" nodes={TREE} expanded={expanded} onExpandedChange={setExpanded} selectedId={selectedNode} onSelect={setSelectedNode} onActivate={setSelectedNode} />
          </div>
          <div className="gr-cat-narrow">
            <Tree label="Repositories loading" nodes={[]} expanded={new Set()} onExpandedChange={() => {}} selectedId={null} onSelect={() => {}} onActivate={() => {}} loading />
          </div>
        </Surface>
      </Section>

      <Section name="Menu">
        <Surface>
          <Row label="surface">
            <MenuSurface label="Fetch options" entries={MENU_ENTRIES} highlightedId="fetch" />
          </Row>
          <Row label="live">
            <Menu label="Fetch options live" entries={MENU_ENTRIES} trigger={(props) => <Button {...props}>Fetch</Button>} />
          </Row>
        </Surface>
      </Section>

      <Section name="Popover">
        <Surface>
          <Row label="surface">
            <PopoverSurface label="Column settings">
              <Switches />
            </PopoverSurface>
          </Row>
          <Row label="live">
            <Popover label="Column settings live" trigger={(props) => <IconButton variant="ghost" label="Open column settings" icon={<Gear />} {...props} />}>
              <Switches />
            </Popover>
          </Row>
        </Surface>
      </Section>

      <Section name="Toast">
        <Surface>
          {(["success", "info", "warning", "error"] as ToastKind[]).map((kind) => (
            <Row key={kind} label={kind}>
              <ToastSurface kind={kind} message={`A ${kind} message`} onClose={() => {}} {...(kind === "success" ? { action: { label: "Undo", onAction: () => {} } } : {})} />
            </Row>
          ))}
          <ToastTriggers />
        </Surface>
      </Section>

      <Section name="Dialog">
        <Surface>
          <Row label="confirm">
            <DialogSurface
              kind="confirm"
              title="Delete feat-pagos?"
              description="The branch is removed from this repository. You can recover it from the Time Machine."
              confirm={{ label: "Delete", variant: "danger", onConfirm: () => {} }}
            />
          </Row>
          <Row label="info">
            <DialogSurface kind="info" title="Engine stopped" description="Restart it from Settings." acknowledge={{ label: "Got it" }} />
          </Row>
          <Row label="live">
            <Button onClick={() => setConfirmOpen(true)}>Open confirm</Button>
            <Button onClick={() => setInfoOpen(true)}>Open info</Button>
          </Row>
          <Dialog
            open={confirmOpen}
            onOpenChange={setConfirmOpen}
            kind="confirm"
            title="Delete feat-pagos?"
            description="The branch is removed from this repository."
            confirm={{ label: "Delete", variant: "danger", onConfirm: () => setConfirmOpen(false) }}
          />
          <Dialog open={infoOpen} onOpenChange={setInfoOpen} kind="info" title="Engine stopped" description="Restart it from Settings." acknowledge={{ label: "Got it" }} />
        </Surface>
      </Section>

      <Section name="Tooltip">
        <Surface>
          <Row label="surface">
            <TooltipSurface content="Left panel" shortcut="⌘B" />
          </Row>
          <Row label="live">
            <Tooltip content="Left panel" shortcut={{ keys: "⌘B", aria: "Meta+B" }}>
              <IconButton label="Toggle left panel" icon={<Gear />} />
            </Tooltip>
            <Tooltip content="Available in UI-2" describes>
              <Button disabled>Disabled with a reason</Button>
            </Tooltip>
          </Row>
        </Surface>
      </Section>

      <Section name="Table">
        <Surface>
          <TableDemo columns={COLUMNS_6} wipId="c1" />
          <TableDemo columns={COLUMNS_3} />
          <TableDemo columns={COLUMNS_3} disabled />
          <TableDemo columns={COLUMNS_3} loading />
        </Surface>
      </Section>
    </>
  );
}
