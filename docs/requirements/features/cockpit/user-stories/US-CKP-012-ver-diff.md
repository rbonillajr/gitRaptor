---
id: US-CKP-012
title: "El desarrollador revisa lo que un agente integraría antes de hacer merge"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-27
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
- **Contrato que fija**: las consultas bajo demanda de la TUI (N8 de ADR-CKP-003: id de petición, cancelación y topes); lo reutilizan US-CKP-021 y US-CKP-022.
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

Detalle en [DS-US-CKP-012](../dev-specs/US-CKP-012-ver-diff.md), que fija además el contrato de las consultas bajo demanda (N8 de ADR-CKP-003).

- Métodos nuevos `diff.worktree` y `query.cancel`, en sus módulos del contrato; ninguno se ofrece a `raptor-mcp` (D1).
- El daemon calcula el diff en un pool propio con `gix`, sin filtros ni `git` hijo, y responde fuera de orden a la conexión que lo pidió; nunca lo registra ni lo persiste (D2, D6).
- Entraría al merge: merge-base con la base confirmada contra `HEAD`. Sin commitear: `HEAD` contra el working tree, con los archivos sin seguimiento (D5).
- Topes por archivo, en total y de tiempo; al superarlos, "truncado" con aviso, nunca un error. Binarios sin contenido (D4, D7).
- La TUI gana un cursor de fila y la tecla `d`, desactivada con motivo en un worktree no disponible; el diff se ve a pantalla completa, sin atribución por archivo (D8, D10 a D12).

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** [DS-US-CKP-012](../dev-specs/US-CKP-012-ver-diff.md) (`draft`, sin gaps bloqueantes).
