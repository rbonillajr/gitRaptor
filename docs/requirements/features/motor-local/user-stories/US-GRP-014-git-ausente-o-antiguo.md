---
id: US-GRP-014
title: "El desarrollador sabe qué Git le falta y el motor empieza solo cuando lo instala"
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
    - US-GRP-001
    - US-GRP-002
    - US-GRP-005
    - US-GRP-009
    - US-GRP-015
tags:
  - motor-local
  - primer-uso
  - requisitos-entorno
---

# US-GRP-014: El desarrollador sabe qué Git le falta y el motor empieza solo cuando lo instala

## Descripción

**Como** desarrollador que estrena máquina, **quiero** que sin Git 2.38 o superior el motor me diga qué falta y cómo resolverlo, espere sin observar y arranque solo cuando instale o actualice Git, **para** no confundir una máquina a medio preparar con una herramienta rota.

**Valor**: el primer uso nunca falla en silencio y el motor nunca toca la máquina para arreglarlo.

## Reglas cubiertas

BR-VAL-003 · BR-WF-002 (estado "Esperando Git"; supuestos S18 y S19 aceptados) · BR-EDGE-005 (la espera es un hueco "sin atribuir") — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001, US-GRP-002; US-GRP-015 (en serie: 015 fija los estados "Sin repos" y "Observando" de BR-WF-002 y esta historia añade "Esperando Git" encima; el contrato compartido lo fija la Dev Spec); US-GRP-005 (lo ocurrido mientras espera es un hueco "sin atribuir") y US-GRP-009 (agente registrado del último escenario).
- **Externas**: el Cockpit (F-001-02) y la CLI presentan el aviso; esta historia expone el estado y lo que falta.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Esquema del escenario: Con Git ausente o antiguo el motor espera y no observa nada**

Dado una máquina con "<git>" y el repo "demo" anotado en el perfil
Cuando arranca el motor
Entonces el estado del motor es "Esperando Git"
  Y expone "<falta>", la versión mínima 2.38 y cómo resolverlo
  Y no observa ningún repo, tampoco "demo"
  Y la versión de Git instalada en la máquina no cambia

Ejemplos:
| git | falta |
| Git no instalado | que no encuentra Git |
| Git 2.34 | que encontró la versión 2.34 |

**Escenario: Git instalado después pone el motor a observar solo**

Dado el motor en "Esperando Git" por falta de Git y el repo "demo" anotado en el perfil
Cuando el desarrollador instala Git 2.45
Entonces el motor pasa a "Observando" sin reinstalar ni reconfigurar GitRaptor
  Y empieza a observar "demo"

**Escenario: Git actualizado pone el motor a observar solo**

Dado el motor en "Esperando Git" con Git 2.34 y ningún repo en el perfil
Cuando el desarrollador actualiza Git a 2.45
Entonces el motor pasa a "Sin repos" sin reinstalar ni reconfigurar GitRaptor

**Escenario: No se añaden repos mientras falta Git**

Dado el motor en "Esperando Git"
Cuando el desarrollador intenta añadir el repo "demo"
Entonces el motor rechaza la petición con el aviso de Git
  Y la lista de repos observados no cambia

**Escenario: Si Git deja de cumplir mientras observa, el motor vuelve a esperar**

Dado el motor en "Observando" el repo "demo" con Git 2.45
  Y "otro agente: Codex" registrado en "feat-login"
Cuando Git deja de estar disponible para el motor, o el disponible pasa a ser la versión 2.34
Entonces el motor pasa a "Esperando Git" con el aviso correspondiente
Cuando se hace un commit en "feat-login" mientras el motor espera
  Y Git vuelve a cumplir el requisito
Entonces el motor vuelve a observar "demo"
  Y ese commit figura como "sin atribuir", no como de "otro agente: Codex"
  Y "otro agente: Codex" sigue registrado en "feat-login"
