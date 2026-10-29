---
id: US-GRD-009
title: "Los agentes entregan commits pequeños y con el formato que exige el equipo"
type: us
status: implemented
priority: medium
created: 2026-10-04
updated: 2026-10-29
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-008
    - US-MCP-009
tags:
  - guardrails
  - politicas
  - tamano-diff
  - formato-commit
---

# US-GRD-009: Los agentes entregan commits pequeños y con el formato que exige el equipo

## Descripción

**Como** desarrollador orquestador, **quiero** fijar un límite de tamaño de diff y un formato de commit en la configuración del equipo, **para** revisar el trabajo de los agentes en piezas pequeñas y con un historial legible.

**Valor**: diffs revisables (BRD P5); segundo bloque de políticas de Q-GRD-9.

## Reglas cubiertas

BR-VAL-003 (límite de diff en líneas cambiadas por commit, S-GRD-7; formato de commit, Conventional Commits como mínimo) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-008 (va después del primer bloque de políticas, Q-GRD-9).
- **Externas**: ninguna. Estuvo bloqueada por P8 (formato de la configuración, motor-local) hasta el 2026-10-04, cuando Rene Bonilla aceptó ADR-GRP-007, que la cierra.
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

El límite de diff y el formato de commit aplican por defecto solo a los agentes detectados o registrados (Q-GRD-48). Los mensajes que escribe Git no incumplen el formato (Q-GRD-49).

**Esquema del escenario: El límite de diff decide el commit**

Dado el repo "demo" con un límite de diff de 400 líneas en la configuración del equipo
Cuando un agente detectado hace un commit de "<líneas>" líneas cambiadas
Entonces el commit "<resultado>"

Ejemplos:
| líneas | resultado |
| 400 | se ejecuta |
| 1200 | no se ejecuta; el motivo da el tamaño y el límite |

**Escenario: Un mensaje fuera de formato se deniega con un ejemplo válido**

Dado el repo "demo" que exige Conventional Commits
Cuando un agente detectado hace un commit con el mensaje "arreglos varios"
Entonces el commit no se ejecuta
  Y el motivo nombra el formato exigido e incluye un ejemplo válido

**Escenario: Un mensaje con formato se acepta**

Dado el repo "demo" que exige Conventional Commits
Cuando un agente detectado hace un commit con el mensaje "fix: handle empty branch name"
Entonces el commit se ejecuta

**Escenario: Sin límite ni formato no se exige nada**

Dado el repo "demo" sin límite de diff ni formato de commit en ningún nivel
Cuando un agente detectado hace un commit de 5.000 líneas con el mensaje "wip"
Entonces el commit se ejecuta

**Esquema del escenario: A la persona solo se le exige si el equipo lo pide para todos**

Dado el repo "demo" con un límite de diff de 400 líneas y Conventional Commits, aplicados a "<a quién>"
Cuando la persona, sin agente, hace desde su terminal un commit de 1.200 líneas con el mensaje "arreglos varios"
Entonces el commit "<resultado>"

Ejemplos:
| a quién | resultado |
| los agentes (valor por defecto) | se ejecuta |
| todos | no se ejecuta; el motivo nombra el límite y el formato |

**Esquema del escenario: Los mensajes que escribe Git no incumplen el formato**

Dado el repo "demo" que exige Conventional Commits
Cuando un agente detectado "<operación>"
Entonces el commit "<resultado>"

Ejemplos:
| operación | resultado |
| fusiona "feat-x" con el mensaje que propone Git | se ejecuta |
| revierte un commit con el mensaje que propone Git | se ejecuta |
| hace un commit de corrección (fixup) de un commit con formato | se ejecuta |
| hace un commit normal con el mensaje "Merge stuff" | no se ejecuta |

**Escenario: Por MCP, el mensaje fuera de formato se deniega con el mismo motivo que por el hook** (de US-MCP-009)

Dado el repo "shop" en la allowlist del MCP con la convención de commit del equipo "tipo(ámbito): descripción"
Cuando "claude-1" pide `safe_commit` con el mensaje "arreglos"
Entonces el commit se rechaza con la regla de la convención y la acción
  Y el código de regla es el mismo que daría la capa de hooks para ese commit
  Y la decisión queda registrada una sola vez con capa "mcp"
  Y el worktree no cambia

_Escenario movido desde US-MCP-009 (DS-US-MCP-009 D14): Decisión del orquestador (2026-10-21), validada por PO. Probado en PR #313 (US-GRD-016): `us_grd_016_commit_policies::a_message_out_of_format_gets_the_hooks_reason_without_an_oid`._

## Requisitos Técnicos

Completado por el Arquitecto (2026-10-21). El detalle y su justificación están en la [Dev Spec DS-US-GRD-009](../dev-specs/US-GRD-009-tamano-diff-formato-commit.md).

- **Claves** (ADR-GRP-007): `policies.diffSize` (`maxLines` de 1 a 100 000, `exclude` con patrones de ruta) y `policies.commitFormat` (`style: conventional`, `types` con los once de `config-conventional` por defecto). Las dos tienen `appliesTo` (`agents` por defecto, Q-GRD-48) y `effect` (`deny` por defecto, o `ask`). Niveles: equipo, perfil y local.
- **Combinación** (BR-CONS-001, S-GRD-2): cada fuente aporta su regla de límite con sus propias exclusiones y el commit debe cumplirlas todas, así que el efectivo es el menor (filas 5 y 6 de US-GRD-010). El formato es el del primer nivel que lo fije: suelo, después worktree y después personal (Q-GRD-51). Subir el límite o añadir exclusiones en el suelo espera la confirmación.
- **Dónde se evalúa**: en `reference-transaction` `prepared`, en el daemon, sobre todos los commits nuevos que llegan a una rama (commit, `--amend`, `--no-verify`, plumbing, rebase, cherry-pick, revert, am). No en `commit-msg` ni en `pre-push`: enmienda propuesta de ADR-GRD-002 § 1. Las fusiones solo se evalúan por el formato.
- **Medida** (S-GRD-7): líneas añadidas más eliminadas contra el primer padre, con la detección de renombrados por defecto de Git; los binarios cuentan 0. El oráculo es `git diff --numstat -M`.
- **Formato**: gramática de Conventional Commits 1.0.0, con un parser lineal sin expresiones regulares. Los mensajes que escribe Git (fusión, revert, fixup/squash/amend) no incumplen (Q-GRD-49).
- **Contrato**: reglas `policy.diff-size` y `policy.commit-format` y capacidad `guard.commit-policies`. Del mensaje solo se lee en el daemon la primera línea, y ese texto no sale nunca de él. El motivo da el tamaño y el límite, o el formato con un ejemplo válido, en en/es.
- **Rendimiento**: < 100 ms p95 por evaluación (ADR-GRD-002 § 5). Solo se lee si una regla alcanza al actor. Los topes deniegan (fail-closed) y nunca permiten.
- **Pendiente**: la misma decisión por MCP (US-GRD-016) y la validación en Linux y Windows reales.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-009](../dev-specs/US-GRD-009-tamano-diff-formato-commit.md) (lista para implementar y liberar; Q-GRD-48, Q-GRD-49 y Q-GRD-51 decididas el 2026-10-21).

## Estado de la implementación (2026-10-28)

Implementado en: PR #296. Dev Spec: [DS-US-GRD-009](../dev-specs/US-GRD-009-tamano-diff-formato-commit.md) (§ 10: resultados y residuos). Verificado en macOS; Linux lo cubre el CI; Windows: **Pendiente: etapa de validación multiplataforma**. Pendiente: medición de coste de D15. El escenario por MCP se cierra en PR #313 (US-GRD-016).
