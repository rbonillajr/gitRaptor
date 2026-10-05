---
id: US-MCP-018
title: "Un agente se pone al día con la rama base y, si choca, su rama queda como estaba"
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
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-007
    - US-MCP-009
    - US-MCP-011
    - US-GRD-001
    - US-GRD-008
    - US-GRD-014
ado:
  id: null
  url: null
covers: [BR-MCP-ELIG-003, BR-MCP-WF-002, BR-MCP-WF-003, BR-MCP-EDGE-007, BR-MCP-EDGE-002, BR-MCP-EDGE-003]
blocked_by: [ADR-MCP-001, ADR-CKP-002]
tags: [mcp, safe-rebase, rebase-atomico, escritura, ola-5]
---

# US-MCP-018: Un agente se pone al día con la rama base y, si choca, su rama queda como estaba

## Descripción

**Como** desarrollador orquestador, **quiero** que `safe_rebase` rebase la rama del worktree del agente sobre la rama base confirmada y que, si choca, se aborte solo y deje el repo como antes con la lista de conflictos, **para** que un agente sin editor ni botón de abortar nunca deje un worktree a medias que bloquee el undo y a otras sesiones.

**Valor**: rebase atómico (BR-14). Diverge a conciencia del Cockpit, donde el rebase que choca queda detenido (Q-MCP-6 frente a Q-CKP-11).

## Reglas cubiertas

BR-MCP-ELIG-003 (precondiciones de `safe_rebase`) · BR-MCP-WF-002 (abort automático en la misma operación) · BR-MCP-WF-003 (abort fallido: detenido, informado, escrituras rechazadas) · BR-MCP-EDGE-007 (rama ya empujada: decide Guardrails y se avisa de la divergencia) · BR-MCP-EDGE-002 (parte rebase: exige base confirmada) · BR-MCP-EDGE-003 (parte rebase: bloqueado con otra sesión presente). Ejercita además BR-MCP-EDGE-008 (HEAD separado) y BR-MCP-TIME-001 (tiempo máximo), cubiertas en US-MCP-011 y US-MCP-005 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-007, US-MCP-009 (flujo de escritura), US-MCP-011 (`expect_worktree` y precondiciones). De Guardrails: US-GRD-001 (mínimo seguro: force-push denegado), US-GRD-008 (ramas protegidas), US-GRD-014 (rama base del equipo).
- **Habilitadores**: TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks), en propuesta, rama docs/arch-cockpit.
- **Dueña de la operación del catálogo**: esta historia es dueña del modo `atomic` de `rebase-onto-base`. Relación: US-CKP-015, rebase desde el Cockpit (propuesta, rama docs/stories-cockpit), que usa la misma operación en su modo detenido.
- **Externas**: ADR-CKP-002 (**propuesto**, en docs/arch-cockpit): `safe_rebase` → `rebase-onto-base` en modo `atomic`, con el abort dentro de la operación y el abort fallido como `stopped`; flags fijados y transiciones de refs del abort no reevaluadas (DEP-MCP-2 y DEP-MCP-5 vía ADR-CKP-002). ADR-MCP-001 (DEP-MCP-1, no existe). ADR-MCP-001 y ADR-CKP-002 son bloqueos de arquitectura.
- **Pendiente de ADR-MCP-001**: ADR-CKP-002 § 12 exige que el agente enumere los avisos del plan al ejecutar; esta ficha supone una herramienta de una sola llamada. El escenario de la rama ya empujada (BR-MCP-EDGE-007) puede cambiar según lo que se decida.

## Criterios de Aceptación

**Escenario: El rebase termina y queda protegido**

Dado "claude-1" atribuido en "shop-feat-a", con la rama "feat-a" limpia y la rama base "main" confirmada
  Y Guardrails permite el rebase
Cuando "claude-1" pide `safe_rebase`
Entonces "feat-a" queda rebasada sobre "main" y la respuesta trae el nuevo oid
  Y existe un snapshot previo al rebase
  Y el timeline registra el rebase con actor "claude-1" y canal "mcp"

**Esquema del escenario: Si el rebase choca o se queda sin tiempo, se aborta solo y la rama queda como antes**

Dado "<situación>"
Cuando "claude-1" pide `safe_rebase`
Entonces la respuesta indica "<respuesta>"
  Y "feat-a" y su worktree quedan exactamente como antes de la petición
  Y el timeline registra "<evento>" con actor "claude-1"

Ejemplos:
| situación | respuesta | evento |
| "feat-a" y "main" modifican las mismas líneas de "src/pago.rs" | rebase cancelado: conflicto en src/pago.rs; tu rama está como antes | rebase abortado por conflicto |
| el rebase supera el tiempo máximo de la operación | rebase cancelado por time-limit; tu rama está como antes | rebase abortado por time-limit |

**Escenario: Si el abort falla, el worktree queda detenido y nadie más escribe en él**

Dado un `safe_rebase` de "claude-1" que choca y cuyo abort automático falla
Cuando termina la petición
Entonces la respuesta informa que no se pudo cancelar el rebase, con la acción "pide al desarrollador que lo resuelva en GitRaptor" y la existencia del snapshot previo
  Y una escritura posterior por MCP en "shop-feat-a" se rechaza con el motivo "operación en curso"

**Esquema del escenario: Sin las precondiciones no se rebasa nada**

Dado "shop-feat-a" con "<situación>"
Cuando "claude-1" pide `safe_rebase`
Entonces la petición se rechaza con el motivo "<motivo>" y la acción que lo resuelve
  Y "feat-a" no cambia

Ejemplos:
| situación | motivo |
| cambios sin commitear | el worktree tiene cambios sin commitear |
| la rama base pendiente de confirmar | la rama base está pendiente de confirmar |
| la rama base no confirmada | la rama base no está confirmada |
| "claude-3" con sesión presente en el mismo worktree | claude-3 sigue trabajando en este worktree |
| un merge de Git a medias | operación en curso |
| HEAD separado | HEAD separado: crea una rama o cámbiate a una |

**Escenario: Rebasar una rama ya empujada avisa de la divergencia**

Dado "feat-a" ya empujada a "origin/feat-a" y Guardrails permite el rebase
Cuando "claude-1" pide `safe_rebase`
Entonces el rebase se hace
  Y la respuesta avisa de que "feat-a" diverge de "origin/feat-a" y de que el force-push está bloqueado por política

**Escenario: Rebasar la rama base se deniega igual que por Git directo**

Dado "shop-feat-a" en la rama base "main", protegida
Cuando "claude-1" pide `safe_rebase`
Entonces la petición se rechaza con la regla "la rama base está protegida"
  Y el motivo es el mismo que daría la capa de hooks para un rebase de "main"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y ADR-CKP-002).
