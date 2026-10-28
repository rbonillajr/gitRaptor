---
id: US-TMC-022
title: "La Time Machine nunca pasa del tope de disco que fijé"
type: us
status: draft
priority: medium
created: 2026-10-05
updated: 2026-10-28
domain: GRP
epic: E-001
feature: time-machine
source: inline
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  adrs: [ADR-TMC-007, ADR-GRP-015]
  stories: [US-TMC-016, US-TMC-004, US-GRP-017]
  specs: [DS-US-TMC-022]
covers: [BR-TMC-TIME-001, BR-TMC-EDGE-002, D-TMC-15, D-TMC-25, D-TMC-37, D-TMC-38, D-TMC-39]
blocked_by: []
tags: [time-machine, retencion, disco, tope, recursos]
---

# US-TMC-022: La Time Machine nunca pasa del tope de disco que fijé

## Descripción

**Como** desarrollador orquestador con un disco limitado
**Quiero** fijar un tope de disco para la Time Machine y que, al acercarse a ese tope o a la cuota de un repo, se purguen con aviso los puntos más antiguos que no estén protegidos
**Para** usar la Time Machine sin miedo a que llene mi disco, sin perder nunca el punto anterior a una operación destructiva

**Valor**: el disco de la Time Machine queda acotado (RES-09), no solo por antigüedad, y sigue sin perderse nada sin aviso (NFR-01). La cuota por repo (5 GiB por defecto) se gestiona igual que el tope total: quien trabaja en un solo repo no deja de capturar al llegar a la cuota mientras haya puntos antiguos que purgar (D-TMC-38, TQ-24 → a, abierta a veto de Rene).

> **Origen**: decisión de Rene Bonilla (2026-10-05): tope configurable que purga lo más antiguo sin tocar los snapshots protegidos y avisa. Amplía D-TMC-15 y BR-TMC-TIME-001 con la purga por tamaño (ADR-TMC-007, Enmienda 2026-10-05). Prioridad: decisión del orquestador (2026-10-05), validada por el PO: **Should, fuera de M1**; sube a M1 si US-GRP-017 mide durante el dogfooding un almacén por encima de 2 GiB.

## Reglas cubiertas

BR-TMC-TIME-001 (ampliada: purga por tamaño, del tope total y de la cuota del repo, con aviso visto y 24 h; nunca puntos protegidos) · BR-TMC-EDGE-002 (causas del hueco "sin espacio") · D-TMC-15 (ampliada), D-TMC-25 (su parte de la cuota, sustituida), D-TMC-37, D-TMC-38 (un solo aviso del tope; cuota como el tope), D-TMC-39 (tope y cuota detienen la observación y el manual, nunca los previos; repos que ya no se observan) · RES-09 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-TMC-016 (purga en dos fases, aviso, gracia y puntos protegidos; esta historia reutiliza el mismo trabajo de purga).
- **Transversal**: verificado en macOS con tamaños simulados del almacén; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: Aviso al acercarse al tope**

Dado un tope de 10 GiB y una Time Machine de 8,2 GiB
Cuando corre el trabajo de purga
Entonces la CLI y la TUI avisan de que la Time Machine pasó el 80 % del tope
  Y el aviso dice cuántos puntos se purgarán, de qué periodo y cuánto espacio se libera

**Escenario: Purga de lo más antiguo tras el aviso y la gracia**

Dado el aviso del tope mostrado por primera vez hace 25 horas en la CLI
Cuando corre el trabajo de purga
Entonces se purgan los puntos más antiguos que no están protegidos hasta bajar al 70 % del tope
  Y en el timeline quedan como "punto purgado"
  Y el repo del usuario no cambia

**Escenario: Los puntos protegidos y los recientes no se purgan nunca**

Dado una Time Machine por encima del tope en la que solo quedan el punto previo a la última operación destructiva de cada worktree y puntos de menos de 7 días
Cuando corre el trabajo de purga
Entonces no se purga ningún punto
  Y el desarrollador ve un aviso persistente de que el tope está superado
  Y la captura por observación se detiene con un hueco "sin espacio"

**Escenario: Al llegar al tope antes de la gracia se detiene la captura, no se borra**

Dado una Time Machine en el 100 % del tope y un aviso mostrado hace 2 horas
Cuando un agente cambia un archivo del worktree "feat-login"
Entonces ese cambio no se captura y queda un hueco "sin espacio"
  Y no se purga ningún punto
  Y el snapshot previo de una operación protegida se sigue tomando con su reserva

**Esquema del escenario: El tope solo se fija en el perfil**

Dado `timeMachine.maxDiskSizeGiB` con el valor "<valor>" en el nivel "<nivel>"
Cuando el motor lee la configuración
Entonces el tope efectivo es "<efectivo>" GiB
  Y "<diagnóstico>"

Ejemplos:
| valor | nivel | efectivo | diagnóstico |
| 20 | perfil | 20 | no hay diagnóstico |
| 2 | equipo | 10 | se avisa de que la clave no se admite en ese nivel |
| 0 | perfil | 10 | se avisa de que el valor está fuera de rango |

**Escenario: La cuota de un repo se gestiona como el tope**

Dado una cuota de 5 GiB por repo, un tope de 10 GiB y un solo repo con 4,1 GiB de snapshots
Cuando corre el trabajo de purga
Entonces la CLI y la TUI avisan de que el repo pasó el 80 % de su cuota
  Y el aviso dice cuántos puntos se purgarán, de qué periodo y cuánto espacio se libera

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-022](../dev-specs/US-TMC-022-tope-disco.md)
