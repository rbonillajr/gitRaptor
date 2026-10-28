---
id: US-MCP-009
title: "Un agente commitea por MCP con las reglas del repo y con un punto de recuperación previo"
type: us
status: partially-implemented
priority: high
created: 2026-10-04
updated: 2026-10-27
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-002
    - ADR-GRD-003
    - ADR-TMC-004
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-005
    - US-MCP-006
    - US-MCP-007
    - US-TMC-001
    - US-GRD-001
    - US-GRD-007
    - US-GRD-009
    - US-GRD-016
    - TS-TMC-004
ado:
  id: null
  url: null
covers: [BR-MCP-WF-001, BR-MCP-ELIG-002, BR-MCP-VAL-003, BR-MCP-AUTH-001, BR-MCP-CONS-002, BR-MCP-TIME-002]
blocked_by: [US-GRD-007]
tags: [mcp, safe-commit, escritura, guardrails, operacion-protegida, ola-3]
---

# US-MCP-009: Un agente commitea por MCP con las reglas del repo y con un punto de recuperación previo

## Descripción

**Como** desarrollador orquestador, **quiero** que `safe_commit` commitee en el worktree del agente solo si Guardrails lo permite, con snapshot previo y respetando los hooks y las convenciones del repo, **para** que la vía cómoda del agente sea también la que no pierde trabajo ni se salta las reglas.

**Valor**: primera escritura del MCP: misma decisión que por los hooks y 0 escrituras sin snapshot (NFR-01, BR-14).

## Reglas cubiertas

BR-MCP-WF-001 (flujo de una escritura) · BR-MCP-ELIG-002 (`safe_commit`: nunca amend, no-verify ni commit vacío; devuelve oid y rutas, no el mensaje) · BR-MCP-VAL-003 (mensaje de commit) · BR-MCP-AUTH-001 (decide Guardrails, capa `mcp`, misma decisión que los hooks) · BR-MCP-CONS-002 (sin snapshot no hay operación; nunca remoto) · BR-MCP-TIME-002 (cancelación y desconexión). Ejercita además BR-MCP-TIME-001 (tiempo máximo de un hook), cubierta en US-MCP-005 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-005, US-MCP-006, US-MCP-007 (requisito previo de toda escritura). De la Time Machine: US-TMC-001 (operación protegida), TS-TMC-004 (solicitante y operación protegida en el canal). De Guardrails: US-GRD-001 (decisión y mínimo seguro), US-GRD-007 (permisos por operación), US-GRD-009 (formato de commit).
- **Habilitadores**: TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks), en propuesta, rama docs/arch-cockpit.
- **Dueña de la operación del catálogo**: esta historia es dueña de `commit` y fija en su Dev Spec el flujo de escritura que reutilizan US-MCP-008, 018 y 019.
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe). ADR-CKP-002 (**propuesto**, en docs/arch-cockpit): `safe_commit` → operación `commit` del catálogo; el ejecutor respeta hooks y config del usuario. DEP-MCP-2 y DEP-MCP-5 (correspondencia herramienta → operación normalizada) se resuelven vía ADR-CKP-002. ADR-MCP-001 y ADR-CKP-002 son bloqueos de arquitectura.
- **Límites por solicitante** (S-03 completo, **condición de entrada dura**): cupo de rate limit compartido entre las conexiones del mismo solicitante y ≤ 8 conexiones por solicitante. US-MCP-008 solo cubre la cuota durable del snapshot; el resto pasa a esta historia (Enmienda (2026-10-08, US-MCP-008) de ADR-MCP-001) y es criterio de salida de M4 (v0.1.0).
- **Flujo heredado**: hereda de US-MCP-008 el cubo de escrituras, el mapeo de errores y la resolución del ámbito. `commit` no tiene avisos en `catalogVersion` 1, así que `safe_commit` se resuelve en una sola llamada; el parámetro `acknowledge` llega cuando `commit` tenga avisos (BR-MCP-WF-001; D-3 de `user-stories.md`, v1.4).
- **Escritura rechazada por Guardrails** (D5 de DS-US-MCP-013): `raptor-mcp` nunca rechaza por ningún campo de la vista previa (ni `decision` ni `warnings`); pasa `acknowledge` como `accepted_warnings` y decide `run`, para que la decisión quede registrada.
- **Desbloquea**: US-GRD-016 (Guardrails), junto con US-MCP-002 y US-MCP-003.
- **Transversal**: tiempo máximo de una escritura en la capa `mcp` de 300 s (ADR-CKP-002, S-MCP-1). `safe_commit` espera su desenlace hasta ese tope más la cola del cerrojo y el snapshot previo, y no responde `running` hasta que exista US-MCP-017 (DS-US-MCP-009 D2). Si la conexión se corta, la respuesta es `outcome-unknown` sin id, con la acción de revisar `status` o `raptor timeline`.
- **Liberación** (DS-US-MCP-009 G2, G3 y G4): se libera junto con US-MCP-010 (rutas) y US-MCP-011 (`expect_worktree`, que esta historia reserva por nombre y rechaza hasta entonces), y con US-MCP-007 (confused deputy) implementada. No son `blocked_by`: las tareas de esta historia se pueden construir hoy.
- **Convención de formato**: el ejemplo de la convención "tipo(ámbito): descripción" por MCP es de US-GRD-009 (escenario de paridad por MCP). Decisión del orquestador (2026-10-21), validada por PO.

## Criterios de Aceptación

**Escenario: El agente commitea las rutas que nombra**

Dado el repo "shop" en la allowlist, "claude-1" atribuido en "shop-feat-a" y Guardrails permite el commit
  Y "src/a.rs" modificado
Cuando "claude-1" pide `safe_commit` de "src/a.rs" con un mensaje válido
Entonces existe un snapshot previo de "shop-feat-a" anterior al commit
  Y los hooks del repo se ejecutaron
  Y la respuesta trae el oid del nuevo commit, la ruta "src/a.rs" y el worktree "shop-feat-a", sin el mensaje
  Y el timeline registra el commit con actor "claude-1" y canal "mcp"

**Escenario: Guardrails deniega con el mismo motivo que daría el hook**

Dado el repo "shop" con la política de autoría "agents-commit", que exige el trailer del agente
Cuando "claude-1" pide `safe_commit` con un mensaje válido sin el trailer "Co-Authored-By: Claude"
Entonces el commit se rechaza con la regla "agents-commit" y la acción
  Y el motivo es el mismo que daría la capa de hooks para ese commit
  Y la decisión queda registrada una sola vez con capa "mcp"
  Y el worktree no cambia

**Esquema del escenario: Un mensaje no válido no llega a Guardrails**

Dado "claude-1" atribuido en "shop-feat-a" con cambios que commitear
Cuando pide `safe_commit` con "<mensaje>"
Entonces el commit se rechaza con el motivo "<motivo>"
  Y el worktree no cambia

Ejemplos:
| mensaje | motivo |
| un mensaje vacío | el mensaje es obligatorio |
| un mensaje que supera el tope de longitud | mensaje demasiado largo |
| un mensaje con caracteres de control | texto no válido |

**Escenario: No se crea un commit vacío**

Dado "shop-feat-a" sin cambios que commitear
Cuando "claude-1" pide `safe_commit` con un mensaje válido
Entonces la petición se rechaza con el motivo "nada que commitear"
  Y no se crea ningún commit

**Esquema del escenario: Si el snapshot previo o un hook fallan, no hay commit**

Dado "<situación>"
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces no se crea ningún commit y la historia de la rama no cambia
  Y la respuesta trae el motivo "<motivo>" y el id de la operación

Ejemplos:
| situación | motivo |
| el disco está lleno y el snapshot previo no se puede tomar | no se pudo tomar el snapshot previo; no se hizo el commit |
| el hook "pre-commit" del repo supera el tiempo máximo de la operación | time-limit: el hook superó el tiempo máximo; no se hizo el commit |

**Escenario: Una cancelación del agente no deja el commit a medias**

Dado un `safe_commit` de "claude-1" ya iniciado
Cuando el usuario interrumpe a Claude Code y el cliente cancela la llamada
Entonces la operación termina igualmente
  Y el commit figura en el timeline con actor "claude-1"

**Escenario: Una escritura que requiere confirmación se rechaza con la acción** (heredado de US-MCP-013)

Dado el repo "shop" cuya configuración del equipo fija "commit" en "pedir confirmación"
  Y la cola de confirmación no disponible
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces la petición se rechaza con el motivo "requiere confirmación del desarrollador"
  Y la acción indica que la decisión es del desarrollador
  Y el worktree no cambia
  Y la decisión queda registrada una sola vez con capa "mcp"

**Escenario: Reintentar no cambia la decisión** (heredado de US-MCP-013)

Dado "claude-1" recibió "requiere confirmación del desarrollador" para un `safe_commit` de "src/a.rs"
Cuando repite la misma petición
Entonces recibe el mismo rechazo con el mismo motivo
  Y el worktree sigue sin cambios

**Criterio de certificación**: el servidor nunca pide confirmación al cliente del agente (sin elicitation; BR-MCP-WF-004).

**Criterio de certificación**: "el mismo motivo que daría el hook" significa el mismo código de regla; el texto legible compartido con el hook queda diferido (DS-US-MCP-009 D11).

**Criterio de certificación**: si el cliente corta o pasa a segundo plano la llamada antes de que termine, el commit termina igualmente y figura en el timeline (BR-MCP-TIME-002). ⚠️ **ASSUMPTION**: el tope real de Claude Code para una llamada no está verificado.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-MCP-009](../dev-specs/US-MCP-009-dev-spec.md) (validada por Arquitecto y PO el 2026-10-21).

## Estado de la implementación (2026-10-27)

Implementado en: PR #289 ([DS-US-MCP-009](../dev-specs/US-MCP-009-dev-spec.md), junto con US-MCP-010).

Queda abierto, y bloquea la liberación: US-MCP-007 en `main` (G3) y US-MCP-011 (G4); US-MCP-009 se libera junto con US-MCP-010 y US-MCP-011. Linux y Windows sin verificar (XP-47).
