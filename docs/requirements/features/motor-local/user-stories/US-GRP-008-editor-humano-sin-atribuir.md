---
id: US-GRP-008
title: "El trabajo que el desarrollador hace en su editor nunca se atribuye a Claude Code"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-007
    - US-GRP-009
tags:
  - motor-local
  - atribucion
  - editor-humano
---

# US-GRP-008: El trabajo que el desarrollador hace en su editor nunca se atribuye a Claude Code

## Descripción

**Como** desarrollador orquestador que edita en Cursor u otro editor, **quiero** que mis cambios nunca se atribuyan a Claude Code ni a ningún agente, y que lo dudoso quede "sin atribuir", **para** que un "deshacer lo que hizo el agente" no se lleve mi trabajo.

**Valor**: protege el trabajo humano en el caso real del MVP: el humano en su editor y Claude Code en el mismo repo (Q32).

## Reglas cubiertas

BR-EDGE-004 · BR-EDGE-003 (agente sin registrar: "sin atribuir", Q35) · BR-CONS-003 (el motor no emite "humano", Q34) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-007; US-GRP-009 (en serie: comparten el contrato de "quién hizo un evento", agente con su origen o "sin atribuir"; lo fija la Dev Spec de US-GRP-009).
- **Externas**: el criterio de "evidencia suficiente" se valida en el spike (c) del BRD (riesgo R2).
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Esquema del escenario: Un cambio sin agente identificado queda sin atribuir**

Dado el repo "demo" observado sin ninguna sesión de Claude Code ni agente registrado en "feat-pagos"
Cuando "<autor>" modifica "pagos.txt" en "feat-pagos"
Entonces el estado de "feat-pagos" no tiene ninguna sesión de agente
  Y el cambio de "pagos.txt" figura como "sin atribuir"
  Y no figura como de "Claude Code", de ningún otro agente ni como "humano"

Ejemplos:
| autor |
| el desarrollador desde Cursor |
| el desarrollador desde otro editor |
| un agente sin soporte completo que no se registró, como Codex |

> Los tres ejemplos producen el mismo resultado observable a propósito: un agente sin registrar que no se detecta es indistinguible del trabajo del desarrollador (Q35), y el motor no emite "humano" (Q34).

**Escenario: Un cambio dudoso junto a Claude Code no se atribuye a Claude Code**

Dado una sesión de "Claude Code" activa en "feat-login"
Cuando el desarrollador modifica "README.md" en "feat-login" desde su editor, un proceso distinto de la sesión de Claude Code
Entonces el cambio de "README.md" figura como "sin atribuir"
  Y no figura como de "Claude Code"

**Escenario: Un commit del desarrollador junto a Claude Code no se atribuye a Claude Code**

Dado una sesión de "Claude Code" activa en "feat-login"
Cuando el desarrollador hace un commit en "feat-login" desde su editor, un proceso distinto de la sesión de Claude Code
Entonces el evento del commit figura como "sin atribuir"
