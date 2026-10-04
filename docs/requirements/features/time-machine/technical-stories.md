---
mode: bulk
status: expanded
generated: 2026-10-03
updated: 2026-10-04
generator: architect
domain: GRP
feature: time-machine
total_artifacts: 6
expanded: 6
approved: 0
related:
  context: [CTX-TMC-001]
  rules: [BR-TMC-001]
  adrs: [ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-004, ADR-TMC-005, ADR-TMC-006, ADR-TMC-007]
---

# Technical Stories — INDEX: Time Machine

> Índice. Cada historia vive en su archivo dentro de [`technical-stories/`](./technical-stories/), con status `draft` y pendientes de Dev Spec (columna Status = `Dev Spec Pending`): la Dev Spec de TS e INF se genera con `/aadd-devspec <id>`. El SPIKE no tiene Dev Spec: lleva un Research Brief.
>
> **Criterio de inclusión (Enabler Decision Gate)**: solo trabajo técnico **sin historia de usuario dueña única y sin resultado observable**. Lo observable ya está en US-TMC-001..021. Hay **6 enablers para 21 historias**. El trabajo de una sola historia vive en sus `Requisitos Técnicos` y en su Dev Spec: banco de overhead (US-TMC-020), cadencia de captura (US-TMC-004), comando para hooks (US-TMC-005), confirmación y regla base (US-TMC-013), solape (US-TMC-012), precondiciones de Git (US-TMC-015), aviso de ya empujado (US-TMC-014), purga (US-TMC-016), clave de retención (US-TMC-017), política de Guardrails (US-TMC-021), aviso de recuperación (US-TMC-019).
>
> **Reutilizado de motor-local, sin enabler nuevo**: perfil y almacén (TS-GRP-001), lectura de Git (TS-GRP-002), daemon (TS-GRP-003), canal y controles de comandos reservados (TS-GRP-004), arnés "repo intacto" (INF-GRP-001), banco de frescura (INF-GRP-002).

## Índice

| ID | Tipo | Título | Valor (1 línea) | ADR | US que habilita | Depende de | Complejidad | Status |
|----|------|--------|-----------------|-----|-----------------|-----------|-------------|--------|
| [SPIKE-TMC-001](./technical-stories/SPIKE-TMC-001-repo-mediano-overhead.md) | SPIKE | Repo mediano de referencia y viabilidad del snapshot en menos de 200 ms | Fija "repo mediano" (D-TMC-21) y valida almacén, reparto y cadencia | ADR-TMC-006, ADR-TMC-001, ADR-TMC-004 (valida) | US-TMC-001, 004, 020 | — (prototipo aislado; usa el generador de INF-GRP-002) | Medium | **Done (macOS)**, Linux y Windows sin verificar ([resultados](./research/SPIKE-TMC-001-resultados.md); D-TMC-21 aprobada: perfil `M`; enmiendas E1–E12 aplicadas en los ADR) |
| [TS-TMC-002](./technical-stories/TS-TMC-002-oplog-diario.md) | TS | Oplog de la Time Machine con diario de intención y recuperación | Registro inmutable y estados nombrados; recuperación al arrancar | ADR-TMC-003, ADR-TMC-007 | US-TMC-001, 002, 003, 006, 008, 009, 010, 011, 016, 019 | TS-GRP-001, TS-GRP-003 | Medium | Dev Spec Pending |
| [TS-TMC-001](./technical-stories/TS-TMC-001-almacen-captura-snapshots.md) | TS | Almacén de snapshots en el perfil y captura de estado | Snapshots fuera del alcance del push, del gc y de los agentes; el repo no cambia al capturar | ADR-TMC-001, ADR-TMC-004, ADR-TMC-006 | US-TMC-001, 004, 005, 009, 016, 018, 020 | TS-GRP-001, TS-GRP-002, TS-TMC-002 (informa SPIKE-TMC-001) | High | Dev Spec Pending |
| [TS-TMC-003](./technical-stories/TS-TMC-003-escritura-aplicador.md) | TS | Capa de escritura acotada y aplicador de estados | Escrituras internas sin hooks ni filtros, con locks, intercambio atómico y rutas seguras | ADR-TMC-002 | US-TMC-002, 003, 009, 010, 011, 014, 015, 019 | TS-GRP-002, TS-TMC-001, TS-TMC-002 | High | Dev Spec Pending |
| [TS-TMC-004](./technical-stories/TS-TMC-004-operacion-protegida-solicitante.md) | TS | Operación protegida y resolución del solicitante en el canal | Único camino de escritura con snapshot previo; solicitante resuelto en el daemon | ADR-TMC-004, ADR-TMC-005 | US-TMC-001, 002, 003, 005, 009, 010, 011, 013 | TS-GRP-004, TS-TMC-001, TS-TMC-002 | High | Dev Spec Pending |
| [INF-TMC-001](./technical-stories/INF-TMC-001-arnes-caos-recuperable.md) | INF | Arnés de caos y de garantías de los snapshots en los tres SO | Gate de CI de NFR-12, D-TMC-11 y casos hostiles de seguridad | ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-005, ADR-TMC-007 | US-TMC-001, 002, 009, 016, 018, 019 | INF-GRP-001, TS-TMC-001, TS-TMC-003 | Medium | Dev Spec Pending |

## Ruta de ejecución sugerida (DAG)

1. **Día uno, en paralelo**: SPIKE-TMC-001 (aislado) y, cuando estén TS-GRP-001 y TS-GRP-003, TS-TMC-002.
2. **Tras TS-TMC-002 y TS-GRP-002**: TS-TMC-001. Por el resultado del spike, su Dev Spec parte de los escalones 2 y 3 de ADR-TMC-006 § 5 (detección con el estado del motor y escritura del almacén con gitoxide) y de la siembra por clon o copia (ADR-TMC-001 § 3). Necesita que TS-GRP-002/003 expongan las rutas cambiadas por worktree desde una marca.
3. **Tras TS-TMC-001**: TS-TMC-003 (con TS-GRP-002) y TS-TMC-004 (con TS-GRP-004), en paralelo.
4. **Tras TS-TMC-003**: INF-TMC-001, que bloquea el merge de toda historia de la Time Machine que escribe.
5. **Historias**: US-TMC-001 necesita TS-TMC-001, 002 y 004; US-TMC-002 añade TS-TMC-003; US-TMC-020 ya no espera a SPIKE-TMC-001 (cerrado en macOS) y necesita TS-TMC-001. El mapa completo está en el [overview de la feature](../../../architecture/time-machine/overview.md#mapa-us--adr--enabler).
