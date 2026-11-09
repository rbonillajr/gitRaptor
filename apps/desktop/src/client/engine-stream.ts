import { Channel } from "@tauri-apps/api/core";
import type { EngineMessage } from "./engine-types";
import { call } from "./ipc";

/** `receivedAt`: `performance.now()` when the first message of the batch arrived (the start of `gr:fleet-delta`). */
export interface BatchMeta {
  receivedAt: number;
}

/**
 * Opens the engine stream: one `Channel`, messages accumulated as they arrive and handed to `onBatch` in a single
 * batch per animation frame, in order (NFR-13.1). Call it once per webview load (from `App`).
 */
export async function watchEngine(onBatch: (batch: EngineMessage[], meta: BatchMeta) => void): Promise<void> {
  let pending: EngineMessage[] = [];
  let receivedAt = 0;
  let scheduled = false;

  const flush = () => {
    scheduled = false;
    const batch = pending;
    pending = [];
    if (batch.length > 0) onBatch(batch, { receivedAt });
  };

  const channel = new Channel<EngineMessage>();
  channel.onmessage = (message) => {
    if (pending.length === 0) receivedAt = performance.now();
    pending.push(message);
    if (!scheduled) {
      scheduled = true;
      requestAnimationFrame(flush);
    }
  };
  await call("engine_watch", { channel });
}
