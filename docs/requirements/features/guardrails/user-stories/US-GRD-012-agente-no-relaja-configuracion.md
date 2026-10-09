---
id: US-GRD-012
title: "Un agente no puede relajar las reglas del equipo cambiando su configuración"
type: us
status: implemented
priority: high
created: 2026-10-04
updated: 2026-10-09
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-008
    - US-GRD-010
tags:
  - guardrails
  - proteccion-configuracion
  - rutas-prohibidas
---

# US-GRD-012: Un agente no puede relajar las reglas del equipo cambiando su configuración

## Descripción

**Como** desarrollador orquestador, **quiero** que la configuración de Guardrails sea una ruta prohibida para los agentes por defecto y que los cambios del equipo entren por un commit mío, **para** que un agente no pueda quitarse sus propias restricciones.

**Valor**: las reglas no las cambia quien está sujeto a ellas (Q-GRD-7).

## Reglas cubiertas

BR-AUTH-004 (la configuración de Guardrails es ruta prohibida para agentes por defecto; los cambios del equipo entran por commit revisado) · BR-AUTH-001 (relajar está reservado al humano) · BR-VAL-001 (las relajaciones solo salen de la configuración del equipo en la rama principal; el worktree solo endurece, Q-GRD-20) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-008 (rutas prohibidas), US-GRD-010 (los tres niveles).
- **Externas**: ninguna bloqueante. Las rutas de la configuración las fija ADR-GRP-007, que cerró P8 al aceptarse (Rene Bonilla, 2026-10-04). Por Q-GRD-17 rige la última versión commiteada de la configuración del equipo en el worktree de la operación, y por Q-GRD-20 esa versión solo endurece: las relajaciones salen solo de la rama principal. Distinguir al humano es transversal (lo define el Arquitecto; R-GRD-3). Que un agente escriba en disco la configuración personal, que no se versiona, no lo puede impedir una regla sobre commits: queda como riesgo R-GRD-4 para el Arquitecto, fuera de esta historia.
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Escenario: Un agente intenta commitear una configuración del equipo más laxa**

Dado el repo "demo" protegido, cuya configuración del equipo deniega force-push
  Y el agente "codex" registrado en "feat-x"
Cuando "codex" hace un commit que cambia la configuración del equipo para permitir force-push
Entonces el commit no se ejecuta y el motivo nombra la protección de la configuración
  Y el intento queda en el registro con actor "codex"

**Escenario: Un agente edita la configuración del equipo sin commitear y no relaja nada**

Dado el repo "demo" protegido, cuya configuración del equipo commiteada deniega force-push
Cuando un agente edita esa configuración en su worktree para permitir force-push, sin commitear, y hace force-push desde ese worktree
Entonces la operación no se ejecuta, porque rige la última versión commiteada de la configuración del equipo (Q-GRD-17)

**Escenario: Una configuración más laxa commiteada solo en el worktree de un agente no relaja nada**

Dado el repo "demo" protegido, cuya configuración del equipo en la rama principal deniega force-push
  Y el worktree "feat-x" de un agente está en un commit, creado sin pasar por la protección, cuya configuración del equipo permite force-push
Cuando un proceso hace force-push desde "feat-x"
Entonces la operación no se ejecuta, porque una relajación solo puede venir de la configuración del equipo en la rama principal (Q-GRD-20)

**Escenario: Un agente intenta borrar la configuración del equipo**

Dado el repo "demo" protegido con configuración del equipo
Cuando un agente hace un commit que borra esa configuración
Entonces el commit no se ejecuta y la configuración sigue en la rama

**Escenario: El commit de un agente que no toca la configuración se evalúa como siempre**

Dado el repo "demo" protegido
Cuando un agente hace un commit que solo modifica "src/main.rs"
Entonces el commit se ejecuta

**Escenario: El desarrollador cambia la configuración del equipo**

Dado el repo "demo" con la configuración del equipo protegida
Cuando el desarrollador cambia esa configuración y la commitea con su confirmación consciente
Entonces el commit se ejecuta y el cambio queda en su rama para revisión

## Requisitos Técnicos

Ver el Brief y la Enmienda (2026-10-08) de ADR-GRD-003: regla `policy.config-protected`, aviso `config.relax-ignored` y capacidad `guard.config-protection`.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [Brief de implementación](../../../../dev-briefs/layered-config.md) (ADR-GRD-003, Enmienda 2026-10-08, US-GRD-010 y US-GRD-012).

## Estado de la implementación (2026-10-09)

Implementado en: PR #217. Brief: [Brief de implementación](../../../../dev-briefs/layered-config.md).

Los seis escenarios y el escenario de la configuración local (un agente que la edita para relajar una regla: la regla efectiva no cambia y queda un aviso con su actor) están cubiertos por `apps/cli/tests/guard_us_grd_012.rs`, `crates/core/tests/us_grd_012_config_guard.rs` y `us_grd_012_config_bypass.rs` (verificado en macOS; Linux lo cubre el CI de ubuntu; Windows: **Pendiente: etapa de validación multiplataforma**, XP-39).

Decisiones y límites declarados:

- **Confirmación consciente (escenario 6).** Se lee como el commit propio de la persona, decidido por el **actor** que ve el hook (ascendencia del proceso), nunca por el autor, el committer ni el trailer, porque el agente commitea con la identidad de la persona. Con actor «sin atribuir» cuenta como la persona (riesgo residual aceptado del MVP, Q35 y R-GRD-3). Un commit sin hooks no confirma nada.
- **Alcance de la protección.** Solo la configuración del equipo (`.gitraptor/`) es ruta prohibida. Perfil y local no están protegidos contra escritura del agente (**R-GRD-4**; el cierre es el trinquete de Q-GRD-32): un agente puede quitar endurecimientos de su propio perfil o local, nunca bajar del equipo.
- **Cambio de comportamiento.** Un agente cuyo movimiento no se puede verificar (más de 256 commits nuevos, objetos que faltan) se deniega aunque no haya reglas de rutas.
- **Residuo M-01.** Retroceder la rama base o forjar `refs/remotes/<r>/main` no crea commits nuevos y no pasa por esta regla; lo compensa el suelo confirmado, y el cierre es la marca de agua de US-GRD-014 (`policy-floor`, `policy-reach`).
- Las ediciones sin commitear no se registran (Q-GRD-17, ADR-GRD-004).
