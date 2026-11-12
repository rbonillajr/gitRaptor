import { useTranslation } from "react-i18next";
import { Button, Gear, IconButton, Popover, Toggle } from "@/ui";
import { folded, useGraphPrefs } from "./graph-prefs";
import type { GraphColumnId } from "./graph-types";

/** Graph is not offered: it never folds nor hides (D11), so a switch for it would do nothing. */
const TOGGLES: ReadonlyArray<Exclude<GraphColumnId, "graph">> = ["branch", "message", "author", "date", "sha"];

/**
 * The gear of the header and its popover (primitives-spec § 2.11): one switch per column, Commit message always on,
 * "Reset". A column that does not fit at this width says so and keeps the switch as the person left it.
 */
export function ColumnSettings({ availableWidthPx }: { availableWidthPx: number }) {
  const { t } = useTranslation();
  const columns = useGraphPrefs((s) => s.columns);
  const setColumn = useGraphPrefs((s) => s.setColumn);
  const resetColumns = useGraphPrefs((s) => s.resetColumns);
  const view = useGraphPrefs((s) => s.view);
  const doesNotFit = new Set(folded({ columns, view }, availableWidthPx));

  return (
    <Popover
      label={t("graph.settings.title")}
      trigger={(props) => <IconButton {...props} variant="ghost" size="sm" label={t("ui.table.settings")} icon={<Gear />} />}
    >
      <h3 className="m-0 text-text-main [font:var(--gr-type-body-strong)]">{t("graph.settings.title")}</h3>
      {TOGGLES.map((id) =>
        id === "message" ? (
          <Toggle key={id} label={t("graph.column.message")} checked disabled description={t("graph.settings.alwaysVisible")} onCheckedChange={() => undefined} labelPosition="start" className="justify-between" />
        ) : (
          <Toggle
            key={id}
            label={t(`graph.column.${id}`)}
            checked={columns[id]}
            onCheckedChange={(on) => setColumn(id, on)}
            {...(doesNotFit.has(id) ? { description: t("graph.settings.folded") } : {})}
            labelPosition="start"
            className="justify-between"
          />
        ),
      )}
      <Button variant="link" size="sm" className="self-start" onClick={resetColumns}>
        {t("graph.settings.reset")}
      </Button>
    </Popover>
  );
}
