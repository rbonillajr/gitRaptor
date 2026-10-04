---
id: SEQ-GRP-CAMBIO
title: "Cambio en un worktree hasta el Cockpit"
type: diagram
status: expanded
domain: GRP
feature: motor-local
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013]
  stories: [US-GRP-002]
---

# Secuencia — Cambio en un worktree hasta el Cockpit (US-GRP-002)

```mermaid
sequenceDiagram
  autonumber
  participant A as Agente o desarrollador
  participant OS as SO (watcher nativo)
  participant D as raptor daemon
  participant G as crates/git (solo lectura)
  participant P as Perfil (SQLite)
  participant C as Cockpit / TUI

  A->>OS: Escribe un archivo (t0)
  OS->>D: Evento de archivo (t_recv, ≤ 50 ms)
  Note over D: Ventana fija de debounce de 75 ms por worktree (t_flush)
  D->>G: Recomputo incremental de las rutas tocadas
  G-->>D: Estado sin escrituras ni locks (t_computed)
  D->>P: Lote del debounce en una transacción (t_persisted)
  Note over D,P: Recomputo + persistencia ≤ 150 ms
  D->>C: Evento con secuencia y bloque de tiempos (t_client_recv, ≤ 25 ms)
  Note over A,C: Motor ≤ 300 ms p95 (ADR-GRP-011)
  C->>C: Aplica el delta y pinta (t_render, ≤ 100 ms)
  opt Cambio grande que no cabe en 150 ms
    D->>C: Primero rama, HEAD y operación en curso; después los recuentos
  end
```
