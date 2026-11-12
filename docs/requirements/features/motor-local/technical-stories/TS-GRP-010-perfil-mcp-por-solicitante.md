---
id: TS-GRP-010
title: "Perfil mcp por solicitante: la CLI o la TUI que lanza un agente recibe el perfil mcp"
type: ts
status: implemented
feature: motor-local
domain: GRP
priority: high
complexity: high
milestone: M3
created: 2026-11-10
updated: 2026-11-12
blocked_by: []
related:
  adrs: [ADR-GRP-005, ADR-MCP-001, ADR-GRP-012, ADR-GRP-017, ADR-GRD-007]
  stories: [US-MCP-020, US-MCP-023, TS-GRP-009, US-CKP-011]
  rules: [BR-AUTH-004, BR-CONS-001]
  specs: [DS-TS-GRP-010]
ado:
  id: null
  url: null
tags: [canal, perfil-mcp, ascendencia, solicitante, s-01, sec-mcp-01, seguridad, superficie-agentes, hito-m3]
---

## TS-GRP-010: Perfil `mcp` por solicitante: la CLI o la TUI que lanza un agente recibe el perfil `mcp`

**Valor**: un agente que usa la CLI `raptor` (o habla directo con el socket) no ve ni puede más que por el MCP. Sin esto, la CLI es una puerta lateral que esquiva la allowlist, la vista restringida y el cubo de escrituras, y la guía para agentes no puede enseñarla.

> Es **S-01** (SEC-MCP-01) de [ADR-MCP-001 § 2](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md) y de la Enmienda 2026-10-05 (MCP) de [ADR-GRP-005](../../../../architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md). Estaba decidido y **no implementado**; tampoco tenía ficha. Decisión del orquestador (2026-11-10), por delegación de Rene Bonilla: pasa de la puerta de M4 a **Must de M3**, porque M3 enseña la CLI a los agentes (US-MCP-020, US-MCP-023).

> Dev Spec: [DS-TS-GRP-010](../dev-specs/TS-GRP-010-dev-spec.md). Precisa el orden de la decisión (hook y daemon mismo antes de S-01; completo solo para un sin atribuir que podría confirmar), el multiplexor ambiguo como `mcp`, el perfil fijo en el `hello` para toda conexión, qué conserva una conexión estrechada (canal declarado, veredicto de reservados, presupuestos de lectura) y la vista acotada mínima de `raptor status` y la TUI. Decisión del orquestador (2026-11-12), validada por Arquitecto.

Implementado en: PR #368 (2026-11-12). Pendientes del security-review con dueño a asignar: M-01 (rechazo en el release de binarios con `debug_assertions`), L-01 (ADR del cliente del hook), L-02 (canal que autoriza en `permission`).

### Descripción

**Como** Arquitecto
**Quiero** que el daemon asigne el perfil `mcp` (métodos, vistas y allowlist) a cualquier conexión cuyo **solicitante resuelto sea un agente**, sea cual sea el cliente que la abre (`raptor`, la TUI, un cliente JSON-RPC directo o la app de escritorio), y que lo deduzca él mismo de la ascendencia del proceso par, nunca del cwd ni de lo que el cliente declare
**Para** cerrar la entrada alternativa al canal (S-01, MCP02/MCP07), cumplir SEC-MCP-01 y desbloquear US-MCP-023 y la parte CLI de US-MCP-020

**Estado actual (a verificar en el código al empezar).** El daemon da el perfil `mcp` solo a un par cuyo ejecutable se llama `raptor-mcp` o a un cliente que se declara `mcp` (`crates/core/src/channel/conn.rs`, asignación del perfil); la app de escritorio ya lo recibe con un agente en su ascendencia desde [TS-GRP-009](./TS-GRP-009-clase-desktop-perfil-lectura.md) (`crates/core/src/channel/desktop.rs`). Un agente que lanza `raptor` obtiene hoy el perfil completo.

### Alcance Técnico

- **Reutilizar** la detección de ascendencia de TS-GRP-009 (`crates/core/src/channel/desktop.rs`) y el solicitante resuelto del canal, generalizándolos de la clase de la app a **toda** conexión: si el solicitante es un agente, el perfil es `mcp`.
- **La atribución sale del proceso**, nunca del cwd: el desarrollador que ejecuta `raptor status` desde su terminal dentro del worktree de un agente **conserva la vista completa**. US-CKP-011 («`raptor status` igual que la TUI») vale para el desarrollador.
- **Falla cerrada**: si la ascendencia no se puede resolver con seguridad (proceso ya muerto, lectura ilegible bajo carga), el resultado es el de TD-GRP-007 para una conexión sin solicitante claro, sin dar más que el perfil `mcp`.
- **Sin cambio de clase ni de perfil tras el handshake**, y sin que el cliente pueda estrechar o ampliar el resultado con lo que declare (solo puede pedir menos).
- **El cliente de los hooks** de Guardrails sigue como hasta ahora (ADR-GRD-001); los comandos reservados siguen rechazándose desde un agente por ADR-GRP-005 § 6.
- **Registrar** en el log del daemon la clase de cliente, el perfil asignado y el motivo (agente en la ascendencia), sin rutas de repos ni texto del cliente.
- **Cambio aditivo** del contrato (ADR-GRP-016): un cliente que no lo conoce ve las mismas formas; sin subir el protocolo salvo que la Dev Spec lo justifique.
- **Fuera de alcance**: restringir al agente por el cubo de rate limit compartido (S-03 completo, puerta de M4); Windows y Linux (**Pendiente: etapa de validación multiplataforma**); un agente lanzado con `open -a` o con una ascendencia rota a propósito (hueco ya aceptado en SPIKE-DSK-002).

### Plan de Verificación

#### Pruebas Automatizadas

- **Cliente directo bajo un agente simulado** (Validación 12 de ADR-MCP-001): un cliente JSON-RPC que se declara `cli` bajo un agente simulado recibe la vista MCP y `repo-not-enabled` en un repo no habilitado.
- **`raptor` y TUI bajo un agente simulado**: `raptor status --json` devuelve la vista con allowlist de campos, igual que el MCP; no ofrece los métodos que el perfil `mcp` no ofrece.
- **El desarrollador conserva su vista**: `raptor status` lanzado desde una terminal sin agente en la ascendencia, con el cwd dentro del worktree de un agente, devuelve la vista completa.
- **Sin escalada**: un cliente no cambia de perfil tras el handshake; declarar otra clase no da más.
- **Compatibilidad**: el cliente de los hooks, `raptor-mcp` y la app de escritorio reciben las mismas formas y perfiles que antes (suites del canal de `crates/core` en verde).

#### Verificación Manual / Sandbox

- En macOS, desde una sesión de Claude Code: `raptor status --json` en un repo no habilitado devuelve `repo-not-enabled` sin datos; la misma orden desde la terminal del desarrollador devuelve la vista completa.

### Dependencias

- **Depende de**: TS-GRP-009 (implementada; aporta la detección de ascendencia que se reutiliza).
- **Habilita (bloquea hasta su merge)**: US-MCP-023 (escenario «por la CLI un agente no ve más que por el MCP»), la parte CLI de US-MCP-020 y el cierre de los riesgos residuales de ADR-MCP-001 (S-01).
- **Dev Spec**: sí (`/aadd-devspec TS-GRP-010`); toca el canal y la seguridad. Revisión de `security-expert` antes del merge.
- **Modelo sugerido para el worker**: Opus (seguridad del canal).

### Notas

- **Revisar con UX** que la vista restringida le diga al humano cómo obtener la vista completa sin delatar la regla que la decide (DS-TS-GRP-010 D6: el desarrollador en `tmux` con un agente vivo recibe `mcp`).
