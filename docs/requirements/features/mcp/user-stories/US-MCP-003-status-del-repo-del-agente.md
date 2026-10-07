---
id: US-MCP-003
title: "Un agente consulta por MCP el estado del repo en el que trabaja, y de ningún otro"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-GRP-005
    - ADR-TMC-005
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-002
    - US-MCP-004
    - US-MCP-005
    - US-MCP-006
    - US-GRP-001
    - US-GRP-007
    - TS-GRP-003
    - TS-GRP-004
    - TS-TMC-004
ado:
  id: null
  url: null
covers: [BR-MCP-CALC-001, BR-MCP-ELIG-006, BR-MCP-ELIG-001, BR-MCP-EDGE-004, BR-MCP-CONS-001, BR-MCP-TIME-003, BR-MCP-EDGE-001]
blocked_by: []
tags: [mcp, status, ambito, esqueleto-andante, ola-1]
---

# US-MCP-003: Un agente consulta por MCP el estado del repo en el que trabaja, y de ningún otro

## Descripción

**Como** desarrollador orquestador, **quiero** que el agente que trabaja en un worktree pida `status` por MCP y reciba el estado del repo donde arrancó, sin poder elegir otro, **para** que coordine su trabajo con los datos del motor y nunca vea un repo que no habilité.

**Valor**: esqueleto andante del MCP: agente → servidor → motor → respuesta, con el ámbito y la allowlist aplicados desde la primera llamada.

## Reglas cubiertas

BR-MCP-CALC-001 (ámbito por el cwd del proceso, también desde una subcarpeta; un `cd` no lo cambia) · BR-MCP-ELIG-006 (lecturas: allowlist y worktree disponible; las usa "sin atribuir") · BR-MCP-ELIG-001 (filas de allowlist y worktree disponible para las lecturas) · BR-MCP-EDGE-004 (fuera de la allowlist o de un worktree observado, sin datos) · BR-MCP-CONS-001 (cliente del motor: nada leído por su cuenta) · BR-MCP-TIME-003 (motor arrancado con la primera llamada) · BR-MCP-EDGE-001 (motor no arrancable) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-002 (allowlist). De otras features: US-GRP-001 (estado publicado del repo), US-GRP-007 (sesiones de Claude Code, para el solicitante); TS-GRP-004 (canal de clientes, arranque bajo demanda) y TS-GRP-003 (proceso del motor), de motor-local; TS-TMC-004 (solicitante resuelto en el canal), de la Time Machine. TS-TMC-004 está en draft y es de complejidad alta: es el eslabón que más puede retrasar el esqueleto andante.
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): ámbito por cwd y catálogo; bloqueo de arquitectura. El transporte stdio lo fija Q-MCP-13.
- **Transversal**: leer el cwd de otro proceso en Linux y Windows: pendiente de la etapa de validación multiplataforma (DEP-MCP-9, S-MCP-4); se verifica en macOS. Que Claude Code arranque el servidor en la carpeta del proyecto es el supuesto S-MCP-3.
- **Alcance**: aquí `status` devuelve lo mínimo para el esqueleto (repo, worktree del llamante y solicitante). El contenido completo es US-MCP-004 y los topes de respuesta, US-MCP-005.

## Criterios de Aceptación

**Escenario: El agente pide el estado desde una subcarpeta de su worktree**

Dado el repo "shop" en la allowlist del MCP
  Y una sesión de Claude Code arrancada en "/code/shop-feat-a/src", atribuida a "claude-1"
Cuando el agente pide `status`
Entonces la respuesta es del repo "shop"
  Y nombra "shop-feat-a" como el worktree en el que actuó
  Y declara el solicitante "claude-1"

**Escenario: Un cambio de directorio del agente no cambia su ámbito**

Dado una sesión MCP que arrancó en el worktree "shop-feat-a"
Cuando el agente cambia de directorio en su shell a "/code/shop-feat-b" y pide `status`
Entonces la respuesta sigue nombrando "shop-feat-a" como el worktree en el que actuó

**Esquema del escenario: Fuera de un repo habilitado, ninguna herramienta da datos**

Dado una sesión de Claude Code arrancada en "<carpeta>"
Cuando el agente pide `status`
Entonces la llamada se rechaza con el motivo "<motivo>" y la acción que lo resuelve
  Y la respuesta no contiene ninguna ruta, rama ni dato del repo

Ejemplos:
| carpeta | motivo |
| el repo "shop", observado pero fuera de la allowlist | repo no habilitado para el MCP |
| "~/descargas", que no está en ningún repo observado | esta carpeta no está en un repo observado por GitRaptor |

**Escenario: Un agente sin atribuir también puede leer**

Dado el repo "shop" en la allowlist del MCP
  Y un cliente MCP que el motor no asigna a ningún agente
Cuando el cliente pide `status`
Entonces recibe el estado del repo
  Y la respuesta declara el solicitante "sin atribuir" y la acción "regístrate para escribir"

**Escenario: El motor arranca con la primera llamada, no con la sesión**

Dado el motor de GitRaptor parado
Cuando Claude Code abre una sesión sin usar ninguna herramienta de GitRaptor
Entonces el motor sigue parado
Cuando el agente pide `status`
Entonces el motor arranca y la respuesta llega con el estado del repo

**Escenario: Si el motor no puede arrancar, el agente recibe la acción y ningún dato**

Dado el motor de GitRaptor parado y sin poder arrancar
Cuando el agente pide `status`
Entonces la llamada se rechaza con el motivo "GitRaptor no está en marcha y no se pudo arrancar" y la acción "revisa la instalación de GitRaptor"
  Y la respuesta no contiene ningún dato leído del repo por otra vía que el motor

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (respuesta de herramienta; mensajes según la guía de contenido del design system, DSYS-GRP-001).
- **Dev Spec:** [DS-US-MCP-003](../dev-specs/US-MCP-003-dev-spec.md).
