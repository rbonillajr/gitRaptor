---
id: US-MCP-022
title: "Configurar y retirar GitRaptor en un repo deja intacto lo que escribió el equipo"
type: us
status: draft
priority: medium
created: 2026-11-10
updated: 2026-11-10
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
    - US-MCP-001
    - US-MCP-020
    - US-MCP-021
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-007, BR-MCP-WF-008]
blocked_by: []
tags: [mcp, agentes, configure-agents, nfr-01, reversion, superficie-agentes, hito-m4]
---

# US-MCP-022: Configurar y retirar GitRaptor en un repo deja intacto lo que escribió el equipo

## Descripción

**Como** desarrollador orquestador, **quiero** que GitRaptor escriba en `CLAUDE.md` o `AGENTS.md` solo dentro de su propio bloque, que no pise lo que alguien editó a mano y que al retirar la configuración el archivo quede como estaba, **para** poder probar GitRaptor en un repo del equipo sin arriesgar las instrucciones que el equipo ya escribió.

**Valor**: los archivos de instrucciones del agente son del equipo y están versionados. Cero pérdida de datos (NFR-01) aplica también a ellos: si configurar o retirar puede borrar una línea, nadie lo prueba en un repo real.

**Prioridad e hito**: Should, M4 (v0.1.0). Sale junto con US-MCP-021. Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32). Archivo del bloque: P-MCP-34.

## Reglas cubiertas

BR-MCP-CONS-007 (solo dentro del bloque, bloque editado a mano = conflicto, enlace o fuera del repo = no se toca, retirar deja el archivo como estaba) · BR-MCP-WF-008 (parte: retirar quita guía y bloque del repo y no toca el registro del MCP) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-021 (el comando y su plan; salen juntas) y US-MCP-020 (la guía que se retira).
- **Relación**: US-MCP-001 (`raptor mcp uninstall` retira el registro del MCP, que es de ámbito de usuario).

## Criterios de Aceptación

**Esquema del escenario: El bloque va al archivo de instrucciones que el agente lee**

Dado el repo "shop" con <archivos>
Cuando el desarrollador configura "shop" para Claude Code
Entonces el bloque de GitRaptor queda en "<destino>"

Ejemplos:
| archivos | destino |
| AGENTS.md y un CLAUDE.md que importa AGENTS.md | AGENTS.md |
| solo CLAUDE.md | CLAUDE.md |
| AGENTS.md y un CLAUDE.md que no lo importa | CLAUDE.md |
| ninguno de los dos | CLAUDE.md, creado por GitRaptor |

**Escenario: Lo del equipo no cambia al configurar**

Dado "CLAUDE.md" de "shop" con 40 líneas escritas por el equipo
Cuando el desarrollador configura "shop" para Claude Code
Entonces "CLAUDE.md" contiene las mismas 40 líneas, byte a byte, y el bloque de GitRaptor al final

**Escenario: Un bloque editado a mano no se pisa ni se borra**

Dado el bloque de GitRaptor en "CLAUDE.md" de "shop" editado a mano después de la última configuración
Cuando el desarrollador configura de nuevo "shop" o retira su configuración
Entonces no se escribe ni se borra nada
  Y ve que el bloque fue editado a mano y que debe decidir si lo conserva o lo reemplaza

**Escenario: Un archivo que es un enlace o apunta fuera del repo no se toca**

Dado "CLAUDE.md" de "shop" es un enlace a un archivo fuera de "shop"
Cuando el desarrollador configura "shop" para Claude Code
Entonces el paso del bloque queda en conflicto con ese motivo
  Y no se escribe nada en el repo ni en el registro del MCP

**Escenario: Retirar deja el repo como estaba antes de configurar**

Dado "shop" configurado, con "CLAUDE.md" que ya existía con 40 líneas del equipo
Cuando el desarrollador retira la configuración de agentes de "shop"
Entonces "CLAUDE.md" vuelve a tener las mismas 40 líneas, byte a byte
  Y la guía de GitRaptor ya no está en "shop"
  Y si "CLAUDE.md" lo había creado GitRaptor, el archivo ya no existe

**Escenario: Retirar en un repo no toca el registro del MCP de los demás**

Dado "shop" y "api" configurados y el servidor "gitraptor" registrado para el usuario
Cuando el desarrollador retira la configuración de agentes de "shop"
Entonces el servidor "gitraptor" sigue registrado y "api" sigue configurado
  Y el mensaje indica que el registro se retira con `raptor mcp uninstall`

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), puntos C2 y F.

- **Marca**: el bloque lleva la versión de la guía y el hash de su contenido. Si el hash no coincide, hubo edición a mano y hay conflicto sin escribir. "Creado por GitRaptor" = el archivo solo contiene el bloque.
- **Escritura**: la hace el daemon dentro del método reservado de US-MCP-021, con huella comprobada bajo el cerrojo, temporal y renombrado atómico, y verificación de que los bytes fuera del bloque son idénticos. No sigue enlaces.
- **Puerta**: prueba de propiedades en CI con archivos generados: tras configurar y retirar, el archivo es idéntico byte a byte o no existe (Validación 6 de la enmienda).

## Diseño y Dev Spec

- **Diseño:** no aplica (CLI; mensajes en/es según la guía de contenido del design system).
- **Dev Spec:** **sí, conjunta con US-MCP-021** (decisión del Arquitecto, 2026-11-10). Pendiente: `../dev-specs/US-MCP-021-dev-spec.md`.
