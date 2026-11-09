---
id: TS-CKP-009
title: "Operación commit del catálogo abierta a la capa cockpit"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
blocked_by: [TS-CKP-002, TS-CKP-003, US-MCP-009]
related:
  adrs: [ADR-CKP-002, ADR-TMC-004, ADR-TMC-007, ADR-GRD-003, ADR-GRP-016, ADR-GRP-017, ADR-GRD-008]
  stories: [US-DSK-010, US-DSK-011, US-DSK-006, US-MCP-009, US-MCP-010, US-GRD-009, US-GRD-018, TS-CKP-002, TS-CKP-003, TS-CKP-008, INF-GRP-001]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, desktop, dep-dsk-4, catalogo-operaciones, commit, capa-cockpit, nfr-01, d6, ui-2]
---

## TS-CKP-009: Operación commit del catálogo abierta a la capa cockpit

**Valor**: el humano crea un commit desde la app con la misma operación `commit` que ya usa el MCP, con punto previo, políticas del repo, autoría a su nombre y "Deshacer", sin una segunda vía de escritura.

> Enabler de DEP-DSK-4 ([contexto de desktop](../../desktop/context.md) § 8). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: trabajo del catálogo del motor (slug CKP) con dos historias dueñas, US-DSK-010 (rutas elegidas) y US-DSK-011 (todo lo preparado). Cambia una marca de superficie: sube la versión **mayor** del catálogo y exige la enmienda de ADR-CKP-002 § 1, donde `commit` figura como "No en el MVP" para el Cockpit.

### Descripción

**Como** Arquitecto
**Quiero** que la operación `commit` del catálogo, servida por `operation.prepare` y `operation.run`, acepte la capa `cockpit` además de la `mcp`, con las reglas propias de un humano
**Para** que el commit desde la app pase por la vía única (Q-DSK-2), con snapshot previo garantizado (ADR-TMC-004, NFR-01), la decisión de Guardrails registrada una vez (TS-CKP-003) y la capa fijada por el daemon según ADR-GRP-017 § 4

### Alcance Técnico

- **Cambiar** la marca Cockpit de `commit` a "sí" en el catálogo y en `operation.describe`, y subir la versión mayor (ADR-CKP-002 § 1); la marca MCP y `safe_commit` no cambian.
- **Aceptar** con capa `cockpit` las dos formas de `commit`: rutas literales con tope y "todo lo preparado".
  - "Todo lo preparado" con otra sesión presente en el worktree: aviso en el plan que el humano confirma (con capa `mcp` sigue rechazado, Q-MCP-21).
  - El mensaje, obligatorio, con tope y sin NUL, nunca por argv ni en el registro; su hash entra en la huella.
- **Fijar** la autoría: el autor es la persona de la configuración de Git del worktree; sin coautor de agente cuando el solicitante es el humano (D6, US-GRD-018).
- **Proteger** con el snapshot `previo_garantizado` antes de crear el commit; "Deshacer" de la Time Machine deja el commit fuera de la rama y los cambios otra vez sin commitear (US-DSK-010, escenario 3).
- **Decidir** con Guardrails como operación gobernada "Commit" (TS-CKP-003): las políticas de formato del mensaje y tamaño de diff (US-GRD-009) y de autoría (US-GRD-018) se evalúan en la preparación y se repiten al ejecutar; una denegación devuelve la regla y el motivo.
- **Resolver** la capa con ADR-GRP-017 § 4: en la app, `cockpit` con **presencia de sesión** (se revierte en local); con identidad por ruta, **factor por acción** ligado al plan; desde la TUI, la capa `cockpit` de siempre. Un agente solicitante sigue en la capa `mcp`.
- **Entregar** la salida de los hooks del repo (`pre-commit`, `commit-msg`), saneada y truncada, solo a la conexión `cockpit` que pidió el commit (ADR-CKP-002 § 6).
- **Mantener** sin variantes `--amend`, `--no-verify` ni `--allow-empty` (ADR-CKP-002 § 1).
- **Fuera de alcance**: preparar y quitar del índice (TS-CKP-008); el compositor del mensaje en la app (US-DSK-010); el commit que cierra un merge detenido (Requisitos Técnicos de US-DSK-014); la presencia de sesión y el factor, que da ADR-GRP-017 por su propia historia; ofrecer el commit en la TUI, que necesitaría su historia.

### Plan de Verificación

#### Pruebas Automatizadas

- **Rutas elegidas**: en un repo temporal con `readme.md` y `pagos.rs` cambiados, `commit` de capa `cockpit` con solo `readme.md` crea el commit y deja `pagos.rs` sin commitear (US-DSK-010, escenario 1).
- **Todo lo preparado**: con un fragmento preparado (TS-CKP-008), el commit contiene solo ese fragmento (US-DSK-011, escenario 1).
- **Política**: con Conventional Commits exigido, el mensaje "cambios" se deniega con la regla; nada cambia en el repo (US-DSK-010, escenario 2).
- **Autoría**: el commit lleva como autor la persona configurada y ningún trailer de agente.
- **NFR-01**: con el almacén lleno, `aborted` y el repo igual (huella de INF-GRP-001); "Deshacer" devuelve la rama y el índice al previo.
- **Capa (ADR-GRP-017, V4)**: con el doble del factor, la app sin presencia de sesión recibe rechazo; con sesión e identidad firmada se acepta; con identidad por ruta pide factor por acción; con capa `mcp`, "todo lo preparado" con otra sesión presente sigue rechazado.
- **Regresión MCP**: la suite de `safe_commit` (US-MCP-009 y US-MCP-010) sigue en verde con la versión mayor nueva.
- **Red**: captura de red durante el commit: 0 conexiones (Validación 16 de ADR-CKP-002, parte local).

#### Verificación Manual / Sandbox

- Con la app en un repo de demo con hooks reales (formateador y `commit-msg`), crear un commit, ver la salida del hook y deshacerlo desde la Time Machine.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
