---
id: US-MCP-021
title: "El desarrollador deja un repo listo para Claude Code con un comando que enseña antes lo que va a cambiar"
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
    - ADR-GRD-007
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-001
    - US-MCP-002
    - US-MCP-020
    - US-MCP-022
ado:
  id: null
  url: null
covers: [BR-MCP-WF-008]
blocked_by: []
tags: [mcp, agentes, configure-agents, instalacion, comando-reservado, superficie-agentes, hito-m4]
---

# US-MCP-021: El desarrollador deja un repo listo para Claude Code con un comando que enseña antes lo que va a cambiar

## Descripción

**Como** desarrollador orquestador, **quiero** que `raptor configure-agents` registre el servidor MCP, deje la guía para agentes y añada el bloque de instrucciones en un repo, mostrándome antes el plan paso a paso y sin repetir lo que ya está hecho, **para** preparar un repo para mis agentes en un solo paso sin descubrir después qué tocó en mis archivos.

**Valor**: hoy el desarrollador registra el MCP con `raptor mcp install` y explica GitRaptor a mano a cada agente. Con un comando idempotente y un plan visible, preparar un repo nuevo o revisar uno ya preparado es seguro de repetir.

**Prioridad e hito**: Should, M4 (v0.1.0). Sale junto con US-MCP-022 (NFR-01: escribir en archivos del usuario exige poder revertirlo). Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32). Solo Claude Code (D2).

## Reglas cubiertas

BR-MCP-WF-008 (plan visible con estado por paso, confirmación o `--yes`, `--dry-run`, idempotencia, conflicto sin escritura, sin CLI `claude` imprime el comando, no habilita la allowlist, solo Claude Code) — ver [business-rules.md](../business-rules.md). Rechazo a un agente: decisión P-MCP-35 del 2026-11-10.

## Dependencias

- **Historias**: US-MCP-001 (registro del MCP con las reglas de `raptor mcp install`), US-MCP-020 (la guía que se instala) y US-MCP-022 (cómo se escribe y se retira el bloque en los archivos del usuario; salen juntas).
- **Relación**: US-MCP-002 (la allowlist sigue siendo un comando reservado aparte; esta historia solo avisa).
- **Transversal (lo define el Arquitecto)**: cómo se distingue al humano de un agente al lanzar el comando (ADR-GRD-007, como en los comandos reservados).

## Criterios de Aceptación

**Escenario: La primera configuración enseña el plan y lo aplica al confirmar**

Dado el repo "shop" sin guía, sin bloque de GitRaptor y con el servidor "gitraptor" sin registrar
  Y "shop" no está habilitado para el MCP
Cuando el desarrollador ejecuta `raptor configure-agents` en "shop" y confirma
Entonces antes de escribir ve el plan con tres pasos en estado "se hará": registro del MCP, guía y bloque de instrucciones
  Y el plan avisa de que "shop" no está habilitado para el MCP y da el comando reservado para habilitarlo
  Y al terminar los tres pasos están hechos y "shop" sigue sin habilitar

**Escenario: Repetir el comando no cambia nada**

Dado el repo "shop" ya configurado para Claude Code
Cuando el desarrollador ejecuta `raptor configure-agents` en "shop"
Entonces ve "shop ya está configurado para Claude Code; nada que hacer"
  Y ningún archivo del repo ni el registro del MCP cambian

**Escenario: Con --dry-run solo se ve el plan**

Dado el repo "shop" sin configurar
Cuando el desarrollador ejecuta `raptor configure-agents --dry-run` en "shop"
Entonces ve el plan con el estado de cada paso
  Y no se escribe nada en el repo ni en el registro del MCP

**Escenario: Sin la CLI de Claude Code se hacen los pasos del repo y se imprime el registro**

Dado el repo "shop" sin configurar y sin la CLI `claude` disponible
Cuando el desarrollador ejecuta `raptor configure-agents --yes` en "shop"
Entonces la guía y el bloque quedan hechos
  Y el paso del registro queda en estado "manual" con el comando exacto para registrar el servidor "gitraptor"

**Escenario: Un agente no puede configurarse a sí mismo**

Dado una sesión de Claude Code trabajando en "shop"
Cuando el agente ejecuta `raptor configure-agents` desde su shell
Entonces la petición se rechaza porque configurar agentes es un comando reservado al desarrollador
  Y no se escribe nada en el repo ni en el registro del MCP

**Escenario: Otro agente no está soportado todavía**

Dado el repo "shop" sin configurar
Cuando el desarrollador pide configurar "shop" para Codex
Entonces ve "Codex no está soportado todavía; hoy solo Claude Code"
  Y no se escribe nada

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), puntos C2 y F.

- **Reservado**: la aplicación es un comando reservado (ADR-GRP-005 § 6: controles 1 a 3, `daemon-descendant`, auditoría, fuera del MCP). Es una fila nueva de ADR-GRD-007 § 1 que no relaja, sin ventana D5. El plan y `--dry-run` son una lectura sin marca MCP.
- **Quién escribe**: guía y bloque, el daemon, como transacción (huella con `(dev, inode)` y hash, temporal y renombrado atómico, auditoría), sin shell y sin Git. El registro del MCP, la CLI, solo después de que el daemon acepte, reutilizando ADR-MCP-001 § 8 tal cual (ruta absoluta, argv fijo, `claude mcp get`).
- **Idempotencia**: por la marca autodescriptiva de F (versión y hash), no por un diario del perfil.
- **Instalador único**: `raptor new` ("Ciclo de vida del worktree") reutiliza este módulo.

## Diseño y Dev Spec

- **Diseño:** no aplica (CLI; el plan y los mensajes en/es siguen la guía de contenido del design system).
- **Dev Spec:** **sí, conjunta con US-MCP-022** (decisión del Arquitecto, 2026-11-10). Pendiente: `../dev-specs/US-MCP-021-dev-spec.md`, que cubre las dos.
