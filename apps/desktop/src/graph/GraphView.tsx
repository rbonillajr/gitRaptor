import type { ReactElement, ReactNode } from "react";
import type { GraphLayer, GraphModel } from "./graph-types";

export interface GraphViewProps {
  model: GraphModel;
  repoName: string;
  stale: boolean;
  layers?: readonly GraphLayer[];
  laneBadge?: (key: string) => ReactNode;
}

/** STUB (T001): the real view is T005. */
export function GraphView(props: GraphViewProps): ReactElement {
  void props;
  throw new Error("not implemented");
}
