---
id: US-MCP-001
title: "El desarrollador conecta Claude Code a GitRaptor con un solo comando y lo retira igual de fácil"
type: us
status: implemented
priority: high
created: 2026-10-04
updated: 2026-10-08
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-002
    - US-MCP-003
ado:
  id: null
  url: null
covers: [BR-MCP-WF-007, BR-MCP-EDGE-009]
blocked_by: []
tags: [mcp, instalacion, claude-code, ola-1]
---

# US-MCP-001: El desarrollador conecta Claude Code a GitRaptor con un solo comando y lo retira igual de fácil

## Descripción

**Como** desarrollador orquestador, **quiero** registrar y retirar el servidor MCP de GitRaptor en Claude Code con un comando, **para** que todas mis sesiones de Claude Code tengan las herramientas seguras sin editar a mano la configuración del agente.

**Valor**: instalación en un paso (BR-15, parcial por D2: solo Claude Code) y dogfooding desde la primera entrega.

## Reglas cubiertas

BR-MCP-WF-007 (instalar y desinstalar: ámbito de usuario, idempotente, muestra el cambio, no toca otros servidores, binario instalado) · BR-MCP-EDGE-009 (servidor ajeno con el mismo nombre, agente no soportado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: ninguna de esta feature. Se prueba sin ningún repo habilitado; las herramientas llegan con US-MCP-003 y el opt-in de repos con US-MCP-002.
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): fija la instalación y el transporte; bloqueo de arquitectura. CLI de Claude Code en su forma actual (S-MCP-5, R-MCP-6). Regla de "binario instalado" (SEC-14).
- **Transversal**: comportamiento de la CLI `claude` en Linux y Windows: pendiente de la etapa de validación multiplataforma (S-MCP-4, DEP-MCP-9); en el MVP se verifica en macOS.

## Criterios de Aceptación

**Escenario: El desarrollador instala el servidor en Claude Code**

Dado GitRaptor instalado en la máquina y Claude Code sin ningún servidor llamado "gitraptor"
Cuando el desarrollador ejecuta `raptor mcp install`
Entonces GitRaptor le informa qué va a cambiar antes de cambiarlo
  Y Claude Code tiene registrado el servidor "gitraptor" con ámbito de usuario, apuntando al binario instalado de GitRaptor
  Y los demás servidores MCP del usuario siguen exactamente igual
  Y ningún archivo de ningún repo cambia
  Y ningún repo queda añadido a la allowlist del MCP

**Escenario: Instalar dos veces no cambia nada**

Dado el servidor "gitraptor" de este GitRaptor ya registrado en Claude Code
Cuando el desarrollador vuelve a ejecutar `raptor mcp install`
Entonces GitRaptor responde que ya está instalado y que no hay nada que hacer
  Y la configuración de Claude Code no cambia

**Escenario: El desarrollador retira el servidor**

Dado el servidor "gitraptor" de este GitRaptor registrado en Claude Code junto a otros dos servidores
Cuando el desarrollador ejecuta `raptor mcp uninstall`
Entonces "gitraptor" deja de estar registrado en Claude Code
  Y los otros dos servidores siguen registrados sin cambios

**Esquema del escenario: La instalación no cambia nada cuando no puede hacerlo de forma segura**

Dado "<situación>"
Cuando el desarrollador ejecuta `raptor mcp install`
Entonces la instalación se rechaza con el motivo "<motivo>" y la acción que lo resuelve
  Y la configuración de Claude Code no cambia

Ejemplos:
| situación | motivo |
| el agente pedido es Cursor | Cursor no está soportado todavía |
| el agente pedido es Codex | Codex no está soportado todavía |
| el agente pedido es Copilot | Copilot no está soportado todavía |
| GitRaptor se ejecuta desde una caché de npx o una carpeta temporal | instala GitRaptor primero |
| Claude Code ya tiene un servidor "gitraptor" que apunta a otro binario | ya existe un servidor gitraptor que no es de este GitRaptor |

**Escenario: Sin la CLI de Claude Code, el desarrollador recibe el comando exacto**

Dado una máquina donde la CLI de Claude Code no está disponible
Cuando el desarrollador ejecuta `raptor mcp install`
Entonces GitRaptor le muestra el comando exacto para registrar el servidor a mano
  Y no escribe en ningún archivo de configuración del agente

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (sin superficie visual; los mensajes siguen la guía de contenido del design system, DSYS-GRP-001).
- **Dev Spec:** [DS-US-MCP-001](../dev-specs/US-MCP-001-dev-spec.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #70.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- La parte de `raptor doctor` (SEC-MCP-10) espera a la historia que cree `doctor`; `cargo-deny` sobre `rmcp` (SEC-07) sigue pendiente.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
