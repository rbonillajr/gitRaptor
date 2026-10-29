---
id: US-DSK-010
title: "El usuario guarda sus cambios en un commit desde la app y puede deshacerlo"
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
    - US-MCP-009
    - US-MCP-010
    - US-GRD-009
    - US-GRD-018
    - US-DSK-006
    - US-DSK-009
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - commit
  - must
---

# US-DSK-010: El usuario guarda sus cambios en un commit desde la app y puede deshacerlo

## Descripción

**Como** desarrollador novato, **quiero** elegir qué archivos entran, escribir el mensaje y crear el commit desde la app, **para** guardar mi trabajo sin la terminal y con la tranquilidad de poder deshacerlo.

**Valor**: BR-27 (UI-2, Must). Nivel **UI-2**. Imprescindible.

## Reglas

- El commit pasa por las políticas del repo: tamaño de diff y formato del mensaje (US-GRD-009) y autoría (US-GRD-018, D6).
- Antes de escribir hay punto de recuperación (NFR-01; Q-DSK-2).
- Solo entran los archivos que el usuario eligió.

## Dependencias

- **Reutiliza**: la operación `commit` del catálogo (US-MCP-009 y US-MCP-010, hoy solo para el MCP), US-GRD-009, US-GRD-018; `operation.prepare`, `operation.run`.
- **Historias**: US-DSK-009 (ver qué se va a commitear), US-DSK-006 (deshacerlo).
- **Qué falta en el motor**: DEP-DSK-4, ofrecer `commit` a la capa del humano (ADR-CKP-002 § 1 lo marca "No en el MVP" para el Cockpit). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Commit de los archivos elegidos**

Dado "main" con "readme.md" y "pagos.rs" cambiados
Cuando el usuario elige solo "readme.md", escribe "docs: update readme" y confirma
Entonces se crea un commit con "readme.md" y "pagos.rs" sigue sin commitear
  Y el historial registra el commit con su punto de recuperación previo

**Escenario: El mensaje no cumple el formato del equipo**

Dado "tienda" con política de formato Conventional Commits
Cuando el usuario intenta commitear con el mensaje "cambios"
Entonces el commit no se crea
  Y el usuario sabe qué formato exige el repo

**Escenario: Deshacer el commit recién hecho**

Dado el commit "docs: update readme" recién creado desde la app
Cuando el usuario lo deshace
Entonces el commit desaparece de "main" y "readme.md" vuelve a estar cambiado sin commitear

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
