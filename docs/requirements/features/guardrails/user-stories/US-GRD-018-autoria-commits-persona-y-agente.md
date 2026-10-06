---
id: US-GRD-018
title: "Cada commit entra a nombre de la persona y deja constancia del agente que lo hizo, con la exigencia que fija el equipo"
type: us
status: draft
priority: medium
created: 2026-10-06
updated: 2026-10-06
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-007
    - US-GRD-009
    - US-GRD-010
    - US-GRD-005
    - US-GRP-007
    - US-GRP-009
    - US-GRD-019
tags:
  - guardrails
  - politicas
  - autoria-commits
  - co-authored-by
---

# US-GRD-018: Cada commit entra a nombre de la persona y deja constancia del agente que lo hizo, con la exigencia que fija el equipo

## Descripción

**Como** desarrollador orquestador, **quiero** que el commit de un agente entre a mi nombre con el agente como coautor, y poder exigirlo, prohibir que los agentes hagan commits o solo registrarlo según el repo, **para** que el historial diga quién responde de cada cambio y qué agente participó, con la exigencia que permita el harness de mi empresa.

**Valor**: BRD BR-26 y D6 (decisión de Rene Bonilla, 2026-10-06). Las credenciales de Git son de la persona y el agente actúa con ellas; el equipo elige lo estricto que quiere ser.

**Alcance (decisión del orquestador, 2026-10-06, validada por el PO)**: esta historia es la **política** (decidir el commit). Quién ejecutó frente a a nombre de quién entra, en `raptor events` y en el Cockpit, y la validación de la pista `inferred` contra el trailer, son US-GRD-019. Exportar estas entradas es BR-24 (Fase 3).

## Reglas cubiertas

BR-AUTH-005 (política de autoría de commits: modelo por defecto `agents-commit`, `human-author` con bloquear o avisar, `flexible`; solo aplica a commits que ejecuta un agente detectado o registrado), BR-CONS-001 (un nivel personal endurece, no relaja), BR-CONS-004 (entradas de autoría en el registro), BR-CALC-001 (la decisión más restrictiva) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (permisos y políticas en la configuración del equipo), US-GRD-009 (política sobre el mensaje del commit, misma evaluación), US-GRD-010 (precedencia entre niveles), US-GRD-005 (registro), US-GRP-007 (sesiones de Claude Code detectadas) y US-GRP-009 (agente registrado).
- **Externas**: ninguna.
- **Transversal**: Windows, macOS y Linux. Reconocer qué nombres de un trailer `Co-Authored-By` identifican a cada agente y saber, en el momento del commit, qué proceso lo ejecuta: transversal (lo define el Arquitecto). Por MCP, la misma decisión cuando exista US-GRD-016 (BR-CONS-002); no es criterio de esta historia.

## Criterios de Aceptación

**Esquema del escenario: El trailer del agente decide su commit con el modelo por defecto y con agents-commit**

Dado el repo "demo" con "<política>" de autoría
  Y la identidad de Git del usuario es "Ana Pérez"
Cuando una sesión de Claude Code hace un commit "<trailer>"
Entonces el commit "<resultado>"

Ejemplos:
| política | trailer | resultado |
| ninguna política configurada | con el trailer "Co-Authored-By: Claude" | se ejecuta; el autor es "Ana Pérez" y Claude figura como coautor |
| ninguna política configurada | sin trailer | no se ejecuta; el motivo nombra "agents-commit" e incluye un trailer de ejemplo |
| la política "agents-commit" | con el trailer "Co-Authored-By: Claude" | se ejecuta; el autor es "Ana Pérez" y Claude figura como coautor |
| la política "agents-commit" | sin trailer | no se ejecuta; el motivo nombra "agents-commit" e incluye un trailer de ejemplo |

**Escenario: Un commit sin agente detectado no necesita trailer**

Dado el repo "demo" con la política "agents-commit"
Cuando un proceso sin atribuir hace un commit sin trailer
Entonces el commit se ejecuta

**Esquema del escenario: Con human-author, el commit de un agente se bloquea o se avisa según la política**

Dado el repo "demo" con la política "human-author" marcada como "<efecto>"
Cuando una sesión de Claude Code hace un commit con el trailer "Co-Authored-By: Claude"
Entonces el commit "<resultado>"
  Y el registro de "demo" anota el commit de Claude Code con la decisión

Ejemplos:
| efecto | resultado |
| bloquear | no se ejecuta; el motivo dice que en este repo los commits los hace la persona |
| avisar | se ejecuta y el aviso dice que en este repo los commits los hace la persona |

**Escenario: Con flexible, el commit del agente solo se registra**

Dado el repo "demo" con la política "flexible"
Cuando una sesión de Claude Code hace un commit sin trailer
Entonces el commit se ejecuta
  Y el registro de "demo" anota que lo ejecutó Claude Code, su autor y que no lleva trailer
  Y esa entrada no cuenta como acción peligrosa bloqueada

**Escenario: Un nivel personal no relaja la política de autoría del equipo**

Dado el repo "demo" con la política "human-author" marcada como "bloquear" en la configuración del equipo
  Y la configuración local de "demo" fija la política "flexible"
Cuando una sesión de Claude Code hace un commit con el trailer "Co-Authored-By: Claude"
Entonces el commit no se ejecuta
  Y el motivo nombra "human-author" y la configuración del equipo

## Requisitos Técnicos

- **Gobierno**: ADR-GRD-003 § 1 a § 4 con su Enmienda (2026-10-06, US-GRD-018): el actor entra en la condición de las reglas de autoría, `warn` es `allow` con `notices[]` y el modo degradado pierde estas reglas; ADR-GRP-012, Enmienda (2026-10-06, autoría de commits) § 4; ADR-GRD-002 § 1 (fila Commit) y ADR-GRP-016 (capacidad `guard.notices`).
- **Forma**: clave `policies.commitAuthorship` (`mode`, `onAgentCommit`), dispatchers `pre-commit` y `commit-msg`, actor S4 en el daemon, tabla versionada de identidades del trailer en `crates/policy`. Detalle en la Dev Spec.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-018](../dev-specs/US-GRD-018-autoria-commits-persona-y-agente.md) (2026-10-06; compartida por US-GRD-018 y US-GRD-019, esta historia es su PR-A).
