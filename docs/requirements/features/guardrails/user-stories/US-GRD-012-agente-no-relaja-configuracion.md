---
id: US-GRD-012
title: "Un agente no puede relajar las reglas del equipo cambiando su configuración"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
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
  - bloqueada
---

# US-GRD-012: Un agente no puede relajar las reglas del equipo cambiando su configuración

## Descripción

**Como** desarrollador orquestador, **quiero** que la configuración de Guardrails sea una ruta prohibida para los agentes por defecto y que los cambios del equipo entren por un commit mío, **para** que un agente no pueda quitarse sus propias restricciones.

**Valor**: las reglas no las cambia quien está sujeto a ellas (Q-GRD-7).

## Reglas cubiertas

BR-AUTH-004 (la configuración de Guardrails es ruta prohibida para agentes por defecto; los cambios del equipo entran por commit revisado) · BR-AUTH-001 (relajar está reservado al humano) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-008 (rutas prohibidas), US-GRD-010 (los tres niveles).
- **Externas**: **bloqueada** por el ADR de formato P8 (motor-local), que fija qué rutas son la configuración. Un escenario depende de la pregunta abierta P-GRD-17 (qué copia de la configuración del equipo rige; ver índice). Distinguir al humano es transversal (lo define el Arquitecto; R-GRD-3). Que un agente escriba en disco la configuración personal, que no se versiona, no lo puede impedir una regla sobre commits: queda como riesgo R-GRD-4 para el Arquitecto, fuera de esta historia.
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Escenario: Un agente intenta commitear una configuración del equipo más laxa**

Dado el repo "demo" protegido, cuya configuración del equipo deniega force-push
  Y el agente "codex" registrado en "feat-x"
Cuando "codex" hace un commit que cambia la configuración del equipo para permitir force-push
Entonces el commit no se ejecuta y el motivo nombra la protección de la configuración
  Y el intento queda en el registro con actor "codex"

**Escenario: Un agente edita la configuración del equipo sin commitear** *(Depende de P-GRD-17)*

Dado el repo "demo" protegido, cuya configuración del equipo commiteada deniega force-push
Cuando un agente edita esa configuración en su worktree para permitir force-push, sin commitear, y hace force-push desde ese worktree
Entonces la operación no se ejecuta, porque rige la última versión commiteada de la configuración del equipo

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

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
