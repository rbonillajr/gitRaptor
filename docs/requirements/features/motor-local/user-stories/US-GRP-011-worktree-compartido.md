---
id: US-GRP-011
title: "El desarrollador ve todas las sesiones de un worktree compartido"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-004
    - US-GRP-007
    - US-GRP-009
tags:
  - motor-local
  - worktrees
  - sesiones
---

# US-GRP-011: El desarrollador ve todas las sesiones de un worktree compartido

## Descripción

**Como** desarrollador orquestador, **quiero** que registrar otro agente en un worktree donde ya trabaja uno añada su sesión, y ver todas las sesiones con el worktree marcado como compartido, **para** detectar a tiempo cuándo dos agentes pisan el mismo trabajo.

**Valor**: dos agentes en un worktree dejan de ser invisibles. Registrar otro agente añade una sesión; reemplazar una detección errónea es corregir (US-GRP-010, Q33); registrar al mismo agente que ya se detectaba confirma su sesión y no vuelve compartido el worktree (US-GRP-009, Q39).

## Reglas cubiertas

BR-CONS-004 (registrar otro agente añade sesión, Q33; registrar al mismo agente ya detectado no, Q39) · BR-CONS-005 (lo compartido persiste) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-007, US-GRP-009; US-GRP-004 (el worktree compartido sobrevive a que el motor deje de ejecutarse y vuelva a arrancar).
- **Externas**: ninguna. La atribución por archivo dentro de un worktree compartido queda fuera del MVP (Q7).
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Registrar otro agente junto a una sesión detectada la añade y el worktree pasa a compartido**

Dado una única sesión en "feat-login", de "Claude Code" con origen "detectado"
Cuando el desarrollador registra "Codex" en "feat-login"
Entonces el estado de "feat-login" lista dos sesiones: "Claude Code" (detectado) y "otro agente: Codex" (registrado)
  Y la sesión de "Claude Code" conserva su atribución
  Y "feat-login" figura como compartido

**Escenario: Dos sesiones de Claude Code en el mismo worktree**

Dado una sesión de "Claude Code" activa en "feat-login"
Cuando se lanza una segunda sesión de Claude Code en "feat-login"
Entonces el estado de "feat-login" lista las dos sesiones de "Claude Code"
  Y "feat-login" figura como compartido

**Escenario: El worktree deja de ser compartido cuando queda una sola sesión**

Dado "feat-login" compartido por "Claude Code" y "otro agente: Codex"
Cuando el desarrollador retira el registro de "Codex"
Entonces "feat-login" ya no figura como compartido
  Y conserva la sesión de "Claude Code"

**Escenario: Un worktree compartido sigue compartido tras reiniciar el motor**

Dado "feat-login" compartido por "Claude Code" (detectado) y "otro agente: Codex" (registrado)
  Y la sesión de "Claude Code" sigue presente
Cuando el motor deja de ejecutarse y vuelve a arrancar
Entonces el estado de "feat-login" lista las dos sesiones con su origen
  Y "feat-login" figura como compartido

**Escenario: El editor del desarrollador no convierte un worktree en compartido**

Dado una sesión de "Claude Code" activa en "feat-login"
Cuando el desarrollador edita archivos de "feat-login" en Cursor sin registrarlo como agente
Entonces "feat-login" tiene una sola sesión
  Y no figura como compartido
