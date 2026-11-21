---
id: US-GRP-024
title: "Un agente no puede llenar el disco ni la flota creando worktrees"
type: us
status: ready
priority: high
milestone: M3
created: 2026-11-21
updated: 2026-11-21
domain: GRP
epic: E-001
feature: mcp
source: inline
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-002
  context:
    - CTX-MCP-001
  specs:
    - DS-US-MCP-019
    - DS-US-GRP-024
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-019
    - US-MCP-005
    - US-MCP-020
    - US-WTL-001
    - US-CKP-018
  technical:
    - TS-GRP-010
ado:
  id: null
  url: null
covers: [BR-MCP-ELIG-004]
blocked_by: [US-MCP-019]
tags: [mcp, create-worktree, seguridad, nfr-01, hito-m3]
---

# US-GRP-024: Un agente no puede llenar el disco ni la flota creando worktrees

## Descripción

**Como** desarrollador orquestador, **quiero** que cada sesión de agente tenga un tope de worktrees creados en un repo, que el repo tenga un techo de worktrees de agentes que no se pierde al reiniciar GitRaptor y que ningún agente cree un worktree con poco espacio libre en el disco, **para** que un agente que entra en bucle o recibe una inyección de prompt no llene el disco y deje sin espacio el historial de operaciones y los snapshots de los que depende el undo (NFR-01).

**Valor**: sin tope, un agente que llama a `create_worktree` en bucle llega a unos 1200 checkouts por hora (security-review de DS-US-MCP-019, hallazgo H-01, Alto). Con el disco lleno falla la captura de snapshots y el registro de operaciones, y se pierde la garantía de recuperar el trabajo de todos los agentes del repo, no solo del que falla. El desarrollador sigue creando los worktrees que quiera desde GitRaptor.

## Reglas cubiertas

BR-MCP-ELIG-004 (`create_worktree`: parte del tope, el techo y el suelo de espacio para un agente) — ver [business-rules.md](../business-rules.md). La redacción de la regla la mantiene el coordinador; los valores vienen de DS-US-MCP-019 D14 y D18.

## Dependencias

- **Bloqueos (`blocked_by: [US-MCP-019]`)**: esta historia extiende la operación `create-worktree` que construye US-MCP-019; sin ella no hay nada que limitar.
- **Condición de salida de M3, sin ciclo (decisión del PO, 2026-11-21)**: el tope es condición de salida de M3 (security-review H-01, DS-US-MCP-019 G9). No se pone `blocked_by: [US-GRP-024]` en US-MCP-019, porque crearía un ciclo con el bloqueo de arriba: US-MCP-019 entra primero y esta historia después. La exigencia queda en el [criterio 12 de salida de M3](../../../release-plan.md#criterio-de-salida-2) y en la nota de dependencias de US-MCP-019: M3 no cierra sin esta historia en `main`.
- **Lo que ya construye US-MCP-019**: el suelo de espacio libre y el techo por repo (DS-US-MCP-019 D18, T004 paso 2.3b) entran con la operación. Esta historia añade el tope por sesión de agente (D14) y certifica los tres límites juntos de punta a punta.
- **Perfil del solicitante (TS-GRP-010, implementada)**: un agente recibe el perfil `mcp` también por la CLI o hablando directo con el motor, así que el tope le aplica por cualquier puerta.
- **Capa del desarrollador (decisión del orquestador, 2026-11-21, validada por PO)**: el humano no tiene tope por sesión y sus worktrees no cuentan en el techo de agentes; sí recibe el rechazo por suelo de espacio, porque NFR-01 no distingue quién llena el disco. La regla de cockpit correspondiente (junto a BR-CKP-ELIG-005) y un escenario de poco espacio en US-CKP-018 y US-WTL-001 lo recogen.
- **Relacionadas que no bloquean**: US-MCP-020 (la guía para agentes explica estos rechazos), US-WTL-001 y US-CKP-018 (crean worktrees desde la capa del desarrollador, que no tiene tope de agentes).

## Criterios de Aceptación

**Escenario: El agente crea worktrees mientras no llega al tope de su sesión**

Dado "claude-1" atribuido en "shop-feat-a", con 7 worktrees creados en el repo "shop" desde su sesión que siguen existiendo
  Y el tope por sesión es 8, el valor por defecto
  Y el disco del worktree nuevo tiene espacio libre por encima del suelo
Cuando "claude-1" pide `create_worktree` con la rama "feat/octava"
Entonces existe la rama "feat/octava" con su worktree
  Y la sesión de "claude-1" tiene 8 worktrees en el repo "shop"

**Escenario: La sesión que llega a 8 worktrees no crea el noveno**

Dado "claude-1" con 8 worktrees creados en el repo "shop" desde su sesión que siguen existiendo
Cuando "claude-1" pide `create_worktree` con la rama "feat/novena"
Entonces la petición se rechaza con el código `worktree-limit`, el motivo "Esta sesión ya creó el máximo de worktrees permitido en este repo." y la acción "Pide al desarrollador que borre alguno o que suba worktree.agentLimit."
  Y no se crea ninguna rama ni worktree

**Escenario: El mismo agente por el MCP y por la CLI suma en un solo tope**

Dado "claude-1" con 5 worktrees creados en el repo "shop" por `create_worktree` y 3 creados con la CLI `raptor` desde su sesión, todos existentes
Cuando "claude-1" pide crear otro worktree, por el MCP o por la CLI
Entonces la petición se rechaza con el código `worktree-limit`
  Y no se crea ninguna rama ni worktree

**Escenario: Al borrar un worktree la sesión recupera un hueco**

Dado "claude-1" con 8 worktrees creados en el repo "shop" desde su sesión
  Y el desarrollador borra uno de ellos
Cuando "claude-1" pide `create_worktree` con la rama "feat/novena"
Entonces existe la rama "feat/novena" con su worktree

**Escenario: El techo del repo se mantiene tras reiniciar GitRaptor**

Dado el repo "shop" con 32 worktrees creados por agentes de varias sesiones que siguen existiendo, el techo por defecto
  Y GitRaptor se ha reiniciado
Cuando "claude-9", con 0 worktrees creados en su sesión, pide `create_worktree` con la rama "feat/otra"
Entonces la petición se rechaza con el código `repo-worktree-limit`, el motivo "Este repo ya tiene el máximo de worktrees creados por agentes." y la acción "Pide al desarrollador que borre alguno o que suba worktree.agentRepoLimit."
  Y no se crea ninguna rama ni worktree

**Escenario: Sin espacio libre suficiente un agente no crea el worktree**

Dado un volumen de 200 GiB para los worktrees del repo "shop", con 4 GiB libres, por debajo del suelo de 10 GiB (el mayor de 5 GiB y el 5 % del volumen)
Cuando "claude-1", con 0 worktrees en su sesión, pide `create_worktree` con la rama "feat/pagos"
Entonces la petición se rechaza con el código `no-free-space`, el motivo "Queda poco espacio libre en el disco del worktree nuevo." y la acción "Avisa al desarrollador: libera espacio en ese disco."
  Y no se crea ninguna rama ni worktree

**Escenario: El espacio se agota entre la preparación y la creación**

Dado "claude-1" pide `create_worktree` con la rama "feat/pagos" y la preparación ve espacio libre por encima del suelo
  Y otro proceso deja el disco por debajo del suelo antes de que se cree el worktree
Cuando GitRaptor vuelve a comprobar el espacio justo antes de crearlo
Entonces la petición se rechaza con el código `no-free-space`
  Y no se crea ninguna rama ni worktree

**Esquema del escenario: El desarrollador ajusta el tope por sesión**

Dado el valor "<valor>" para `worktree.agentLimit` en "<nivel>"
Cuando "claude-1" pide `create_worktree` en el repo "shop"
Entonces el tope de su sesión es "<tope efectivo>"
  Y "<diagnóstico>"

Ejemplos:
| valor | nivel | tope efectivo | diagnóstico |
| 3 | el perfil del desarrollador | 3 | no hay diagnóstico |
| 64 | la configuración local personal | 64 | no hay diagnóstico |
| 0 | el perfil del desarrollador | 8 | un diagnóstico avisa del valor fuera de rango (1 a 64) |
| 65 | la configuración local personal | 8 | un diagnóstico avisa del valor fuera de rango (1 a 64) |
| 2 | la configuración del equipo | 8 | un diagnóstico avisa de que ese nivel no se tiene en cuenta |

**Escenario: Los worktrees del desarrollador no suman al techo de agentes**

Dado el repo "shop" con 31 worktrees de agentes y 10 worktrees creados por el desarrollador
Cuando "claude-9", con 0 worktrees en su sesión, pide `create_worktree` con la rama "feat/otra"
Entonces existe la rama "feat/otra" con su worktree, el número 32 de los agentes

**Escenario: Un worktree cuenta aunque la sesión que lo creó ya cerró**

Dado "claude-1" creó 8 worktrees en el repo "shop" y su sesión cerró, sin reiniciar GitRaptor
  Y los 8 worktrees siguen existiendo
Cuando el repo ya tiene 32 worktrees de agentes y "claude-9" pide `create_worktree` con la rama "feat/otra"
Entonces la petición se rechaza con el código `repo-worktree-limit`

**Escenario: El desarrollador tampoco crea con poco espacio libre**

Dado un volumen de 200 GiB con 4 GiB libres, por debajo del suelo de 10 GiB
Cuando el desarrollador crea un worktree desde la TUI o con `raptor new`
Entonces se rechaza con el código `no-free-space` y la acción "libera espacio en ese disco"
  Y no se crea ninguna rama ni worktree
  Y no hay forma de saltarse el suelo en el MVP

**Escenario: El desarrollador no queda limitado por el tope de agentes**

Dado el repo "shop" con 32 worktrees de agentes y "claude-1" con 8 en su sesión
Cuando el desarrollador crea un worktree desde la TUI o con `raptor new`
Entonces el worktree se crea
  Y no se aplican el tope por sesión ni el techo por repo de los agentes
  Y el worktree del desarrollador no cuenta en el techo de 32 por repo

## Criterios de Certificación

- Validar que el 100 % de las peticiones de un agente que superan el tope por sesión, el techo por repo o el suelo de espacio se rechazan con su código (`worktree-limit`, `repo-worktree-limit` o `no-free-space`) sin crear rama ni worktree, por el MCP y por la CLI, y que el techo por repo sigue igual tras reiniciar GitRaptor · *(método: prueba automatizada con repos temporales y suelo de espacio simulado)*.

## Requisitos Técnicos

Arquitecto, 2026-11-21: [DS-US-GRP-024](../dev-specs/US-GRP-024-dev-spec.md). La mayor parte del contrato ya está en [DS-US-MCP-019](../dev-specs/US-MCP-019-dev-spec.md): D14 (tope por sesión, `worktree.agentLimit`), D18 y T004 paso 2.3b (suelo y techo por repo), G9 y sus pruebas de contrato. El Arquitecto decide si basta esa DS o si esta historia lleva Dev Spec propia.

## Diseño y Dev Spec

- **Diseño:** no aplica (los textos de rechazo vienen de DS-US-MCP-019).
- **Dev Spec:** [DS-US-GRP-024](../dev-specs/US-GRP-024-dev-spec.md) (`spec-ready`, 2026-11-21); base en [DS-US-MCP-019](../dev-specs/US-MCP-019-dev-spec.md) D14, D18 y T004.
