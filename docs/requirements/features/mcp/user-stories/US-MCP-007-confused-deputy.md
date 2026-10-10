---
id: US-MCP-007
title: "Un hook lanzado por una escritura de un agente no puede usar los poderes del desarrollador"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-09
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-002
    - ADR-GRP-005
    - ADR-GRD-007
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-002
    - US-MCP-003
    - US-MCP-008
    - US-MCP-009
ado:
  id: null
  url: null
covers: [BR-MCP-AUTH-005, BR-MCP-AUTH-004]
blocked_by: []
tags: [mcp, seguridad, confused-deputy, comando-reservado, ola-2]
---

# US-MCP-007: Un hook lanzado por una escritura de un agente no puede usar los poderes del desarrollador

## Descripción

**Como** desarrollador orquestador, **quiero** que todo proceso que nazca durante una operación pedida por un agente se atribuya a ese agente y no pueda usar comandos reservados, y que la conexión del MCP no los acepte nunca, **para** que ningún agente relaje reglas ni habilite repos a través de mis propios hooks.

**Valor**: cierra el riesgo R-MCP-1 (confused deputy). Es requisito previo de toda herramienta de escritura (Q-MCP-19, paso 2).

## Reglas cubiertas

BR-MCP-AUTH-005 (descendientes del ejecutor atribuidos al solicitante y sin comandos reservados) · BR-MCP-AUTH-004 (ninguna herramienta reservada; el canal rechaza los reservados desde la conexión del MCP) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-002 (comandos reservados de la allowlist), US-MCP-003 (conexión del MCP con el motor). La bloquean a ella todas las escrituras: US-MCP-008 a US-MCP-019.
- **Habilitadores**: TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks), ambos en propuesta, rama docs/arch-cockpit.
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe). DEP-MCP-3 (enmienda a ADR-GRP-005 § 6 y SEC-03, no creada). ADR-CKP-002 (catálogo de operaciones y ejecutor; **propuesto**, en revisión en la rama docs/arch-cockpit): fija que los descendientes del ejecutor se atribuyen al solicitante del plan. ADR-GRD-007 § 1 (el canal rechaza los reservados desde la conexión del MCP). Las tres primeras son bloqueos de arquitectura.
- **Lado del canal (resuelto, D-20)**: el rechazo `daemon-descendant` de lo reservado ya está en main (TS-GRP-004, D21) con las marcas del ejecutor (TS-TMC-004 § 7). El rechazo en preparar, ejecutar y cancelar llega con TS-CKP-002, del que esta historia ya depende. La allowlist reservada la fija ADR-GRP-005, Enmienda (2026-10-05, MCP).
- **Transversal**: el corpus de seguridad incluye el caso confused deputy; cómo se distingue al humano en un comando reservado lo define el Arquitecto (ADR-GRD-007).

## Criterios de Aceptación

**Escenario: Un hook lanzado durante la operación de un agente se atribuye a ese agente**

Dado una operación del catálogo compartido en curso, pedida por "claude-1" en "shop-feat-a"
  Y un hook "pre-commit" del usuario que se ejecuta durante esa operación
Cuando el hook lanza un cliente de GitRaptor que hace una petición
Entonces el registro de auditoría muestra esa petición a nombre de "claude-1"
  Y nunca a nombre del desarrollador

**Esquema del escenario: Ese hook no puede pedir nada que solo el desarrollador o el ejecutor pueden pedir**

Dado una operación del catálogo compartido en curso, pedida por "claude-1" en "shop-feat-a"
Cuando un hook lanzado por esa operación pide "<petición>"
Entonces la petición se rechaza en el acto, sin esperar a que termine la operación en curso
  Y nada cambia en la configuración, la allowlist, la cola ni el repo
  Y el registro de auditoría muestra el intento a nombre de "claude-1"

Ejemplos:
| petición |
| relajar una regla de Guardrails |
| una operación del catálogo, como un commit |
| cancelar la operación en curso |
| dar una confirmación pendiente |

**Esquema del escenario: La conexión del MCP no acepta comandos reservados**

Dado un agente conectado al servidor MCP en el repo "shop", en la allowlist
Cuando por esa conexión llega la petición de "<comando reservado>"
Entonces la petición se rechaza
  Y nada cambia en GitRaptor ni en el repo

Ejemplos:
| comando reservado |
| habilitar otro repo para el MCP |
| quitar un repo de la allowlist |
| añadir o retirar un repo de la observación |
| editar la configuración de Guardrails |
| decidir en la cola de confirmación |
| instalar o quitar los hooks |
| usar la excepción consciente |
| parar el motor |

**Escenario: Terminada la operación, el desarrollador conserva sus comandos reservados**

Dado la operación que pidió "claude-1" ya terminó
Cuando el desarrollador habilita el repo "shop-docs" para el MCP desde su propia terminal
Entonces el comando se completa con su confirmación
  Y el registro de auditoría no lo muestra a nombre de "claude-1"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-MCP-007](../dev-specs/US-MCP-007-dev-spec.md) (2026-10-09).
