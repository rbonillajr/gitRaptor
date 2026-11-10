import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { call, CommandFailure } from "@/client/ipc";
import { useEngineStore } from "@/state/engine-store";
import { AutostartOffer } from "./AutostartOffer";

const OFFER_KEY = ["engine", "autostart-offer"] as const;
const ERROR_KEYS = { "not-offered": "error.notOffered", cancelled: "error.cancelled" } as const;

/**
 * Asks `engine_autostart_offer` only while the engine is live and shows the row for `offered` / `registered`.
 * Pressing it calls `engine_autostart { enable }`; a failure shows its translated error and the offer is asked again.
 */
export function AutostartSection() {
  const { t } = useTranslation();
  const live = useEngineStore((s) => s.conn.kind === "live");
  const client = useQueryClient();
  const offer = useQuery({ queryKey: OFFER_KEY, queryFn: () => call("engine_autostart_offer", {}), enabled: live });
  const choice = useMutation({
    mutationFn: (enable: boolean) => call("engine_autostart", { enable }),
    onSettled: () => client.invalidateQueries({ queryKey: OFFER_KEY }),
  });

  const kind = offer.data?.offer;
  if (!live || (kind !== "offered" && kind !== "registered")) return null;
  const code = choice.error instanceof CommandFailure ? choice.error.code : choice.error ? "failed" : null;
  const error = code === null ? null : t(code in ERROR_KEYS ? ERROR_KEYS[code as keyof typeof ERROR_KEYS] : "error.failed");
  return <AutostartOffer offer={kind} pending={choice.isPending} onChange={(enable) => choice.mutate(enable)} error={error} />;
}
