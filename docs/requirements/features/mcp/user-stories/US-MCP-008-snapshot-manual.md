---
id: US-MCP-008
title: "Un agente guarda por MCP un punto de recuperación antes de un cambio arriesgado"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-08
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-002
    - ADR-TMC-004
    - ADR-TMC-007
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-009
    - US-MCP-005
    - US-MCP-006
    - US-MCP-007
    - US-TMC-001
    - US-TMC-016
    - TS-TMC-004
ado:
  id: null
  url: null
covers: [BR-MCP-ELIG-005, BR-MCP-VAL-004, BR-MCP-TIME-001, BR-MCP-AUTH-002, BR-MCP-EDGE-008]
blocked_by: []
tags: [mcp, snapshot, time-machine, cuota, ola-3]
---

# US-MCP-008: Un agente guarda por MCP un punto de recuperación antes de un cambio arriesgado

## Descripción

**Como** desarrollador orquestador, **quiero** que un agente atribuido tome un snapshot manual de su worktree con una etiqueta corta, sujeto a una cuota y un límite propios, **para** que pueda guardar un punto antes de un cambio arriesgado sin poder llenar el disco con snapshots.

**Valor**: la red de seguridad de la Time Machine al alcance del agente (BR-14), sin abrir un vector de agotamiento de disco (SEC-TMC-12).

## Reglas cubiertas

BR-MCP-ELIG-005 (parte: `snapshot` manual con cuota y rate limit propios) · BR-MCP-VAL-004 (parte: etiqueta de snapshot) · BR-MCP-TIME-001 (parte: cuota y rate limit de snapshots manuales) · BR-MCP-AUTH-002 ("sin atribuir" no escribe) · BR-MCP-EDGE-008 (parte: con HEAD separado se permite; con operación en curso, no) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-005 (respuestas y errores), US-MCP-006 (registro), US-MCP-007 (confused deputy cerrado), US-MCP-009 hereda el flujo de escritura que fija esta historia (D-3, invertida el 2026-10-08). De la Time Machine: US-TMC-001 (operación protegida), US-TMC-016 (retención), TS-TMC-004 (comando de snapshot en el canal).
- **Habilitadores**: TS-CKP-002 (catálogo y ejecutor) y TS-CKP-003 (decisión única heredada por los hooks), en propuesta, rama docs/arch-cockpit.
- **Dueña de la operación del catálogo**: esta historia es dueña de `snapshot`.
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe). ADR-CKP-002 (**propuesto**, en docs/arch-cockpit): `snapshot` → operación `snapshot` del catálogo; DEP-MCP-2 y DEP-MCP-5 se resuelven vía ADR-CKP-002. `snapshot` **no es una operación gobernada** (D-17, BR-MCP-001 v0.3): no hay escenario de decisión de Guardrails. La **API de captura manual** (nivel `manual`, etiqueta, cuota y rate limit) la fija ADR-TMC-004, Enmienda (2026-10-05, MCP) (D-21); con la cuota llena se rechaza y nunca se borra un snapshot manual. Bloqueo de arquitectura que queda: ADR-CKP-002 vía TS-CKP-002.
- **Límites por solicitante** (S-03, estrechado): la cuota durable del oplog (por solicitante y worktree, con techos por worktree y por repo) lo cubre aquí. El cupo de rate limit compartido entre las conexiones del mismo solicitante y el tope de ≤ 8 conexiones son de US-MCP-009 (Enmienda (2026-10-08, US-MCP-008) de ADR-MCP-001).
- **Transversal**: cifras de ADR-MCP-001 § 6 (D-22): ≤ 20 snapshots por solicitante y worktree en una ventana de 24 h, 5 por minuto y la cuota de disco general por encima.

## Criterios de Aceptación

**Escenario: El agente toma un snapshot con etiqueta**

Dado el repo "shop" en la allowlist y "claude-1" atribuido en "shop-feat-a"
Cuando "claude-1" pide `snapshot` con la etiqueta "antes de migrar"
Entonces existe un snapshot manual de "shop-feat-a" con esa etiqueta
  Y en el timeline figura con actor "claude-1" y canal "mcp"
  Y la respuesta nombra el worktree y el id del snapshot, con la etiqueta marcada como dato no confiable

**Escenario: Con HEAD separado el snapshot se permite**

Dado "shop-feat-a" con HEAD separado y "claude-1" atribuido
Cuando "claude-1" pide `snapshot`
Entonces existe un snapshot manual de "shop-feat-a"

**Escenario: Con un rebase a medias el snapshot se rechaza**

Dado "shop-feat-a" con un rebase de Git a medias
Cuando "claude-1" pide `snapshot`
Entonces la petición se rechaza con el motivo "operación en curso" y la acción que la resuelve
  Y no se crea ningún snapshot

**Esquema del escenario: Una etiqueta no válida no crea nada**

Dado "claude-1" atribuido en "shop-feat-a"
Cuando pide `snapshot` con "<etiqueta>"
Entonces la petición se rechaza con el motivo "texto no válido"
  Y no se crea ningún snapshot

Ejemplos:
| etiqueta |
| una etiqueta que supera el tope de longitud |
| una etiqueta con caracteres de control |
| una etiqueta vacía |

**Esquema del escenario: Un agente en bucle choca con la cuota de snapshots manuales**

Dado "claude-1" ya alcanzó <límite> en "shop-feat-a"
Cuando pide otro `snapshot`
Entonces la petición se rechaza con el motivo y <espera>
  Y los snapshots anteriores siguen intactos
  Y los snapshots previos de las operaciones protegidas se siguen tomando

Ejemplos:
| límite | espera |
| 5 snapshots en el último minuto | los segundos hasta que pueda pedir otro |
| 20 snapshots en las últimas 24 h | la hora a la que el más antiguo sale de la ventana |

**Escenario: Sin atribuir no toma snapshots**

Dado un cliente MCP resuelto como "sin atribuir" en "shop-feat-a"
Cuando pide `snapshot`
Entonces la petición se rechaza con la acción "usa register_agent"
  Y no se crea ningún snapshot

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y ADR-CKP-002).
