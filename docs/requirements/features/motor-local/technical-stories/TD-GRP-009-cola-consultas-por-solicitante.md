---
id: TD-GRP-009
title: "La cola global de consultas bajo demanda se reparte por solicitante"
type: td
status: draft
feature: motor-local
domain: GRP
priority: low
complexity: small
created: 2026-11-10
updated: 2026-11-10
source: inline
blocked_by: [US-CKP-012]
related:
  adrs: [ADR-GRP-009, ADR-GRP-016]
  stories: [US-CKP-012, TS-CKP-006, US-DSK-009]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, consultas-bajo-demanda, cola, dos]
---

## TD-GRP-009: La cola global de consultas bajo demanda se reparte por solicitante

**Valor**: un cliente que pide muchas consultas no deja sin plaza a los demás (la TUI y la app).

> **Reserva (2026-11-10).** Solo la ficha: sin Dev Spec ni código. Origen: I-01 de la revisión de seguridad de DS-US-DSK-009 y R5 de TS-CKP-006; decisión del coordinador del 2026-11-10 (D1 de DS-TS-CKP-006).

### Descripción

**Como** Arquitecto
**Quiero** que la cola global de 4 plazas del `QueryPool` (`QUERY_QUEUE`, `crates/core/src/channel/query.rs`) se reparta por solicitante
**Para** que un cliente, con su plaza única por conexión (`QUERIES_PER_CONNECTION = 1`), no agote las 4 plazas de los demás

**Hecho**: hoy `query-busy` sale por la plaza única de la conexión o por la cola global compartida. Con varias conexiones de un mismo cliente (o de uno hostil) la cola se llena y la TUI y la app reciben `query-busy`.

### Alcance Técnico

- **Medir** primero si ocurre con los clientes reales (TUI, app, grafo, diff de commit).
- **Repartir** las plazas por solicitante o por clase de cliente sin cambiar el contrato: `query-busy` conserva su forma (`QueryBusyData`).
- **Fuera de alcance**: subir los topes de D4 de DS-US-CKP-012 y el plazo y la cancelación de `status()` (ficha aparte).

### Plan de Verificación

- Con un cliente que llena su cuota, otro cliente sigue obteniendo plaza.
- La plaza única por conexión y la cancelación se mantienen.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
