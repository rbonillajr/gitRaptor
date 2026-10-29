---
id: US-DSK-002
title: "El novato protege su repo con un clic, sabe qué se instaló y puede quitarlo"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-DSK-001
    - US-GRD-001
    - US-GRD-003
    - US-GRD-004
    - US-GRD-020
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - guardrails
  - must
---

# US-DSK-002: El novato protege su repo con un clic, sabe qué se instaló y puede quitarlo

## Descripción

**Como** desarrollador novato, **quiero** activar la protección de mi repo sabiendo qué cambia en él y cómo deshacerlo, **para** que un agente no pueda hacer force-push ni tocar las ramas protegidas sin que yo lo decida.

**Valor**: BR-27 (UI-1, Must); BR-11 y BR-12. Nivel **UI-1**.

## Reglas

- Proteger exige el consentimiento explícito del usuario y explica qué se instala antes de hacerlo (Q-DSK-6).
- Si el repo no se puede proteger, el usuario ve el motivo y lo que no se puede impedir (lo no impedible de `guard.plan`).
- Rechazar la protección queda recordado y no se vuelve a ofrecer en cada apertura.
- Instalar la protección es un comando reservado al humano (ADR-GRP-005 § 6; Q-DSK-3).
- Retirar la protección deja el repo exactamente como estaba (US-GRD-003).

## Dependencias

- **Reutiliza**: US-GRD-001 (bloqueo del force-push), US-GRD-003 (retirar sin rastro), US-GRD-004 (aviso de protección caída), US-GRD-020 (estado de la protección); `guard.plan`, `guard.status`, `guard.install`, `guard.decline`.
- **Historias**: US-DSK-001 (el repo ya está observado).
- **Qué falta en el motor**: DEP-DSK-1 (prueba de presencia humana desde la app); retirar la protección también es comando reservado. transversal (lo define el Arquitecto).
- **Decisión pendiente**: Q-DSK-15.

## Criterios de Aceptación

**Escenario: Proteger con consentimiento**

Dado "tienda" observado y sin protección
Cuando el usuario revisa qué se instalará y acepta proteger el repo
Entonces "tienda" queda protegido
  Y un force-push de un agente a "main" se deniega

**Escenario: Repo que no se puede proteger**

Dado "legacy" con un gestor de hooks que GitRaptor no puede encadenar
Cuando el usuario pide protegerlo
Entonces "legacy" queda sin proteger
  Y el usuario conoce el motivo y lo que la protección no podría impedir

**Escenario: El usuario rechaza la protección**

Dado "tienda" observado y sin protección
Cuando el usuario decide no protegerlo
Entonces el repo sigue sin cambios
  Y la protección no se le vuelve a ofrecer al abrir "tienda"

**Escenario: Desproteger devuelve el repo a como estaba**

Dado "tienda" protegido
Cuando el usuario retira la protección
Entonces la configuración de hooks de "tienda" es la misma que antes de protegerlo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), flujo de onboarding.
- **Dev Spec:** pendiente.
