import { queryOptions } from "@tanstack/react-query";
import { call } from "./ipc";

/** Version and protocol of the app backend. */
export const appInfoQuery = queryOptions({
  queryKey: ["app", "info"],
  queryFn: () => call("app_info", {}),
});
