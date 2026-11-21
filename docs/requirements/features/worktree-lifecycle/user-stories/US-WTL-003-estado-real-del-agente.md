---
id: US-WTL-003
title: "El desarrollador ve qué agentes esperan su decisión, cuáles esperan el CI y cuáles se colgaron"
type: us
status: ready
priority: high
milestone: M3
blocked_by: []
created: 2026-11-10
updated: 2026-11-21
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-GRP-012
  stories:
    - US-CKP-001
    - US-CKP-002
    - US-DSK-004
    - US-WTL-002
    - US-WTL-011
  rules:
    - BR-WTL-007
    - BR-WTL-009
    - BR-WTL-016
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - estado-del-agente
  - cockpit
  - github-actions
  - must
---

# US-WTL-003: El desarrollador ve qué agentes esperan su decisión, cuáles esperan el CI y cuáles se colgaron

## Descripción

**Como** desarrollador orquestador, **quiero** ver cada sesión de agente en uno de cinco estados (activo, esperando una decisión humana, esperando el CI, colgado o terminado) con las que me necesitan arriba, **para** atender primero al agente que está parado por mí y dejar de depender de scripts propios de supervisión.

**Valor**: BR-04 y BR-06 (Must). Amplía los estados de ADR-GRP-012 (activo, inactivo, terminado) con la enmienda de los estados nuevos. En la CLI con salida JSON, la TUI y la app. El estado solo informa: ninguno dispara una acción sobre el agente ni sobre su worktree. Los cinco estados son también los que protegen un worktree del descarte (US-WTL-002).

Las señales son tres: el proceso del agente vivo y su actividad o la de su worktree; los avisos de Claude Code de que espera al humano o terminó su turno, que llegan solo si el humano activó esa señal; y el estado de GitHub Actions de la rama, en solo lectura.

## Criterios de Aceptación

**Escenario: Esperando una decisión humana sube a la zona de atención**

Dado "claude-1" trabajando en "feat-a" y "claude-2" en "feat-b"
  Y la señal de avisos de Claude Code activada por el desarrollador en el repo
Cuando "claude-2" se detiene a pedir una decisión del humano
Entonces el cockpit muestra "claude-2" como «esperando una decisión humana» en la zona de atención, por encima de "claude-1" «activo»

**Escenario: Un prompt de aprobación largo no es colgado ni activo**

Dado "claude-2" con la sesión viva y un prompt de aprobación de permiso abierto desde hace 40 minutos
  Y la señal de avisos de Claude Code activada
  Y sin actividad en "feat-b" ni CI en curso para "feat-b"
Cuando el cockpit actualiza el estado
Entonces "claude-2" aparece como «esperando una decisión humana»
  Y nunca como «colgado» ni como «activo», aunque pasen los 15 minutos del umbral

**Escenario: Turno terminado con la sesión abierta espera al humano**

Dado "claude-7" con la sesión viva que terminó su turno hace 5 minutos
  Y la señal de avisos de Claude Code activada
  Y ninguna ejecución de CI en curso para su rama
Cuando el cockpit actualiza el estado
Entonces "claude-7" aparece como «esperando una decisión humana» y no como «terminado»

**Escenario: Agente vivo con su CI en curso espera el CI**

Dado "claude-3" con la sesión viva, sin actividad desde hace 20 minutos en "feat-c"
  Y una ejecución de GitHub Actions en curso para la rama "feat-c"
Cuando el cockpit actualiza el estado
Entonces "claude-3" aparece como «esperando el CI» y no como «colgado» ni como «activo»

**Escenario: Trabajando mientras corre el CI sigue activo**

Dado "claude-3" escribiendo archivos en "feat-c" hace 1 minuto
  Y una ejecución de GitHub Actions en curso para "feat-c"
Cuando el cockpit actualiza el estado
Entonces "claude-3" aparece como «activo»

**Escenario: Colgado tras 15 minutos sin actividad**

Dado "claude-4" en "feat-d" con la sesión viva
  Y 15 minutos sin actividad del agente ni cambios en "feat-d"
  Y ninguna ejecución de CI en curso para "feat-d"
  Y ningún aviso de Claude Code de que espera al humano
Cuando el cockpit actualiza el estado
Entonces "claude-4" aparece como «colgado» en la zona de atención

**Escenario: Umbral configurable**

Dado el umbral de «colgado» configurado en 30 minutos
  Y "claude-4" con 20 minutos sin actividad y sin CI en curso
Cuando el cockpit actualiza el estado
Entonces "claude-4" no aparece como «colgado»

**Escenario: Colgado nunca actúa solo**

Dado "claude-4" «colgado» desde hace 2 horas
Cuando el cockpit sigue actualizando el estado
Entonces GitRaptor no termina el proceso de "claude-4", no le envía ninguna entrada, no lo relanza y no escribe en "feat-d"
  Y "claude-4" sigue «colgado» en la zona de atención hasta que el desarrollador actúe o el agente vuelva a tener actividad

**Escenario: Nunca activo si está colgado ni terminado si sigue vivo**

Dado "claude-5" cuya sesión sigue viva y lleva 16 minutos sin actividad y sin CI
Cuando el desarrollador consulta la flota
Entonces "claude-5" no figura como «activo» ni como «terminado»

**Escenario: Terminado**

Dado "claude-6" cuyo proceso acabó
Cuando el cockpit actualiza el estado
Entonces "claude-6" aparece como «terminado» y sale de la zona de atención

**Escenario: Sin la señal de avisos, el estado se deduce y lo dice**

Dado un repo sin la señal de avisos de Claude Code activada
  Y "claude-8" con la sesión viva y 20 minutos sin actividad ni CI en curso
Cuando el cockpit actualiza el estado
Entonces "claude-8" aparece como «colgado» en la zona de atención con la nota «sin avisos de Claude Code: puede estar esperando tu decisión»
  Y la flota indica que en ese repo «esperando una decisión humana» no se puede detectar hasta que el desarrollador active la señal
  Y los estados «activo», «esperando el CI» y «terminado» se siguen mostrando con lo que se deduce del proceso, del worktree y del CI

**Escenario: La señal de avisos la activa solo el desarrollador**

Dado un repo sin la señal de avisos de Claude Code activada
Cuando "claude-2" pide activarla desde el MCP o desde la CLI
Entonces la petición se rechaza indicando que solo el desarrollador puede activarla
  Y la señal sigue sin activar y el estado de "claude-2" sigue deducido sin avisos

**Escenario: Avisos que no vienen de la configuración del desarrollador no cuentan**

Dado la señal de avisos activada por el desarrollador en el repo
  Y "claude-2" que añadió por su cuenta otra configuración que envía avisos de «esperando al humano»
Cuando llega uno de esos avisos
Entonces el estado de "claude-2" no cambia por ese aviso

**Escenario: Estado por la CLI con salida JSON**

Dado "claude-1" «activo», "claude-2" «esperando una decisión humana» y "claude-6" «terminado»
Cuando el desarrollador consulta la flota desde la CLI con salida JSON
Entonces cada sesión trae su estado entre los cinco y si la señal de avisos está activada en el repo

**Escenario: Solo lectura del CI**

Dado "claude-3" «esperando el CI»
Cuando el desarrollador consulta su estado
Entonces GitRaptor lee el resultado de GitHub Actions sin lanzar, reintentar ni cancelar ninguna ejecución

**Escenario: El CI no se puede leer**

Dado un repo cuyo estado de GitHub Actions no se puede leer
Cuando un agente con la sesión viva lleva 15 minutos sin actividad
Entonces el cockpit indica junto a ese agente que el estado del CI no se pudo leer
  Y nunca lo muestra como «esperando el CI»

## Criterios de Certificación

- Validar los cinco estados con al menos una sesión en cada uno, con la señal de avisos activada y sin ella · *(método: prueba automatizada con sesiones simuladas y repos temporales)*.
- Verificar que ningún estado, «colgado» incluido, termina, relanza ni escribe en el worktree de un agente · *(método: prueba automatizada con repos temporales)*.
- Confirmar con el orquestador del proyecto que el estado del cockpit sustituye al supervisor y al watchdog de `bitacora/orquestador/` durante una jornada de dogfooding · *(método: demo en dogfooding)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (estados en la fila del cockpit y en la app, nota «sin avisos de Claude Code»).

## Dependencias

- **De User Stories**: Relacionado US-CKP-001 (flota en vivo), US-CKP-002 (zona de atención), US-DSK-004 · Bloquea US-WTL-002 (protección por estados) y US-WTL-011.
- **ADR citado**: ADR-GRP-012 con su enmienda de los estados nuevos (cinco estados, señales y criterio de «colgado»), que escribe el Arquitecto. La historia se escribe contra esa enmienda; si la enmienda cambia un valor de los escenarios, se ajusta la historia.
- **Por revisar con el Arquitecto**: sin la señal de avisos, un agente parado en un prompt no se distingue de uno colgado; la historia lo muestra como «colgado» con la nota (escenario «Sin la señal de avisos»). Si la enmienda define otro trato, manda la enmienda.
- **Externas**: GitHub Actions, solo lectura; en M3 no se lee otro CI. Los avisos de Claude Code dependen de que el desarrollador los active.
- **Preguntas abiertas**: la 4 (criterio de «colgado») está resuelta en CTX-WTL-001.
- **Cross-cutting / transversales**: cómo se mide la actividad del agente y del repo para el umbral de 15 minutos, y cómo se reconoce que un aviso viene de la configuración que activó el desarrollador: transversal (lo define el Arquitecto).
