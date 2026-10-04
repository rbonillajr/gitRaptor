---
id: SPIKE-GRP-002
title: "Viabilidad del observador de cambios a escala en los tres SO"
type: spike
status: Research Pending
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-010, ADR-GRP-011, ADR-GRP-009, ADR-GRP-006]
  stories: [US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, spike, watcher, latencia, escala, debounce, windows, inotify]
---

## SPIKE-GRP-002: Viabilidad del observador de cambios a escala en los tres SO

**Valor**: confirmar con mediciones reales el diseño de ADR-GRP-010 y las cifras de ADR-GRP-011 antes de que US-GRP-002 entre en desarrollo.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-GRP-002-viabilidad-observador.md`. Prototipo aislado, sin código del motor. **Depende de**: — (arranca el día uno). **Valida**: ADR-GRP-010 y ADR-GRP-011.

### Pregunta

¿Un watcher nativo con debounce de ventana fija de 75 ms, recomputo incremental, sondeo de respaldo y reconciliación cumple el presupuesto del motor (≤ 300 ms p95), la escala de NFR-05 y cero huecos silenciosos en Windows, macOS y Linux?

### Hipótesis

- Las cifras por etapa de ADR-GRP-011 § 2 se cumplen en p95: detección ≤ 50 ms, debounce 75 ms, recomputo y persistencia ≤ 150 ms, publicación ≤ 25 ms, total ≤ 300 ms.
- La ventana fija de 75 ms acota la espera durante una ráfaga continua, en lugar de posponer la publicación hasta que acaba.
- Persistir antes de publicar, con un lote por ventana, cabe en el presupuesto aunque el `fsync` de macOS y Windows cueste decenas de milisegundos.
- Sin refrescar el índice (ADR-GRP-009), la caché de stat en memoria mantiene el recomputo dentro de presupuesto.

### Experimento

- **Latencia por etapa**: con 10 worktrees y un repo de 100K commits o más, medir el p95 de cada etapa y del total al modificar un archivo, `git add`, commit, checkout y crear y borrar un worktree.
- **Debounce de 75 ms**: comparar la ventana fija de 75 ms con ventanas alternativas en latencia del primer cambio visible, número de recomputos y CPU durante una ráfaga.
- **Interpretación p95**: comparar p95, p99 y máximo para confirmar o corregir la interpretación de NFR-04 de ADR-GRP-011 § 1.
- **Escala**: ráfaga de 10K archivos en un worktree; watches usados, memoria, CPU en reposo y p95 de los otros nueve durante la ráfaga.
- **Sondeo**: coste del sondeo de respaldo (30 s) y del modo degradado (2 s) con 10 worktrees.
- **Huecos**: suspensión y reanudación, desbordamiento forzado de la cola con búfer reducido y watcher reiniciado; la reconciliación debe detectar el 100% de los cambios y marcarlos "sin atribuir".
- **Windows**: `git worktree remove`, borrado y renombrado de la raíz y de archivos con el watcher activo.
- **Linux**: comportamiento al agotar el límite de watches de inotify (modo degradado sin caída del resto).
- **macOS**: latencia mínima de FSEvents y fusión de eventos frente al presupuesto de detección.
- **Repo intacto**: el prototipo no escribe nada en los repos observados (huella antes y después).

### Criterios de Éxito

- Todas las cifras de ADR-GRP-011 § 2 dentro de presupuesto en p95 en los tres SO, con el debounce de 75 ms confirmado o sustituido por un valor medido.
- 100% de los cambios recuperados por reconciliación en los escenarios de hueco.
- Ningún fallo de borrado o movimiento de worktrees atribuible al watcher en Windows.
- **Vía de fracaso**: si una cifra no se cumple, se revisa ADR-GRP-010 (p. ej. sondeo por defecto en el SO afectado o vigilancia desde el directorio padre en Windows) y, si cambia el reparto, ADR-GRP-011. INF-GRP-002 adopta las cifras resultantes como gate.

### Time-box

⚠️ **ASSUMPTION**: 2 semanas, repartidas entre los tres SO.
