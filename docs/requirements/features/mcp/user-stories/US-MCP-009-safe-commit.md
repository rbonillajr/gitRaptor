---
id: US-MCP-009
title: "Un agente commitea por MCP con las reglas del repo y con un punto de recuperación previo"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
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
blocked_by: []
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
- **Desbloquea**: US-GRD-016 (Guardrails), junto con US-MCP-002 y US-MCP-003.
- **Transversal**: ⚠️ **ASSUMPTION**: tiempo máximo de una escritura en la capa `mcp` de 300 s (supuesto de ADR-CKP-002, S-MCP-1); al vencer, la respuesta declara el estado y el id de la operación.

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

Dado el repo "shop" con la convención de commit del equipo "tipo(ámbito): descripción"
Cuando "claude-1" pide `safe_commit` con el mensaje "arreglos"
Entonces el commit se rechaza con la regla de la convención y la acción
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

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y ADR-CKP-002).
