import { QueryClient } from "@tanstack/react-query";

/**
 * The one QueryClient of the webview. Engine data never goes stale on its own: the engine deltas
 * arrive through `setQueryData` (ADR-GRP-004, Enmienda D7).
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: Infinity, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false },
  },
});
