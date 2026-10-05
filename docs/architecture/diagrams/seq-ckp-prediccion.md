---
id: SEQ-CKP-PREDICCION
title: "Predicción de conflictos: de un commit en un worktree al ⚡ en la TUI"
type: diagram
status: expanded
domain: GRP
feature: cockpit
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-001, ADR-CKP-003, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013]
  stories: [TS-CKP-001, INF-CKP-001, SPIKE-CKP-001, US-CKP-006, US-CKP-007, US-CKP-008]
---

# Secuencia — Predicción de conflictos: de un commit en un worktree al ⚡ en la TUI (BR-06)

> Covers: BR-06 (BR-CKP-CALC-002, CALC-003, WF-005, WF-007) · TS-CKP-001, INF-CKP-001 · ADR-CKP-001 § 2 a § 8, ADR-CKP-003 § 3, § 4 y § 8, ADR-GRP-010, ADR-GRP-011 · US-CKP-006, US-CKP-007, US-CKP-008.

Un agente commitea en el worktree W1. El motor publica el cambio de W1 dentro de su presupuesto de 300 ms (ADR-GRP-011) y avisa al predictor, que corre en su propio pool de prioridad baja. El solape se recalcula al momento. El merge en seco se hace solo en los pares afectados que no resuelven la caché ni el prefiltro, en memoria y sin escribir en el repo. Cada resultado sale con la secuencia del motor y su hora de cálculo; la TUI deriva la antigüedad con su reloj de vista. `raptor conflicts` y `check_conflicts` del MCP leen el mismo estado.

```mermaid
sequenceDiagram
  autonumber
  participant AG as Agente en W1
  participant G as git (sistema)
  participant OB as Daemon · observador y recomputo
  participant M as Daemon · motor (estado y secuencia por repo)
  participant PR as Daemon · predictor (pool de prioridad baja)
  participant GL as crates/git · merge en seco (solo lectura)
  participant CA as Canal (instantánea y stream)
  participant T as TUI (cliente y bucle)

  AG->>G: git commit en W1
  G-->>AG: fin del commit (t0)
  OB->>OB: evento en el directorio Git de W1 y debounce
  OB->>M: recomputo de W1 con nueva punta y rutas sin commitear (en memoria)
  M->>CA: cambio de W1 con secuencia s y bloque de tiempos
  CA-->>T: evento s (t_client_recv)
  T->>T: ingesta y saneado, render coalescido de la fila W1 (t_render, ≤ 100 ms p95)
  M->>PR: disparador "cambió la punta de W1"
  PR->>PR: solape de los pares de W1 (intersección de rutas, incluye lo sin commitear)
  PR->>PR: encola los pares de W1 en la cola coalescente (prioridad: contra la base, luego con sesión presente)
  PR->>M: pares de W1 en "recalculando" (el resultado anterior queda desactualizado) y solape vigente
  M->>CA: evento s+1
  CA-->>T: evento s+1
  T->>T: ⚠ actualizado y ⚡ anterior marcado desactualizado, nunca como actual

  loop por cada par afectado (como mucho 10 con 10 worktrees)
    alt mismas ids de commit (punta A, punta B, merge-base) en la caché
      PR->>PR: el resultado sigue valiendo
    else las rutas commiteadas no se cortan (prefiltro, si SPIKE-CKP-001 lo mantiene)
      PR->>PR: "sin conflicto" sin ejecutar el merge
    else merge en seco
      PR->>GL: merge(id de commit A, id de commit B)
      GL->>GL: merge-base y merge de árboles en memoria, sin drivers, filtros, atributos, reemplazos ni descargas
      GL->>GL: interrumpible al vencer el tiempo por par, tope de blob por cabecera y límite fijo de renombrados (M-06)
      GL-->>PR: archivos en conflicto con su tipo y hunks como rangos de líneas
    end
    alt las entradas del par cambiaron durante el cálculo
      PR->>PR: descarta el resultado y deja el trabajo nuevo en la cola
    else entradas vigentes
      PR->>M: par "actual" con hora de cálculo, límites declarados y topes ("truncado" si se superan)
      M->>CA: evento s+k
    end
  end

  CA-->>T: evento con ⚡ nuevo del par (W1, W3) en src/app.rs, hunks 40-52 y 41-60
  T->>T: ingesta y saneado de rutas y ramas (texto no confiable)
  T->>T: ⚡ nuevo, así que ConflictAlert y toast (BR-CKP-WF-007)
  loop cada segundo (Tick)
    T->>T: antigüedad "hace N s" desde la hora de cálculo y el reloj de vista
  end

  Note over AG,T: Del fin del commit a la predicción publicada, ≤ 5 s p95 con 10 worktrees y 100K commits (S-CKP-1, ⚠️ ASSUMPTION hasta SPIKE-CKP-001). Fuera del gate de 500 ms de NFR-04.
  Note over PR,GL: Ninguna escritura en el repo del usuario y ningún proceso hijo con la opción (a) de ADR-CKP-001. Si SPIKE-CKP-001 no demuestra las cotas de M-06 dentro del daemon, el merge pasa a un proceso trabajador con rlimits (el propio binario, nunca git). Verificado por la suite "predicción" de INF-GRP-001.
  Note over PR,M: Base no confirmada o pendiente, o una operación en curso en W1: los pares afectados se publican "pendiente". Base inexistente: "no calculable" con su motivo, sin elegir otra rama.
```

Linux y Windows (prioridad baja del SO, latencias): **Pendiente: etapa de validación multiplataforma**.
