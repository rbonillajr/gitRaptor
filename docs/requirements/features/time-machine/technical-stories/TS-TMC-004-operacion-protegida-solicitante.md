---
id: TS-TMC-004
title: "Operación protegida y resolución del solicitante en el canal"
type: ts
status: implemented
feature: time-machine
domain: GRP
priority: critical
complexity: high
created: 2026-10-03
updated: 2026-10-08
related:
  adrs: [ADR-TMC-004, ADR-TMC-005, ADR-TMC-002, ADR-GRP-005, ADR-GRP-012, ADR-GRP-013]
  stories: [US-TMC-001, US-TMC-002, US-TMC-003, US-TMC-005, US-TMC-009, US-TMC-010, US-TMC-011, US-TMC-013, TS-GRP-004, TS-TMC-001, TS-TMC-002]
  specs: [DS-TS-TMC-004]
ado:
  id: null
  url: null
tags: [time-machine, canal, contrato, solicitante, snapshot-previo, mcp, br-tmc-cons-001, d-tmc-23]
---

## TS-TMC-004: Operación protegida y resolución del solicitante en el canal

**Valor**: ninguna superficie de GitRaptor puede modificar el repo sin snapshot previo, y toda petición queda atribuida a "agente X" o "sin atribuir" por el daemon, nunca por el cliente.

### Descripción

**Como** Arquitecto
**Quiero** la operación protegida y los comandos de la Time Machine en el contrato del canal, con el solicitante resuelto en el daemon
**Para** que CLI, TUI, MCP, Cockpit y los hooks de Guardrails usen un único camino de escritura que cumple BR-TMC-CONS-001 y D-TMC-23

> Dev Spec: [`dev-specs/TS-TMC-004-operacion-protegida-solicitante.md`](../dev-specs/TS-TMC-004-operacion-protegida-solicitante.md) | En revisión
>
> **Depende de**: TS-GRP-004 (canal, contrato y controles de comandos reservados), TS-TMC-001 (snapshot) y TS-TMC-002 (oplog). **ADRs**: ADR-TMC-004 § 1 (operación protegida), ADR-TMC-005 § 1 (solicitante), ADR-GRP-005 § 6 (identificación del proceso llamante), ADR-GRP-012 (procesos de agente).

### Alcance Técnico

- **Exponer** en el contrato del canal la operación protegida: intención, snapshot previo, ejecución y registro en un único camino; sin snapshot no hay operación.
- **Entregar** las operaciones de usuario al ejecutor de operaciones del daemon (F-001-02 y F-001-05) solo tras un snapshot válido; la Time Machine no las ejecuta.
- **Garantizar** en el contrato que ningún otro mensaje modifica el repo.
- **Exponer** los comandos de snapshot, undo, redo, restauración y consulta del timeline para CLI, TUI y MCP, con validación estricta de parámetros.
- **Implementar** la resolución del solicitante en el daemon por la ascendencia del proceso llamante, con hora de inicio de cada antecesor y contaminación por multiplexor: agente X con su origen o "sin atribuir", nunca humano (SEC-TMC-03).
- **Implementar** el reto de confirmación de un solo uso ligado a la conexión, al proceso y al hash del plan, con caducidad, que usa US-TMC-013 (SEC-TMC-03).
- **Tomar** por MCP el repo y el worktree del cwd del llamante, exigir que el repo esté en la allowlist antes de un undo o una restauración y responder como inexistente un id de otro repo (SEC-TMC-07, SEC-TMC-15).
- **Registrar** el canal de cada petición (CLI, TUI, MCP o hook) y entregarlo congelado al oplog.
- **Implementar** el tiempo máximo y el resultado explícito del snapshot previo, con el motivo cuando falla.
- **Marcar** como no confiable todo texto del repo o de un agente en las respuestas, y limitar las respuestas al MCP a campos estructurados sin contenido de archivos.
- **Fuera de alcance**: confirmación interactiva y regla base de permisos (US-TMC-013); evaluación de políticas (US-TMC-021); comando para hooks (US-TMC-005); herramientas MCP como tales (F-001-05); instalar hooks (nunca, Q22).

### Plan de Verificación

#### Pruebas Automatizadas

- **Contrato**: un test recorre todos los mensajes del canal y comprueba que solo la operación protegida y los comandos de la Time Machine pueden escribir en el repo.
- **Garantía**: con el almacén sin espacio o el daemon detenido a la fuerza, la operación no se ejecuta y el cliente recibe el motivo.
- **Solicitante**: un cliente lanzado desde el árbol de un Claude Code simulado queda atribuido a él por CLI y por MCP; uno sin agente en su ascendencia queda "sin atribuir"; ningún resultado es "humano".
- **No confianza en el cliente**: un cliente que declara ser otro agente o un humano en sus parámetros no cambia el solicitante resuelto.
- **Validación**: duraciones, identificadores de agente y de snapshot fuera de formato se rechazan antes de tocar nada.
- **Ascendencia y reto**: `setsid` sin pty, `tmux new-window` + `send-keys`, PID reutilizado, reto reutilizado, de otra conexión o con el plan cambiado: todos rechazados.
- **MCP**: repo fuera de la allowlist, rechazado; id de snapshot de otro repo, "no existe".
- **Salida**: una rama con secuencias de escape sale limpia en la CLI y como texto marcado en el MCP.

#### Verificación Manual / Sandbox

- Lanzar `raptor undo` desde la shell de un Claude Code real en dogfooding y comprobar el solicitante registrado.

## Estado de la implementación (2026-10-08)

Implementado en: PR #40.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
