---
id: US-CKP-012
title: "El desarrollador revisa lo que un agente integraría antes de hacer merge"
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
    - US-CKP-001
tags:
  - cockpit
  - acciones-por-agente
  - diff
  - must
---

# US-CKP-012: El desarrollador revisa lo que un agente integraría antes de hacer merge

## Descripción

**Como** desarrollador orquestador, **quiero** ver desde la fila de un agente lo que entraría al merge y, aparte, lo que tiene sin commitear, **para** revisar su trabajo sin salir de la TUI ni interrumpir al agente.

**Valor**: BR-07 (Must). Revisar es el paso previo a integrar con confianza.

## Reglas cubiertas

BR-CKP-CALC-005 · BR-CKP-ELIG-006 (ver diff) · BR-CKP-EDGE-004 (diff sin atribución por archivo) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001.
- **Huecos del motor**: DEP-CKP-3 (diff bajo demanda publicado por el daemon).
- **Transversal**: que el diff nunca sale por el MCP se verifica con F-001-05.

## Criterios de Aceptación

**Escenario: Dos secciones, commiteado y sin commitear**

Dado "feat-pagos" con 3 commits desde su merge-base con "main" y 1 archivo modificado sin commitear
Cuando el desarrollador pide ver el diff de "feat-pagos"
Entonces la vista muestra "entraría al merge (3 archivos)" y, aparte, "sin commitear (1 archivo)"

**Escenario: Binarios y archivos enormes**

Dado "logo.png" cambiado y un archivo de texto que supera el tope por archivo
Cuando el desarrollador ve el diff
Entonces "logo.png" aparece como "binario" sin contenido y el archivo grande aparece truncado con un aviso

**Escenario: Ver el diff no afecta al agente**

Dado "claude-1" Activo en "feat-pagos"
Cuando el desarrollador ve el diff
Entonces el repo, la sesión de "claude-1" y su working tree no cambian

**Escenario: Worktree compartido, sin atribución por archivo**

Dado "feat-pagos" compartido por "claude-1" y "claude-2"
Cuando el desarrollador ve el diff
Entonces ningún archivo se atribuye a un agente concreto

**Escenario: Worktree no disponible**

Dado "feat-x" publicado como no disponible
Cuando el desarrollador intenta ver su diff
Entonces la acción está desactivada con el motivo "el worktree no está disponible"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente.
