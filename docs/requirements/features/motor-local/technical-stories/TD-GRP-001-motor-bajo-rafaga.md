---
id: TD-GRP-001
title: "Frescura y memoria del motor bajo una ráfaga de archivos"
type: td
status: ready
feature: motor-local
domain: GRP
priority: critical
complexity: medium
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-GRP-010, ADR-GRP-011]
  stories: [INF-GRP-002, US-GRP-001, US-GRP-002]
  specs: [DS-INF-GRP-002]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, rendimiento, rafaga, nfr-04, nfr-05, huella]
---

## TD-GRP-001: Frescura y memoria del motor bajo una ráfaga de archivos

**Valor**: una ráfaga en un worktree (instalar dependencias, compilar, reescribir cientos de archivos) no frena a los demás ni deja memoria retenida en un proceso de fondo.

### Descripción

**Como** desarrollador con 10 agentes trabajando en paralelo
**Quiero** que la ráfaga de un worktree no retrase lo que veo de los otros nueve y que el motor devuelva la memoria al terminar
**Para** que el Cockpit siga siendo fiable mientras un agente instala dependencias y que GitRaptor no pese en mi máquina

> Dev Spec: `dev-specs/TD-GRP-001-dev-spec.md` | Pendiente
>
> **Prioridad**: crítica, **Must antes de cerrar el MVP**. Un MVP no sale incumpliendo un NFR comprometido (PO, 2026-10-05).

**Hallazgo (INF-GRP-002, 2026-10-05).** El banco de INF-GRP-002, en macOS (Mac de Rene), con el repo de 100K commits y 10 worktrees, midió lo siguiente. Las cifras completas están en la [Dev Spec de INF-GRP-002](../dev-specs/INF-GRP-002-dev-spec.md) § 9.

- **Latencia, solo en macOS**: durante una ráfaga en un worktree, el p95 del motor en los otros nueve supera los 300 ms de NFR-04: en el Mac de referencia, 336 ms con 1.000 archivos (el escenario de ADR-GRP-011 § 4) y 496 ms con 10.000. El recomputo del worktree medido sube de ~20 ms a cientos. **En Linux (CI) se cumple**: 193 ms p95 en las dos ráfagas.
- **El cuello está en el motor**: sin daemon, `git status` en otro worktree durante la misma ráfaga no se degrada (33 → 35 ms), ni siquiera con un `F_FULLFSYNC` cada 75 ms (40 ms). La CPU del daemon se queda en torno a un núcleo, así que la causa probable es un recurso compartido que se serializa, no la falta de CPU (Arquitecto, 2026-10-05). Candidatos: el despachador de eventos, el filtro de ignorados, el canal hacia el bucle del daemon o la publicación del estado de todos los worktrees en cada lote.
- **Memoria, en los dos SO**: el pico de RSS en ráfaga va de 420 a 830 MiB según la corrida, y el daemon retiene después entre 220 y 345 MiB (objetivo de reposo: 150 MiB). La causa candidata es el buffer de reproducción del bus: 1.024 eventos `worktree.state`, cada uno con el estado de los 10 worktrees (hasta 200 cambios o 32 KB por worktree). El banco reporta su tamaño serializado (`replay_buffer`).
- **Riesgo**: el temporizador del runner de macOS (VM) se despierta de 92 a 147 ms tarde. Un daemon arrancado por launchd con QoS de fondo podría sufrir el mismo coalescing. Hay que verificarlo en dogfooding con el autoarranque.

Esto refuta la invariante de ADR-GRP-010 § 3 ("la ráfaga de un worktree no retrasa a los demás") y activa el disparador que dejó la Enmienda US-GRP-002 de ADR-GRP-010: "la caché de stat y las dos fases se añaden si INF-GRP-002 muestra que no cabe".

### Alcance Técnico

1. **Diagnosticar** el recurso compartido que serializa el trabajo entre worktrees durante la ráfaga (perfilado del daemon con el escenario `burst-1k` del banco).
2. **Aislar** el trabajo por worktree para que el recomputo de uno no espere al de otro.
3. Si no basta, **añadir** la caché de stat y la publicación en dos fases de ADR-GRP-010 § 4. Después, si hace falta, **acotar** el coste del recomputo del worktree en ráfaga.
4. **Acotar** el pico de memoria y **devolverla** tras la ráfaga.
5. **Retirar** los techos provisionales del banco (`Platform::burst_ceiling_ms` en `crates/testkit/src/freshness.rs`) y pasar los dos escenarios de ráfaga al gate de 300 ms.

- **Fuera de alcance**: la medición del Cockpit (`t_render`), el modo degradado y la reconciliación, que no cuentan para NFR-04.

### Plan de Verificación

#### Pruebas Automatizadas

Criterios de cierre, medidos con el banco de INF-GRP-002 (`cargo bench -p gitraptor-cli --bench engine`):

- `burst-1k` y `burst-10k`: p95 del motor ≤ 300 ms en el Mac de referencia y en Linux (gate bloqueante, sin techo provisional).
- El pico de RSS tiene un techo acordado con el Arquitecto, que hoy no existe porque su variación no está explicada.
- Verificado en dogfooding el coalescing de timers con el daemon arrancado por launchd.
- ⚠️ **ASSUMPTION** (objetivo de producto del PO; pendiente de que lo valide Rene): pico de RSS en ráfaga < 250 MiB.
- ⚠️ **ASSUMPTION** (PO): tras la ráfaga, RSS < 150 MiB en ≤ 60 s y CPU de vuelta al reposo (< 1 %).
- Se actualizan ADR-GRP-010 § 3, ADR-GRP-011 § 4 y la fila HUELLA y NFR-05 de `non-functional.md`.

#### Verificación Manual / Sandbox

**Caducidad del techo provisional**:

Los techos provisionales de INF-GRP-002 caducan cuando se cierra esta TD. Mientras tanto, NFR-04 y NFR-05 bajo ráfaga constan como **no cumplidas** en `non-functional.md`.
