import { Channel } from "@tauri-apps/api/core";
import type { EngineMessage } from "./engine-types";
import { call } from "./ipc";

/**
 * Opens the engine stream: one `Channel`, messages accumulated as they arrive and handed to `onBatch` in a single
 * batch per animation frame, in order (NFR-13.1). Call it once per webview load (from `App`).
 */
export async function watchEngine(onBatch: (batch: EngineMessage[]) => void): Promise<void> {
  let pending: EngineMessage[] = [];
  let scheduled = false;

  const flush = () => {
    scheduled = false;
    const batch = pending;
    pending = [];
    if (batch.length > 0) onBatch(batch);
  };

  const channel = new Channel<EngineMessage>();
  channel.onmessage = (message) => {
    pending.push(message);
    if (!scheduled) {
      scheduled = true;
      requestAnimationFrame(flush);
    }
  };
  await call("engine_watch", { channel });
}
