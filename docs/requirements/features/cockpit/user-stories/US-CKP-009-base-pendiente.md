---
id: US-CKP-009
title: "Con la rama base sin confirmar, la predicción contra la base queda pendiente"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-006
    - US-GRD-014
    - US-GRP-016
tags:
  - cockpit
  - prediccion-conflictos
  - rama-base
  - must
---

# US-CKP-009: Con la rama base sin confirmar, la predicción contra la base queda pendiente

## Descripción

**Como** desarrollador orquestador, **quiero** que, mientras la rama base no esté confirmada, la predicción contra la base diga "pendiente" y me ofrezca confirmarla, **para** no fiarme de un cálculo hecho contra una base que no elegí.

**Valor**: BR-06 (Must). Evita predicciones contra la rama equivocada.

## Reglas cubiertas

BR-CKP-WF-005 (parte de predicción; la parte de acciones desactivadas se verifica en US-CKP-014, US-CKP-015 y US-CKP-018) · BR-CKP-EDGE-001 (pares contra una base que no existe) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-006; US-GRP-016 (base pendiente publicada); US-GRD-014 (confirmación de la rama base). Confirmar la base es una acción reservada, no una operación del catálogo: en esta historia se hace con la CLI; ofrecerla desde la TUI reutilizará el comando reservado que fija US-CKP-019.

## Criterios de Aceptación

**Escenario: Cambio de base pendiente de confirmar**

Dado la base del equipo cambiada de "main" a "develop" y aún sin confirmar
Cuando el desarrollador mira la predicción
Entonces los pares contra la base dicen "pendiente" y la vista muestra "Rama base pendiente de confirmar (main → develop)" con el comando para confirmarla
  Y el ⚡ entre "claude-1" y "claude-2" sigue visible

**Escenario: Al confirmar, los pares contra la base se calculan**

Dado la base pendiente "develop"
Cuando el desarrollador la confirma con la CLI
Entonces los pares contra "develop" pasan a "calculando" y después muestran su resultado

**Escenario: Base que no existe en el repo**

Dado "develop" confirmada y sin esa rama en el repo
Cuando el motor publica la predicción
Entonces los pares contra la base dicen "no calculable: develop no encontrada" y los pares entre worktrees siguen

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (PolicyBanner de base pendiente).
- **Dev Spec:** pendiente.
