---
id: US-MCP-019
title: "Un agente prepara un worktree nuevo desde la rama base para otro trabajo"
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
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-007
    - US-MCP-009
    - US-MCP-011
    - US-GRD-014
ado:
  id: null
  url: null
covers: [BR-MCP-ELIG-004, BR-MCP-VAL-002, BR-MCP-EDGE-002, BR-MCP-VAL-001]
blocked_by: [ADR-MCP-001, ADR-CKP-002]
tags: [mcp, create-worktree, escritura, ola-5]
---

# US-MCP-019: Un agente prepara un worktree nuevo desde la rama base para otro trabajo

## Descripción

**Como** desarrollador orquestador, **quiero** que un agente cree por MCP una rama nueva desde la base confirmada con su worktree en la ubicación por defecto, sin poder elegir la ruta, **para** que el trabajo en paralelo empiece aislado y en un sitio predecible, sin reutilizar ni pisar nada.

**Valor**: un worktree por tarea, también cuando lo pide el agente (BR-14, Q-CKP-13).

## Reglas cubiertas

BR-MCP-ELIG-004 (`create_worktree`: desde la base confirmada, nunca reutilizar, no registra ni lanza agente) · BR-MCP-VAL-002 (nombre de rama) · BR-MCP-EDGE-002 (parte worktree: exige base confirmada) · BR-MCP-VAL-001 (parte: por MCP no hay parámetro de ruta, solo la plantilla del desarrollador) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-007, US-MCP-009 (flujo de escritura), US-MCP-011 (`expect_worktree`). De Guardrails: US-GRD-014 (rama base del equipo).
- **Habilitadores**: TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks), en propuesta, rama docs/arch-cockpit.
- **Dueña de la operación del catálogo**: comparte `create-worktree` con US-CKP-018, crear worktree desde el Cockpit con las reglas de Q-CKP-13 (propuesta, rama docs/stories-cockpit). La implementa la primera de las dos que entre; la otra la reutiliza.
- **Externas**: ADR-CKP-002 (**propuesto**, en docs/arch-cockpit): `create_worktree` → `create-worktree` **solo con la plantilla de ruta del desarrollador** (hallazgo de seguridad H-02; DEP-MCP-2 vía ADR-CKP-002). ADR-MCP-001 (DEP-MCP-1, no existe). ADR-MCP-001 y ADR-CKP-002 son bloqueos de arquitectura.
- **Regla enmendada**: BR-MCP-VAL-001 y BR-MCP-ELIG-004 (BR-MCP-001 v0.2) ya dicen que por MCP no hay ruta, solo la plantilla del desarrollador. Q-MCP-7 ("ruta opcional") queda estrechada por esa enmienda.

## Criterios de Aceptación

**Escenario: El agente crea una rama y su worktree**

Dado "claude-1" atribuido en "shop-feat-a", con la rama base "main" confirmada y Guardrails permite crear worktrees
Cuando "claude-1" pide `create_worktree` con la rama "feat/pagos"
Entonces existe la rama "feat/pagos" creada desde "main" y su worktree "/code/shop-feat-pagos", en la ubicación que fija la plantilla del desarrollador
  Y la respuesta trae la ruta, la rama y el aviso "esta sesión sigue en shop-feat-a; para trabajar ahí, abre una sesión del agente en esa carpeta"
  Y no queda ningún agente registrado ni lanzado en el worktree nuevo

**Esquema del escenario: Un nombre de rama no válido no crea nada**

Dado "claude-1" atribuido en "shop-feat-a"
Cuando pide `create_worktree` con la rama "<rama>"
Entonces la petición se rechaza con el motivo "nombre de rama no válido"
  Y no se crea ninguna rama ni worktree

Ejemplos:
| rama |
| --upload-pack=evil |
| feat..pagos |
| HEAD~3 |
| feat@{1} |
| refs/heads/x |
| un nombre hexadecimal de 40 caracteres, con forma de oid |
| un nombre con un carácter de control bidireccional |
| un nombre que supera el tope de longitud |

**Esquema del escenario: Nunca se reutiliza ni se pisa nada**

Dado "<situación>"
Cuando "claude-1" pide `create_worktree` con la rama "feat/pagos"
Entonces la petición se rechaza con el motivo "<motivo>"
  Y la rama y la carpeta existentes no cambian

Ejemplos:
| situación | motivo |
| la rama "feat/pagos" ya existe | la rama ya existe |
| la carpeta "/code/shop-feat-pagos" ya existe | la ruta ya existe |
| la carpeta padre de la ubicación de la plantilla es un enlace simbólico | ubicación no válida |

**Escenario: Sin rama base confirmada no se crea el worktree**

Dado el repo "shop" con la rama base pendiente de confirmar
Cuando "claude-1" pide `create_worktree` con la rama "feat/pagos"
Entonces la petición se rechaza con la acción "el desarrollador confirma la rama base desde GitRaptor"
  Y no se crea ninguna rama ni worktree

**Escenario: El agente no puede elegir dónde se crea el worktree**

Dado "claude-1" atribuido en "shop-feat-a"
Cuando pide `create_worktree` con la rama "feat/pagos" y una ruta "../../tmp/x"
Entonces la llamada se rechaza como mal formada
  Y no se crea ninguna rama ni worktree

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y ADR-CKP-002).
