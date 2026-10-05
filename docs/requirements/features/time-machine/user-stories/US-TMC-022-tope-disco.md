---
id: US-TMC-022
title: "La Time Machine nunca pasa del tope de disco que fijé"
type: us
status: draft
priority: medium
created: 2026-10-05
updated: 2026-10-05
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
covers: [BR-TMC-TIME-001, D-TMC-15, D-TMC-25]
blocked_by: []
tags: [time-machine, retencion, disco, tope, recursos]
---

# US-TMC-022: La Time Machine nunca pasa del tope de disco que fijé

## Descripción

**Como** desarrollador orquestador con un disco limitado
**Quiero** fijar un tope de disco para la Time Machine y que, al acercarse, se purguen con aviso los puntos más antiguos que no estén protegidos
**Para** usar la Time Machine sin miedo a que llene mi disco, sin perder nunca el punto anterior a una operación destructiva

**Valor**: el disco de la Time Machine queda acotado (RES-09), no solo por antigüedad, y sigue sin perderse nada sin aviso (NFR-01).

> **Origen**: decisión de Rene Bonilla (2026-10-05): tope configurable que purga lo más antiguo sin tocar los snapshots protegidos y avisa. Amplía D-TMC-15 y BR-TMC-TIME-001 con la purga por tamaño (ADR-TMC-007, Enmienda 2026-10-05). Prioridad: decisión del orquestador (2026-10-05), validada por el PO: **Should, fuera de M1**; sube a M1 si US-GRP-017 mide durante el dogfooding un almacén por encima de 2 GiB.

## Reglas cubiertas

BR-TMC-TIME-001 (ampliada: purga por tamaño con aviso visto y 24 h; nunca puntos protegidos) · D-TMC-15 (ampliada), D-TMC-25 · RES-09 — ver [business-rules.md](../business-rules.md)

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
