import { QueryClientProvider } from "@tanstack/react-query";
import "@/i18n";
import { AppShell } from "@/shell/AppShell";
import { queryClient } from "@/state/query-client";

/** Root of the webview: the query cache around the B1 window. The i18n is initialised on import. */
export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AppShell />
    </QueryClientProvider>
  );
}
