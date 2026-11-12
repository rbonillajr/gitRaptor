---
id: TD-GRP-012
title: "El status() del índice de diff.index y diff.worktree respeta el plazo y la cancelación de la consulta"
type: td
status: draft
feature: motor-local
domain: GRP
priority: low
complexity: small
created: 2026-11-12
updated: 2026-11-12
source: inline
blocked_by: []
related:
  adrs: [ADR-GRP-009, ADR-GRP-016]
  stories: [US-CKP-012, TS-CKP-006, US-DSK-009]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, consultas-bajo-demanda, plazo, cancelacion, dos]
---

## TD-GRP-012: El status() del índice de diff.index y diff.worktree respeta el plazo y la cancelación de la consulta

**Valor**: una consulta de diff sobre un worktree enorme no ocupa el único hilo `raptor-query` más allá de su plazo ni ignora `query.cancel`, así que la TUI y la app no esperan en la cola detrás de ella.

> **Reserva (2026-11-12).** Solo la ficha: sin Dev Spec ni código. Origen: L-01 del `security-review` de TS-CKP-006 (PR #362, APPROVED WITH COMMENTS) y la línea «Plazo y cancelación de `status()`» de Fuera de alcance de DS-TS-CKP-006 (L-02 de su revisión).

### Descripción

**Como** Arquitecto
**Quiero** que la llamada a `RepoReader::status()` y las lecturas del índice de `index_diff` (`crates/git/src/diff/index.rs`) y de `worktree_diff` (`crates/git/src/diff.rs`) comprueben el plazo (`limits.deadline`) y el token de cancelación
**Para** que un worktree con cientos de miles de archivos sin seguimiento (por ejemplo un `node_modules` sin ignorar) no retenga el hilo de consultas más allá de `DIFF_DEADLINE_MS`

**Hecho**: hoy el bucle de `index_diff` (lectura del índice, `status()`, relectura; hasta 2 `status()` y 4 lecturas si el índice cambia) y el `status()` de `diff.worktree` no consultan `limits.deadline` ni `stop()`; solo se consultan después, dentro de `build_section`. `query.cancel` no los interrumpe.

### Alcance Técnico

- **Medir** primero con un worktree grande real (cientos de miles de archivos sin seguimiento) cuánto tarda `status()` y si ocurre con los clientes reales.
- **Interrumpir** `status()` por plazo y por token, o comprobar plazo y token antes de cada vuelta del bucle y antes de `status()` y responder `TemporarilyUnavailable` (ya se mapea a `diff-unavailable {worktree-unavailable}`), sin cambiar el contrato ni los códigos.
- **Dónde**: `crates/git/src/reader.rs` (`status`) es el lector que comparten todas las lecturas del motor: el cambio lo toca esta ficha y no TS-CKP-006 (D10 de DS-TS-CKP-006).
- **Fuera de alcance**: repartir la cola por solicitante (TD-GRP-009); subir los topes de D4 de DS-US-CKP-012.

### Plan de Verificación

- Con un worktree de prueba generado en un directorio temporal (nunca este repo) y un plazo forzado por `test_diff_deadline`, `diff.index` y `diff.worktree` responden `diff-unavailable` dentro del plazo y no ocupan el hilo.
- `query.cancel` sobre un `diff.index` en `status()` responde `query-cancelled` y libera la plaza.
- Los tests de `diff.worktree` y `ts_ckp_006_*` siguen en verde.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
