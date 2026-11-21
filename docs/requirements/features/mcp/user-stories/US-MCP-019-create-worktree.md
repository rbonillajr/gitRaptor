---
id: US-MCP-019
title: "Un agente prepara un worktree nuevo desde la rama base para otro trabajo"
type: us
status: ready
priority: high
milestone: M3
created: 2026-10-04
updated: 2026-11-21
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
    - US-CKP-018
    - US-WTL-001
    - US-GRP-024
  technical:
    - TS-CKP-002
    - TS-CKP-003
    - TS-GRP-010
ado:
  id: null
  url: null
covers: [BR-MCP-ELIG-004, BR-MCP-VAL-002, BR-MCP-EDGE-002, BR-MCP-VAL-001]
blocked_by: []
tags: [mcp, create-worktree, escritura, ola-5, hito-m3]
---

# US-MCP-019: Un agente prepara un worktree nuevo desde la rama base para otro trabajo

## Descripción

**Como** desarrollador orquestador, **quiero** que un agente cree por MCP una rama nueva desde la base confirmada con su worktree en la ubicación por defecto, sin poder elegir la ruta, **para** que el trabajo en paralelo empiece aislado y en un sitio predecible, sin reutilizar ni pisar nada.

**Valor**: un worktree por tarea, también cuando lo pide el agente (BR-14, Q-CKP-13).

## Reglas cubiertas

BR-MCP-ELIG-004 (`create_worktree`: desde la base confirmada, nunca reutilizar, no registra ni lanza agente) · BR-MCP-VAL-002 (nombre de rama) · BR-MCP-EDGE-002 (parte worktree: exige base confirmada) · BR-MCP-VAL-001 (parte: por MCP no hay parámetro de ruta, solo la plantilla del desarrollador) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Bloqueos (`blocked_by: []`)**: ninguno. Ya están implementadas US-GRD-014 (rama base del equipo, PR #277), TS-CKP-002 (catálogo y ejecutor, PR #73), TS-CKP-003 (decisión única heredada por los hooks, PR #261), TS-GRP-010 (perfil `mcp` por solicitante, S-01, PR #368) y el flujo de escritura de US-MCP-009 (PR #289).
- **Relacionadas que no bloquean**: US-MCP-007 (draft, confused deputy): el solicitante ya sale de la ascendencia del proceso, como en `snapshot`, y nada de esta historia depende de su cuerpo. US-MCP-011 (draft, `expect_worktree`): hasta que entre, una petición con `expect_worktree` se rechaza como mal formada (fallo cerrado); US-MCP-011 lo añade después a todas las escrituras. Ambos motivos vienen de DS-US-MCP-019.
- **Perfil del solicitante (TS-GRP-010)**: un agente recibe el perfil `mcp` también cuando usa la CLI o habla directo con el motor, así que por ninguna puerta obtiene más que lo que permite `create_worktree`: rama nueva, sin reutilizar, sin copia desde la semilla y sin elegir ruta.
- **Lanzar un agente queda fuera (BRD § 6.4, enmienda 2026-11-10)**: GitRaptor lanza un agente solo a petición explícita del humano (`raptor new`, «Nuevo agente», US-WTL-013), nunca a petición de un agente ni por el MCP. Esta historia solo prepara el worktree.
- **Dueña de la operación del catálogo**: esta historia construye `create-worktree` para producción. US-CKP-018 (crear desde la TUI) y US-WTL-001 (`raptor new` con copia desde la semilla) la reutilizan y dependen de esta (decisión del orquestador, 2026-11-21, validada por PO).
- **Externas**: ADR-CKP-002 (aceptado): `create_worktree` → `create-worktree` **solo con la plantilla de ruta del desarrollador** (hallazgo de seguridad H-02). ADR-MCP-001 (aceptado).
- **Regla enmendada**: BR-MCP-VAL-001 y BR-MCP-ELIG-004 (BR-MCP-001 v0.2) ya dicen que por MCP no hay ruta, solo la plantilla del desarrollador. Q-MCP-7 ("ruta opcional") queda estrechada por esa enmienda.
- **Decisión del orquestador (2026-11-21), validada por PO**: pasa a `ready` con `milestone: M3`, porque es prerrequisito de US-WTL-001 y US-CKP-018, Must de M3. Se añaden los escenarios de reutilizar una rama, copia desde la semilla y ruta pedidas por un agente que habla el protocolo completo (DS-US-WTL-001 D14). La cuota de worktrees por agente está decidida en DS-US-MCP-019 D14 (tope de 8 por sesión de agente y repo) y la construye US-GRP-024.
- **Condición de salida de M3: US-GRP-024** (tope de worktrees por agente, security-review H-01): criterio 12 de salida de M3 en [release-plan.md](../../../release-plan.md). No se añade a `blocked_by`: US-GRP-024 extiende esta operación y está bloqueada por esta historia, así que el bloqueo inverso crearía un ciclo.

## Criterios de Aceptación

**Escenario: El agente crea una rama y su worktree**

Dado "claude-1" atribuido en "shop-feat-a", con la rama base "main" confirmada y Guardrails permite crear worktrees
Cuando "claude-1" pide `create_worktree` con la rama "feat/pagos"
Entonces existe la rama "feat/pagos" creada desde "main" y su worktree "/code/shop-feat-pagos", en la ubicación que fija la plantilla del desarrollador
  Y la respuesta trae la ruta, la rama y el aviso fijo "esta sesión sigue en su worktree; para trabajar en el nuevo, abre una sesión del agente en esa carpeta"
  Y "shop-feat-a" viaja como dato aparte (`session_worktree`), no dentro del aviso
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
| la rama "feat/pagos" ya existe | ya existe una rama con ese nombre o que choca con él |
| la carpeta "/code/shop-feat-pagos" ya existe | la ruta ya existe |
| la carpeta padre de la ubicación de la plantilla es un enlace simbólico | ubicación no válida |

**Escenario: Sin rama base confirmada no se crea el worktree**

Dado el repo "shop" con la rama base pendiente de confirmar
Cuando "claude-1" pide `create_worktree` con la rama "feat/pagos"
Entonces la petición se rechaza con la acción "el desarrollador confirma la rama base desde GitRaptor"
  Y no se crea ninguna rama ni worktree

**Escenario: Base confirmada sin rama local**

Dado la rama base "main" confirmada pero sin rama local "main" en el repo "shop"
Cuando "claude-1" pide `create_worktree` con la rama "feat/pagos"
Entonces la petición se rechaza con el motivo "la rama base no existe en local" y la acción "el desarrollador la crea o la revisa en GitRaptor"
  Y no se crea ninguna rama ni worktree, y nunca se usa la rama remota

**Escenario: Un agente no usa el espacio reservado ni caracteres fuera de la lista**

Dado "claude-1" atribuido en "shop-feat-a"
Cuando pide `create_worktree` con la rama "gitraptor/seed" o con la rama "feat/pagos-é"
Entonces la petición se rechaza con el motivo "nombre de rama no válido"
  Y no se crea ninguna rama ni worktree
  Y las ramas de agentes solo usan letras ASCII, dígitos y los signos . _ / - (DS-US-MCP-019 D17 y D20)

**Escenario: Un agente no llena el disco con worktrees**

Dado el repo "shop" con el techo de worktrees de agentes alcanzado, o con el espacio libre por debajo del suelo
Cuando "claude-1" pide `create_worktree` con la rama "feat/otro"
Entonces la petición se rechaza con el motivo "límite de worktrees alcanzado" o "sin espacio libre", con su acción para el desarrollador
  Y no se crea ninguna rama ni worktree
  Y el tope por sesión de agente lo añade US-GRP-024, condición de salida de M3 (DS-US-MCP-019 D14, D18 y G9)

**Escenario: El agente no puede elegir dónde se crea el worktree**

Dado "claude-1" atribuido en "shop-feat-a"
Cuando pide `create_worktree` con la rama "feat/pagos" y una ruta "../../tmp/x"
Entonces la llamada se rechaza como mal formada
  Y no se crea ninguna rama ni worktree

**Esquema del escenario: Por la herramienta del MCP no se reutiliza una rama ni se copia desde la semilla**

Dado "claude-1" atribuido en "shop-feat-a"
  Y la rama "feat/pagos" existe y no está sacada en ningún worktree
Cuando pide `create_worktree` con la rama "feat/pagos" y además "<argumento>"
Entonces la llamada se rechaza como mal formada
  Y la rama "feat/pagos" sigue sin worktree
  Y no se copia ningún archivo desde la semilla

Ejemplos:
| argumento |
| reutilizar la rama existente |
| copiar desde la semilla |

**Esquema del escenario: Un agente que habla con el motor sin pasar por el MCP tampoco obtiene más**

Dado "claude-1", que recibe el perfil `mcp` aunque se conecte al motor con la CLI `raptor` o con un cliente propio
Cuando pide crear el worktree de la rama "feat/pagos" con "<argumento>"
Entonces la petición se rechaza con el motivo "no disponible para un agente"
  Y no se crea ninguna rama ni worktree
  Y no se copia ningún archivo desde la semilla

Ejemplos:
| argumento |
| reutilizar la rama existente "feat/pagos" |
| copiar desde la semilla |
| la ruta "/tmp/x" |

## Requisitos Técnicos

Arquitecto, 2026-11-21: [DS-US-MCP-019](../dev-specs/US-MCP-019-dev-spec.md) (`spec-ready`). Argumentos `reuse_branch` y `seed_copy` y ruta explícita solo para la capa `cockpit` (D12); el MCP no los tiene. Tope de 8 worktrees por sesión de agente y repo, configurable (D14), decidido en la DS y construido en una historia aparte.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-MCP-019](../dev-specs/US-MCP-019-dev-spec.md) (2026-11-21, `spec-ready`).
