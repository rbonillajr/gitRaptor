---
id: US-DSK-015
title: "El usuario sube y trae los cambios del remoto desde la app con la protección del repo"
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
    - US-GRD-001
    - US-GRD-008
    - US-TMC-014
    - US-DSK-012
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - remoto
  - must
  - pendiente-de-rene
---

# US-DSK-015: El usuario sube y trae los cambios del remoto desde la app con la protección del repo

## Descripción

**Como** desarrollador novato, **quiero** traer los cambios del remoto y subir los míos desde la app, sabiendo antes cuántos commits van y vienen, **para** usar la app como mi cliente Git diario sin volver a otra herramienta.

**Valor**: BR-27 (UI-2, Must). Nivel **UI-2**. Imprescindible. **Condicionada a Q-DSK-14 (pendiente de Rene)**: hoy BR-CKP-CONS-002 dice "nunca push ni fetch" y no puede pasar a Dev Spec hasta que Rene decida.

## Reglas

- Solo el humano, como acción explícita en la app; nunca un agente ni el MCP (propuesta de Q-DSK-14, opción b).
- Un push pasa por Guardrails como cualquier otro: un force-push a una rama protegida se deniega (US-GRD-001, US-GRD-008).
- Traer cambios que modifican la copia de trabajo tiene punto de recuperación previo (NFR-01).
- Las credenciales son las del usuario, en su configuración de Git (NFR-07); la app no las guarda.

## Dependencias

- **Reutiliza**: US-GRD-001, US-GRD-008 (la capa de hooks ya frena el push), US-TMC-014 (aviso de lo que sigue en el remoto), US-GRP-012 (ahead/behind).
- **Historias**: US-DSK-012 (ramas).
- **Qué falta en el motor**: DEP-DSK-4, operaciones de red en el catálogo (hoy prohibidas por BR-CKP-CONS-002 y la Validación 16 de ADR-CKP-002). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Traer y subir cambios**

Dado "main" con 2 commits locales sin subir y 1 commit nuevo en el remoto
Cuando el usuario trae los cambios y después sube los suyos
Entonces "main" local y la del remoto tienen los mismos 3 commits

**Escenario: Force-push a una rama protegida**

Dado "main" protegida y reescrita en local
Cuando el usuario intenta subirla forzando
Entonces el push se deniega
  Y el usuario conoce la regla y cómo hacer una excepción consciente

**Escenario: Falla la autenticación con el remoto**

Dado un remoto que rechaza las credenciales del usuario
Cuando el usuario intenta subir "main"
Entonces nada cambia en el remoto ni en local
  Y el usuario sabe que el problema son sus credenciales de Git

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente; bloqueada por Q-DSK-14.
