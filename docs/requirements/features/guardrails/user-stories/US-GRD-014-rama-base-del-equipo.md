---
id: US-GRD-014
title: "El equipo fija la rama base del repo y Guardrails la protege"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-09
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-007
    - US-GRP-013
tags:
  - guardrails
  - rama-base
  - configuracion-equipo
---

# US-GRD-014: El equipo fija la rama base del repo y Guardrails la protege

## Descripción

**Como** desarrollador orquestador, **quiero** definir la rama base de un repo en la configuración del equipo, el único nivel que la admite, **para** que Guardrails proteja esa rama y el motor mida el ahead/behind contra ella, igual en todos los clones.

**Valor**: la rama de integración que fija el equipo queda protegida igual en todos los clones.

## Reglas cubiertas

BR-CONS-003 (rama base solo del nivel de equipo; `main` por defecto; se lee de la configuración commiteada en la copia conocida de la rama principal, Q-GRD-18 y Q-GRD-20; rige la rama base confirmada por el desarrollador, Q-GRD-21) · BR-AUTH-001 (confirmar la rama base inicial, de forma explícita si el repo ya tiene configuración del equipo, y cada cambio está reservado al humano, Q-GRD-22 y Q-GRD-23) · BR-WF-002 (diagnóstico de rama base no confirmada o pendiente, Q-GRD-25) · BR-EDGE-001 (el mínimo seguro protege la rama base efectiva) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (permisos del equipo, que reutilizan la lectura de los tres niveles de US-GRP-013, motor-local). No depende del comando de edición (US-GRD-013): el valor se puede escribir a mano. La coherencia con la rama base que lee el motor (US-GRP-016, motor-local) es una prueba de integración posterior, anotada en el índice.
- **Externas**: ninguna bloqueante. P8 (formato de la configuración, motor-local) quedó cerrada por ADR-GRP-007, aceptado por Rene Bonilla el 2026-10-04. La confirmación de la rama base usa el mismo mecanismo que las demás acciones reservadas del MVP y no espera al factor de autenticación del sistema operativo (Q-GRD-22). Distinguir al humano: transversal (lo define el Arquitecto; R-GRD-3).
- **Transversal**: Windows, macOS y Linux.
- **Adopción**: el mismo estado tras adoptar una protección huérfana se verifica en US-GRD-003 (escenario de adoptar). Tras perder el perfil y antes de adoptar, la rama base leída sigue protegida (DS-US-GRD-014, D12).

## Criterios de Aceptación

**Escenario: Sin confirmación inicial, la rama base del equipo y main quedan protegidas hasta que el desarrollador confirma**

Dado el repo "demo" protegido, cuya configuración del equipo en la rama principal define la rama base "develop"
  Y la rama base de "demo" aún no está confirmada en esta máquina
Cuando un proceso intenta borrar la rama "develop" y después la rama "main"
Entonces las dos operaciones se deniegan y el motivo nombra la protección de la rama base
  Y el estado de protección de "demo" muestra la rama base como no confirmada, con la acción para confirmarla
  Y tras la confirmación del desarrollador, la rama base protegida es solo "develop"

**Escenario: Sin rama base del equipo, la rama base es main**

Dado el repo "demo" protegido sin rama base en la configuración del equipo
Cuando un proceso intenta borrar la rama "main"
Entonces la operación no se ejecuta

**Escenario: Un nivel personal no cambia la rama base**

Dado el repo "demo" con rama base "develop" en la configuración del equipo y "release" en la configuración local personal
Cuando un proceso intenta borrar la rama "release"
Entonces la decisión es la de cualquier otra rama no protegida
  Y la rama base efectiva de "demo" sigue siendo "develop"

**Escenario: Todos los worktrees comparten la rama base de la rama principal**

Dado el repo "demo" protegido, cuya rama principal en el remoto es "main"
  Y la configuración del equipo commiteada en "main" define la rama base "develop", confirmada por el desarrollador
  Y el worktree "feat-a" está en un commit cuya configuración del equipo dice "develop" y el worktree "feat-b" en otro cuya configuración dice "release"
Cuando un proceso intenta borrar la rama "develop" desde "feat-a" y después desde "feat-b"
Entonces las dos veces la operación no se ejecuta y el motivo nombra la rama base "develop"
  Y borrar "release" desde "feat-b" recibe la decisión de cualquier otra rama no protegida

**Escenario: En una máquina nueva aplica desde el primer momento**

Dado una máquina nueva con un perfil vacío y "demo" recién clonado con rama base "develop" en la configuración del equipo
Cuando el desarrollador añade "demo", instala la protección y después confirma de forma explícita su rama base
Entonces borrar "develop" se deniega desde la primera operación

**Escenario: Un cambio de rama base en la rama principal no se aplica hasta que el desarrollador lo confirma**

Dado el repo "demo" protegido con la rama base confirmada "main"
Cuando llega a la rama principal un cambio de la configuración del equipo que fija la rama base "develop"
Entonces borrar "main" y borrar "develop" se deniegan mientras el cambio está pendiente
  Y el estado de protección de "demo" muestra la rama base pendiente de confirmar, con la acción para confirmarla
  Y si un agente intenta confirmarlo, se rechaza y el intento queda en el registro
  Y tras la confirmación del desarrollador, la rama base protegida es solo "develop"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

> **Nota de TS-GRD-001 (2026-10-04)**: la lectura ya existe. `TeamLoader::load` (`crates/policy`) da la rama base confirmada (`base_branch()`), el conjunto que protege Guardrails (`guarded_base_branches()`) y los diagnósticos `base-unconfirmed`, `base-change-pending` y `floor-relax-pending`. Esta historia aporta el **escritor** de lo confirmado: el comando reservado de confirmación llama a `RepoStore::set_confirmed_team_baseline` (`crates/core`), que guarda la rama base y el blob del suelo confirmados.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-014](../dev-specs/US-GRD-014-rama-base-del-equipo.md) (2026-10-09; decisiones del orquestador validadas por el Arquitecto y el PO).
