---
id: US-MCP-002
title: "El desarrollador decide qué repos pueden usar los agentes por el MCP"
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
    - ADR-GRP-006
    - ADR-GRD-005
    - ADR-GRD-007
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-003
    - US-MCP-007
    - US-GRP-001
    - US-GRP-006
    - US-GRD-004
    - US-GRD-016
ado:
  id: null
  url: null
covers: [BR-MCP-WF-006, BR-MCP-CONS-003, BR-MCP-AUTH-004]
blocked_by: [DEP-MCP-3, DEP-MCP-4]
tags: [mcp, allowlist, opt-in, comando-reservado, ola-1]
---

# US-MCP-002: El desarrollador decide qué repos pueden usar los agentes por el MCP

## Descripción

**Como** desarrollador orquestador, **quiero** habilitar y retirar repos para el MCP con un comando que solo yo puedo ejecutar, **para** que observar un repo no lo exponga a los agentes sin mi decisión.

**Valor**: opt-in explícito; un agente nunca recibe datos ni escribe en un repo que el desarrollador no habilitó (BR-16).

## Reglas cubiertas

BR-MCP-WF-006 (opt-in, reservada, en cascada) · BR-MCP-CONS-003 (allowlist en el perfil, ⊆ observados, capa MCP activa, diagnóstico "MCP no instalado") · BR-MCP-AUTH-004 (parte: añadir y quitar de la allowlist son comandos reservados) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001 (añadir un repo a la observación), US-GRP-006 (retirarlo de la observación), US-GRD-004 (estado de protección y diagnósticos).
- **Externas**: DEP-MCP-4 (enmienda a ADR-GRP-006: almacén de la allowlist en el perfil, invariante ⊆ observados y retirada en cascada) y la mitad de DEP-MCP-3 que toca la allowlist (enmienda a ADR-GRP-005 § 6 y SEC-03: añadir y quitar son comandos reservados). Son sus bloqueos de arquitectura. No la bloquea ADR-MCP-001: la historia es CLI y perfil, sin servidor MCP (D-1).
- **Relación**: las transiciones entre los cuatro estados de protección según la allowlist son de US-GRD-016 (Guardrails), que esta historia ayuda a desbloquear. Aquí solo se comprueba que la capa MCP queda activa o inactiva.
- **Transversal**: cómo se distingue al humano en un comando reservado (transversal, lo define el Arquitecto; ADR-GRD-007).

## Criterios de Aceptación

**Escenario: El desarrollador habilita un repo observado**

Dado el repo "shop" observado por GitRaptor y fuera de la allowlist del MCP
  Y ningún agente con el servidor MCP instalado
Cuando el desarrollador habilita "shop" para el MCP con el comando reservado
Entonces "shop" figura en la allowlist del MCP de su perfil
  Y la capa MCP de "shop" queda activa
  Y el estado de protección de "shop" muestra el diagnóstico "MCP no instalado"
  Y nada cambia en el repo ni en la configuración del equipo

**Escenario: Observar un repo no lo habilita para el MCP**

Dado el desarrollador acaba de añadir el repo "shop" a la observación
Cuando consulta la allowlist del MCP
Entonces "shop" no figura en ella
  Y la capa MCP de "shop" está inactiva

**Escenario: No se puede habilitar un repo que no se observa**

Dado el repo "otro" no observado por GitRaptor
Cuando el desarrollador intenta habilitar "otro" para el MCP
Entonces el comando se rechaza con el motivo "observa el repo primero"
  Y la allowlist del MCP no cambia

**Escenario: Retirar un repo de la observación lo saca de la allowlist**

Dado el repo "shop" observado y en la allowlist del MCP
Cuando el desarrollador retira "shop" de la observación
Entonces "shop" sale también de la allowlist del MCP
  Y el desarrollador recibe el aviso "shop también salió de la allowlist del MCP"
  Y la capa MCP de "shop" queda inactiva

**Escenario: El desarrollador quita un repo de la allowlist y sigue observado**

Dado el repo "shop" observado y en la allowlist del MCP
Cuando el desarrollador quita "shop" de la allowlist con el comando reservado
Entonces "shop" ya no figura en la allowlist
  Y "shop" sigue observado por GitRaptor
  Y la capa MCP de "shop" queda inactiva

**Escenario: Un agente no puede habilitar un repo**

Dado una sesión de Claude Code trabajando en el repo "shop"
Cuando el agente intenta habilitar el repo "otro" para el MCP desde su shell
Entonces la petición se rechaza porque habilitar un repo es un comando reservado al desarrollador
  Y la allowlist del MCP no cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (comandos de la CLI; mensajes según la guía de contenido del design system, DSYS-GRP-001).
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y las enmiendas DEP-MCP-3 y DEP-MCP-4).
