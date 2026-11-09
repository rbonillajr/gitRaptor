---
id: TS-CKP-008
title: "Operaciones del catálogo para preparar y quitar del índice por archivo, fragmento y línea"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: high
created: 2026-11-08
updated: 2026-11-08
blocked_by: [TS-CKP-002, US-CKP-012, TS-CKP-006]
related:
  adrs: [ADR-CKP-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007, ADR-GRP-009, ADR-GRP-016, ADR-GRP-017, ADR-GRD-008]
  stories: [US-DSK-011, US-DSK-014, US-DSK-010, US-DSK-006, TS-CKP-002, TS-CKP-006, TS-CKP-009, US-CKP-012, TS-TMC-004, INF-GRP-001]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, desktop, dep-dsk-4, catalogo-operaciones, indice, staging, fragmento, nfr-01, nfr-02, ui-2]
---

## TS-CKP-008: Operaciones del catálogo para preparar y quitar del índice por archivo, fragmento y línea

**Valor**: el humano prepara para el commit un archivo, un fragmento o unas líneas, y quita lo preparado, desde la app; cada cambio del índice pasa por el ejecutor del daemon con punto previo y se deshace con la Time Machine.

> Enabler de DEP-DSK-4 ([contexto de desktop](../../desktop/context.md) § 8). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: trabajo del catálogo del motor (slug CKP), con dos historias dueñas: US-DSK-011 (fragmento y línea) y US-DSK-014 (marcar un archivo como resuelto es prepararlo). Añade operaciones: sube la versión **menor** del catálogo (ADR-CKP-002 § 1).

### Descripción

**Como** Arquitecto
**Quiero** dos operaciones nuevas del catálogo, `stage` y `unstage`, servidas por `operation.prepare` y `operation.run`, que preparan o quitan del índice del worktree del solicitante archivos enteros, fragmentos o líneas de un fragmento
**Para** que el índice, que es estado del repo, solo cambie por la vía única de escritura (Q-DSK-2, BR-CKP-CONS-002) con plan, revalidación bajo el cerrojo y snapshot previo garantizado (ADR-CKP-002 § 2, ADR-TMC-004, NFR-01)

### Alcance Técnico

- **Registrar** `stage` y `unstage` como operaciones propias del catálogo en su módulo del ejecutor (ADR-GRP-016, [extender sin archivos compartidos](../../../../architecture/extender-sin-archivos-compartidos.md)) y en la consulta `operation.describe`.
  - Clase: no destructiva; el snapshot previo guarda el índice (como `commit`, ADR-TMC-007 § 2).
  - Gobernanza: **no gobernada** (no está en BR-VAL-002), como `abort-in-progress`; las políticas de rutas prohibidas, tamaño y formato se aplican al `commit` (US-GRD-009). Decisión del orquestador (2026-11-08), validada por Arquitecto.
  - Marcas: Cockpit **sí**; MCP **no** (sin herramienta MCP, BR-14).
- **Validar** la selección en el daemon: rutas relativas literales del worktree del solicitante, con tope; o fragmentos y rangos de líneas identificados como los publica el diff (TS-CKP-006 y `diff.worktree` de US-CKP-012).
- **Fijar** en el plan el contenido esperado de cada archivo en el worktree y de su entrada en el índice; si alguno cambió al ejecutar, el resultado es `rejected` con "el archivo cambió, revísalo de nuevo" y no se prepara nada (US-DSK-011, escenario 3).
- **Aplicar** solo al índice: el working tree nunca cambia; lo no seleccionado queda igual en el índice y en el worktree.
- **Preparar** un archivo en conflicto como "resuelto" solo si ya no tiene marcadores de conflicto; si los tiene, `rejected` con el motivo (US-DSK-014).
- **Proteger** con el snapshot `previo_garantizado` de la operación protegida antes de tocar el índice; "Deshacer" de la Time Machine devuelve el índice al previo (US-DSK-006).
- **Resolver** la capa con ADR-GRP-017 § 4: en la app, capa `cockpit` con **presencia de sesión** (se revierte en local); con identidad por ruta, **factor por acción** ligado al plan; desde la TUI, la capa `cockpit` de siempre. Capa `mcp` o agente solicitante: "operación no disponible para este solicitante".
- **Avisar** en el plan si hay otra sesión presente en el worktree (ADR-CKP-002 § 1, Q-MCP-21), que el humano confirma.
- **Respetar** las precondiciones comunes: locks de Git, que nunca se borran; operación en curso, salvo marcar resuelto dentro de un merge o rebase detenido.
- **Fuera de alcance**: el commit de lo preparado (TS-CKP-009); continuar el merge tras resolver (Requisitos Técnicos de US-DSK-014); la vista de fragmentos y la selección de líneas en la app (US-DSK-011); preparar en el worktree de otro actor, que no se ofrece; stash.

### Plan de Verificación

#### Pruebas Automatizadas

- **Fragmento**: en un repo temporal, `pagos.rs` con dos fragmentos; preparar uno deja en el índice solo ese y el working tree igual (US-DSK-011, escenario 1).
- **Línea**: quitar una línea de un fragmento preparado de 5 deja 4 preparadas y la quinta en el worktree (US-DSK-011, escenario 2).
- **Archivo cambiado (carrera)**: un proceso externo modifica `pagos.rs` entre preparar y ejecutar: `rejected`, índice sin cambios.
- **Resuelto**: un archivo con marcadores se rechaza; sin marcadores se prepara y el merge detenido sigue detenido.
- **NFR-01**: con el almacén lleno, `aborted` e índice igual (huella de INF-GRP-001); tras preparar, "Deshacer" devuelve el índice previo byte a byte.
- **Capa (ADR-GRP-017, V4)**: con el doble del factor, la app sin presencia de sesión recibe rechazo; con sesión e identidad firmada, se acepta; con identidad por ruta, pide factor por acción; un agente en la ascendencia y una conexión de `raptor-mcp` reciben "no disponible".
- **Frontera**: la captura de red durante `stage` y `unstage` da 0 conexiones (Validación 16 de ADR-CKP-002, parte local); el repo canario con filtros `clean` no ejecuta el marcador.
- **Catálogo**: `operation.describe` muestra las dos operaciones con su clase y marcas, y la versión menor subida.

#### Verificación Manual / Sandbox

- Con la app en un repo de demo, preparar un fragmento y una línea, comprobar con `git diff --cached` y deshacer desde la Time Machine.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
