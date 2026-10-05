---
id: US-CKP-013
title: "El desarrollador abre el worktree de un agente en su editor"
type: us
status: draft
priority: low
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
  - editor
  - should
---

# US-CKP-013: El desarrollador abre el worktree de un agente en su editor

## Descripción

**Como** desarrollador orquestador, **quiero** abrir con una tecla el worktree de un agente en mi editor, **para** revisar o corregir su trabajo sin buscar la ruta a mano.

**Valor**: BR-07, Should (Q-CKP-9).

## Reglas cubiertas

BR-CKP-VAL-003 · BR-CKP-EDGE-007 · BR-CKP-ELIG-006 (abrir en el editor) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001.
- **Huecos del motor**: DEP-CKP-12 (el daemon lanza el proceso sin shell) y DEP-CKP-13 (clave del editor solo en perfil y local personal).

## Criterios de Aceptación

**Escenario: Editor configurado con argumentos**

Dado el editor configurado como `code --wait` en el perfil
Cuando el desarrollador abre "feat-pagos" en el editor
Entonces se lanza `code` con `--wait` y la ruta de "feat-pagos", y la TUI sigue respondiendo

**Escenario: Editor de terminal**

Dado `$EDITOR=vim` y ningún editor en el perfil
Cuando el desarrollador abre "feat-pagos" en el editor
Entonces la TUI se suspende mientras "vim" está abierto y vuelve a su estado al cerrarlo

**Escenario: Metacaracteres de shell rechazados**

Dado `$EDITOR` con el valor `vim; rm -rf ~`
Cuando el desarrollador abre "feat-pagos" en el editor
Entonces no se lanza ningún proceso y la TUI informa "el editor contiene caracteres de shell no admitidos"

**Escenario: Sin editor, error accionable**

Dado ningún editor en el perfil y sin `$VISUAL` ni `$EDITOR`
Cuando el desarrollador abre "feat-pagos" en el editor
Entonces la TUI informa cómo definir `$EDITOR` o la clave del editor en el perfil

**Escenario: La configuración del equipo no puede fijar el editor**

Dado una clave de editor en la configuración del equipo commiteada en el repo
Cuando el desarrollador abre "feat-pagos" en el editor
Entonces esa clave se ignora y se usa el perfil o `$VISUAL`/`$EDITOR`

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente. Pendiente: etapa de validación multiplataforma (editores en Linux y Windows).
