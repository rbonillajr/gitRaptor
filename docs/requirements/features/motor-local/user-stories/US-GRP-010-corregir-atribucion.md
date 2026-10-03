---
id: US-GRP-010
title: "El desarrollador corrige una atribución automática equivocada"
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
    - US-GRP-004
    - US-GRP-007
    - US-GRP-009
tags:
  - motor-local
  - atribucion
---

# US-GRP-010: El desarrollador corrige una atribución automática equivocada

## Descripción

**Como** desarrollador orquestador, **quiero** corregir una atribución que el motor detectó mal para que la correcta la reemplace, **para** arreglar en un paso una detección errónea y poder confiar en la atribución.

**Valor**: la detección se equivoca a veces; corregirla cuesta una sola acción y no deja una sesión fantasma. Corregir reemplaza; registrar otro agente añade una sesión (US-GRP-011, Q33).

## Reglas cubiertas

BR-CONS-002 (corregir reemplaza la detectada, Q33; alcanza la sesión desde su inicio, Q37; sin detección no se corrige, Q38) · BR-CONS-003 · BR-VAL-002 · BR-AUTH-001 (un agente no corrige) · BR-CONS-005 (la corrección persiste) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-007, US-GRP-009; US-GRP-004 (la corrección sobrevive a que el motor deje de ejecutarse y vuelva a arrancar).
- **Externas**: ninguna. Abierta P17 (si al retirar la corrección los eventos reatribuidos vuelven a la atribución detectada); los escenarios solo exigen lo decidido.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Corregir reemplaza la atribución detectada**

Dado una única sesión en "feat-login", de "Claude Code" con origen "detectado"
Cuando el desarrollador corrige la atribución de "feat-login" a "Codex"
Entonces "feat-login" tiene una sola sesión, de "otro agente: Codex" con origen "registrado"
  Y "feat-login" no figura como compartido
  Y un commit posterior en "feat-login" tiene como actor "otro agente: Codex"

**Escenario: La corrección alcanza lo ya atribuido a la sesión desde su inicio**

Dado una sesión de "Claude Code" detectada en "feat-login" a la que se atribuyeron los commits "c1" y "c2"
  Y un commit "c0" anterior, de otra sesión de "Claude Code" ya terminada en "feat-login"
Cuando el desarrollador corrige la atribución de "feat-login" a "Codex"
Entonces "c1" y "c2" figuran como de "otro agente: Codex" con origen "registrado"
  Y "c0" sigue figurando como de "Claude Code" con origen "detectado"

**Escenario: La detección no deshace una corrección vigente**

Dado la atribución de "feat-login" corregida a "otro agente: Codex"
  Y la sesión que el motor había detectado como "Claude Code" sigue presente
Cuando hay nueva actividad en "feat-login"
Entonces "feat-login" sigue con una sola sesión, de "otro agente: Codex" con origen "registrado"

**Escenario: Retirar la corrección devuelve la atribución detectada**

Dado la atribución de "feat-login" corregida a "otro agente: Codex"
  Y la sesión que el motor había detectado como "Claude Code" sigue presente
Cuando el desarrollador retira la corrección
Entonces "feat-login" tiene una sola sesión, de "Claude Code" con origen "detectado"

**Escenario: La corrección se mantiene tras reiniciar el motor**

Dado la atribución de "feat-login" corregida a "otro agente: Codex", con los commits "c1" y "c2" reatribuidos
  Y la sesión que el motor había detectado como "Claude Code" sigue presente
Cuando el motor deja de ejecutarse y vuelve a arrancar
Entonces "feat-login" sigue con una sola sesión, de "otro agente: Codex" con origen "registrado"
  Y "c1" y "c2" siguen figurando como de "otro agente: Codex"

**Esquema del escenario: Una corrección que no corresponde se rechaza**

Dado el repo "demo" observado, con "Claude Code" detectado en "feat-login" y ninguna atribución detectada en "feat-pagos"
  Y el repo "otro" no observado
Cuando "<quien>" corrige a "Codex" la atribución de "<worktree>"
Entonces el motor rechaza la corrección indicando "<motivo>"
  Y ninguna atribución ni sesión cambia

Ejemplos:
| quien | worktree | motivo |
| el desarrollador | un worktree de "otro" | que el repo no está observado |
| el desarrollador | "feat-pagos" | que no hay ninguna atribución detectada que corregir y que use el registro |
| el agente "Claude Code" de "feat-login" | "feat-login" | que solo el desarrollador puede corregir una atribución |
