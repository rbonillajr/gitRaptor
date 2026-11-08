---
id: TD-TMC-002
title: "Captura continua: blobs fuera del escritor del almacén, para bajar su hashing a background (RES-06)"
type: td
status: draft
feature: time-machine
domain: GRP
priority: medium
complexity: high
created: 2026-11-08
updated: 2026-11-08
related:
  adrs: [ADR-GRP-015, ADR-TMC-004, ADR-TMC-006]
  stories: [TS-GRP-005, TS-TMC-001]
  specs: [DS-TS-GRP-005]
ado:
  id: null
  url: null
tags: [time-machine, deuda-tecnica, captura, escritor, hashing, res-06, nfr-01]
---

## TD-TMC-002: Captura continua: blobs fuera del escritor del almacén, para bajar su hashing a background (RES-06)

**Valor**: el hashing de la captura continua puede correr en `background` sin invertir prioridades, y TS-GRP-005 cierra RES-06 completo.

### Descripción

**Como** responsable de la Time Machine
**Quiero** que la fase de blobs de la captura continua salga del escritor del almacén
**Para** que el escritor cubra solo árboles, ref y oplog (la premisa de ADR-GRP-015 § 1) y el hashing pueda bajar a `background`

> Dev Spec: dev-specs/TD-TMC-002-captura-blobs-fuera-del-escritor.md | Pendiente (se diseña aparte)

**Origen.** [ADR-GRP-015, Enmienda (2026-11-08)](../../../../architecture/decisions/ADR-GRP-015-consumo-recursos.md#enmienda-2026-11-08-hashing-de-la-captura-bajo-el-escritor), hallazgo en PR-B de DS-TS-GRP-005 (D11).

**Hoy.** `run_capture` (`store/capture.rs`) retiene el escritor (`MutexGuard` sobre `capture::State`) durante detección, `write_blobs`, árboles, ref y oplog. Sin previo, `write_blobs` hace el hash en línea en `raptor-tm-capture`. Bajarlo a `background` bajaría el hilo que retiene el escritor (inversión de prioridad, D11 y RES-07). Por eso en PR-B la captura sigue en `default` y el previo en `user-initiated`.

### Alcance Técnico

- **Sacar** la fase de blobs de la sección con el escritor retenido: el escritor cubre solo árboles, ref y oplog.
- **Bajar** el hashing de la captura continua a `background` (RES-06) cuando el hilo que hashea ya no retiene el escritor.
- **Respetar** ADR-TMC-004 § 2: el `state` de continuidad se lee en la detección; el previo cede cada ~1 MiB; Validación 7 (≤ 10 ms p95).
- **Respetar** NFR-01: sin ventana en la que una ref o fila de oplog apunte a blobs no escritos, ni carrera con una captura concurrente.
- **Fuera de alcance**: cambiar clases de otros hilos, el camino del previo y la cadencia de captura.

### Plan de Verificación

#### Pruebas Automatizadas

- Con el hashing en `background` y carga de CPU saturada, un previo no espera a la captura (RES-07) y Validación 7 sigue en ≤ 10 ms p95.
- Una captura interrumpida entre blobs y ref no deja ref ni oplog que apunten a blobs ausentes; el arnés de caos de INF-TMC-001 pasa.
- Capturas concurrentes sobre el mismo almacén dan los mismos snapshots que antes (la continuidad no cambia).
- RES-06 medido para el hashing de la captura en el gate de INF-GRP-002.

**Bloquea** el cierre de RES-06 para el hashing de la captura y el estado `implemented` completo de TS-GRP-005. Debe tener su propia Dev Spec.
