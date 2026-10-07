---
id: US-GRP-022
title: "El desarrollador decide qué repos descubiertos se observan y los que descarta no vuelven a aparecer"
type: us
status: draft
priority: medium
created: 2026-10-07
updated: 2026-10-07
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-001
    - US-GRP-006
    - US-GRP-020
    - US-CKP-025
tags:
  - motor-local
  - repos-descubiertos
  - confirmacion-humana
  - should
---

# US-GRP-022: El desarrollador decide qué repos descubiertos se observan y los que descarta no vuelven a aparecer

## Descripción

**Como** desarrollador con decenas de repos en mis carpetas de código, **quiero** aceptar los repos descubiertos que me interesan y descartar el resto de una vez, **para** observar solo aquello en lo que trabajan mis agentes sin que la lista de propuestas me persiga.

**Valor**: la decisión de observar sigue siendo del humano (BR-AUTH-001) y con más de 100 repos clonados la lista de propuestas no se vuelve ruido.

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO, sobre la propuesta A1 aceptada por Rene Bonilla (2026-10-06). El PO separa esta historia de US-GRP-020 para que cada una quepa en una rama corta (seis escenarios como máximo).

## Reglas cubiertas

BR-AUTH-003 (aceptar exige confirmación humana; descartar es persistente) · BR-AUTH-001 (aceptar es añadir; solo el desarrollador) · BR-CONS-005 (la decisión sobrevive al reinicio) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-020 (repos descubiertos), US-GRP-001 (añadir un repo).
- **Externas**: aceptar desde la TUI es de US-CKP-025; sin ella se acepta desde la CLI. Quién pide la acción (el humano o un agente) lo resuelve el motor como en el resto de acciones reservadas al humano; transversal (lo define el Arquitecto).
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: Aceptar un repo descubierto lo observa**

Dado "billing" descubierto en la raíz "~/code"
Cuando el desarrollador lo acepta desde su terminal
Entonces "billing" está observado igual que si lo hubiera añadido con `raptor repo add`
  Y deja de figurar en `raptor repo discovered`

**Escenario: Un repo descartado no vuelve a proponerse**

Dado "legacy-tools" descubierto en la raíz "~/code"
Cuando el desarrollador lo descarta y después se reinicia el motor
Entonces "legacy-tools" no figura en `raptor repo discovered` ni genera avisos nuevos
  Y "legacy-tools" no está observado

**Escenario: Un repo descartado se puede añadir a mano**

Dado "legacy-tools" descartado
Cuando el desarrollador lo añade con `raptor repo add`
Entonces "legacy-tools" está observado

**Escenario: Un agente no puede aceptar ni descartar**

Dado "billing" descubierto y "Claude Code" conectado por MCP al repo "shop"
Cuando el agente consulta las herramientas del MCP o pide aceptar o descartar "billing" desde su terminal
Entonces ninguna herramienta expone los repos descubiertos ni permite decidir sobre ellos
  Y la petición desde la terminal del agente se rechaza y "billing" sigue descubierto, sin observar

**Escenario: Un repo descubierto que ya no existe no se puede aceptar**

Dado "billing" descubierto y su carpeta borrada después
Cuando el desarrollador intenta aceptarlo
Entonces la petición se rechaza con el motivo
  Y "billing" deja de figurar como descubierto

**Escenario: Retirar una raíz quita sus propuestas y no toca lo observado**

Dado la raíz "~/code" con "billing" descubierto y "shop" observado
Cuando el desarrollador retira la raíz "~/code"
Entonces "billing" deja de figurar como descubierto
  Y "shop" sigue observado

## Requisitos Técnicos

> Arquitecto, 2026-10-07. Decisión del orquestador, validada por el Arquitecto. Diseño en ADR-GRP-010, Enmienda (2026-10-07) N6 y N8, aceptada (**Decisión de Rene (2026-10-07)**); seguridad en SEC-03 y SEC-15.

- **Aceptar es `repo.add`** (comando reservado que ya existe, US-GRP-001) sobre la ruta del candidato, desde `raptor repo add <ruta>` o desde la TUI. El daemon vuelve a comprobar que la ruta existe y que es el mismo repo (clave del directorio Git común) antes de añadirlo. Si ya no existe, rechaza con un motivo tipado y retira el candidato.
- **Descartar es un método reservado nuevo**, `discovery.dismiss` (`raptor repo dismiss <ruta>`). El descarte se guarda por ruta en el índice global del perfil (supuesto del PO en BR-AUTH-003, ratificado: **Decisión de Rene (2026-10-07)**) y sobrevive a un reinicio. Un `raptor repo add` posterior observa el repo y borra el descarte.
- **Retirar una raíz** (`discovery.root.remove`) quita sus candidatos pendientes y no toca los repos observados ni los descartes.
- **Un agente no decide**: el daemon rechaza `repo.add` y `discovery.dismiss` a un cliente que desciende de un agente (SEC-03), y ninguno de los dos existe en el perfil `mcp` (SEC-MCP-01). Las dos decisiones quedan en `reserved.audit`.
- **Un repo aceptado entra activo** y después sigue los niveles de observación (ADR-GRP-010 N1, TS-GRP-006).
- **Verificación**: tests del canal con un daemon de prueba y un perfil temporal; cliente bajo un agente simulado → rechazado; instantánea de `tools/list` del MCP sin métodos de descubrimiento; y un candidato borrado antes de aceptarlo.

## Diseño y Dev Spec

- **Diseño:** presentación en la CLI según DSYS-GRP-001; la decisión en la TUI es de US-CKP-025.
- **Dev Spec:** pendiente.
