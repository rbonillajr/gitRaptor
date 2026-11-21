---
id: US-WTL-001
title: "El desarrollador crea un worktree con rama con nombre y artefactos copiados desde una semilla quieta"
type: us
status: ready
priority: high
milestone: M3
blocked_by:
  - US-MCP-019
created: 2026-11-10
updated: 2026-11-21
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  business:
    - BRD-GRP-001
  adrs:
    - ADR-GRP-012
  stories:
    - US-CKP-018
    - US-MCP-019
  rules:
    - BR-WTL-001
    - BR-WTL-002
    - BR-WTL-003
    - BR-WTL-004
    - BR-WTL-016
  specs:
    - DS-US-WTL-001
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - raptor-new
  - semilla
  - copy-on-write
  - must
---

# US-WTL-001: El desarrollador crea un worktree con rama con nombre y artefactos copiados desde una semilla quieta

## Descripción

**Como** desarrollador orquestador, **quiero** crear con `raptor new <rama>` un worktree con su rama con nombre y sus artefactos de compilación copiados con copy-on-write desde una semilla quieta, **para** tener cada worktree listo en segundos sin duplicar el disco ni armar a mano la copia.

**Valor**: BR-07 (Must). Primera mitad de la antigua US-WTL-001, partida el 2026-11-10 por tamaño: **lanzar el agente e instalar la skill** es [US-WTL-013](./US-WTL-013-lanzar-agente-e-instalar-skill.md), que depende de esta. `raptor new` sin `--agent` solo crea el worktree. En la app de escritorio, la acción es «Nuevo worktree» (alcance según CTX-DSK-001). Plataforma: macOS primero; Linux, Windows y discos sin copy-on-write: Pendiente: etapa de validación multiplataforma.

## Criterios de Aceptación

**Escenario: Worktree con copia desde la semilla**

Dado un repo con semilla de `main` del toolchain y perfil actuales, sin nadie trabajando en ella
  Y un `.worktreeinclude` que lista `target/` y `node_modules/`
Cuando el desarrollador ejecuta `raptor new feat-login`
Entonces existe un worktree en la rama con nombre "feat-login" con `target/` y `node_modules/` copiados desde la semilla
  Y ningún agente se lanza ni se instala ninguna skill

**Escenario: Primera vez en un repo, la semilla se crea desde main**

Dado un repo sin semilla
Cuando el desarrollador ejecuta `raptor new feat-a`
Entonces GitRaptor crea la semilla desde `main` y después el worktree "feat-a" con la copia
  Y el desarrollador puede reconstruir la semilla cuando quiera con `raptor seed refresh`

**Escenario: Sin .worktreeinclude no se copia ningún ignorado**

Dado un repo sin `.worktreeinclude`
Cuando el desarrollador ejecuta `raptor new feat-b`
Entonces el worktree "feat-b" se crea sin ningún archivo ignorado copiado

**Escenario: Solo se copia lo que el repo lista**

Dado un `.worktreeinclude` que lista `target/` y una semilla que también tiene `.claude/runs/` y `.env`
Cuando el desarrollador crea el worktree "feat-c"
Entonces "feat-c" recibe `target/` y no recibe `.claude/runs/` ni `.env`

**Escenario: Rama ya sacada en otro worktree**

Dado la rama "feat-login" sacada en el worktree "../wt-login"
Cuando el desarrollador ejecuta `raptor new feat-login`
Entonces la creación se rechaza indicando que "feat-login" ya está en "../wt-login"
  Y no se crea ningún worktree

**Escenario: Crear sin lanzar agente por el MCP sigue como hasta ahora**

Dado un agente conectado por el MCP
Cuando pide crear un worktree
Entonces lo prepara sin lanzar ningún agente en él

**Escenario: Poco espacio libre, no se crea el worktree**

Dado un volumen con menos espacio libre que el suelo (el mayor de 5 GiB y el 5 % del volumen)
Cuando el desarrollador ejecuta `raptor new feat-h`
Entonces se rechaza con el código `no-free-space` y la acción "libera espacio en ese disco"
  Y no se crea ningún worktree ni se copia nada desde la semilla

**Escenario: Nunca HEAD separado**

Dado cualquier creación con `raptor new`
Cuando el worktree queda creado
Entonces su HEAD apunta a una rama con nombre, nunca a un commit suelto

**Escenario: Semilla de otro toolchain o perfil**

Dado una semilla generada con un toolchain distinto del actual
Cuando el desarrollador ejecuta `raptor new feat-e`
Entonces ningún artefacto de esa semilla se copia a "feat-e"

**Escenario: Semilla con trabajo en curso**

Dado una semilla en la que hay una sesión de agente
Cuando el desarrollador ejecuta `raptor new feat-g`
Entonces ningún artefacto se copia desde ella a "feat-g"

**Esquema del escenario: Un agente no puede compilar la semilla**

Dado el perfil personal del desarrollador con `worktree.seedBuild` igual a `cargo build --workspace`
  Y "claude-1" usa la CLI `raptor`, que recibe el perfil `mcp` porque la lanzó un agente
  Y <situación>
Cuando "claude-1" ejecuta `<orden>`
Entonces la orden se rechaza con el código de operación reservada, el motivo "compilar la semilla es una operación reservada al desarrollador" y la acción "el desarrollador ejecuta `raptor seed refresh` desde su terminal"
  Y `cargo build --workspace` no se ejecuta
  Y la semilla queda como estaba

Ejemplos:
| situación | orden |
| la semilla de `main` existe | raptor seed refresh |
| el repo no tiene semilla | raptor new feat-h |

**Escenario: El desarrollador sí compila la semilla con su orden**

Dado el perfil personal del desarrollador con `worktree.seedBuild` igual a `cargo build --workspace`
  Y un repo sin semilla
Cuando el desarrollador ejecuta `raptor seed refresh` desde su terminal
Entonces GitRaptor crea la semilla desde `main` y ejecuta `cargo build --workspace` en ella, en primer plano, sin pasar por un shell
  Y la salida de la compilación se ve en la terminal del desarrollador

**Escenario: La orden de compilar la semilla del archivo del equipo se ignora**

Dado el `.gitraptor/settings.json` versionado del equipo con `worktree.seedBuild` igual a `make all`
  Y ni el perfil personal ni la configuración local personal tienen `worktree.seedBuild`
Cuando el desarrollador ejecuta `raptor seed refresh`
Entonces `make all` no se ejecuta
  Y la salida avisa de que `worktree.seedBuild` del archivo del equipo se ignora porque solo se lee del perfil personal o de la configuración local personal
  Y la semilla se crea desde `main` sin compilar

## Criterios de Certificación

- Validar que crear un worktree con copia desde una semilla quieta tarda como máximo lo medido con worktrunk (7–10 s en el repo de GitRaptor) y que el `df` no baja el tamaño completo de `target/` · *(método: medición en macOS, como el diario del 2026-11-09)*.
- Verificar que ningún ignorado fuera del `.worktreeinclude` llega al worktree nuevo · *(método: prueba automatizada con repo temporal)*.

## Requisitos Técnicos

Arquitecto, 2026-11-21: [DS-US-WTL-001](../dev-specs/US-WTL-001-dev-spec.md) (`spec-ready`). `raptor new` reutiliza la operación create-worktree de DS-US-MCP-019 con dos argumentos que solo acepta la capa `cockpit` (`reuse_branch`, `seed_copy`). Deshacer la creación es descartar el worktree (US-CKP-017 / US-WTL-002). Las preguntas 5b, 9, 11, 14 y 16 quedan con la decisión provisional del Arquitecto de la DS hasta que Rene las cierre; un escenario de «rama existente se reutiliza» se añade al cerrar la 14.

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (CLI y «Nuevo agente» en la app).

## Dependencias

- **De User Stories**: Requiere **US-MCP-019** (la operación de crear worktree del motor, que esta historia reutiliza y amplía con la copia desde la semilla) · Relacionado US-CKP-018 (preparar sin lanzar desde la TUI) · Bloquea **US-WTL-013** (lanzar el agente e instalar la skill), US-WTL-004, US-WTL-006, US-WTL-007, US-WTL-009, US-WTL-012.
- **Decisión del orquestador (2026-11-21), validada por PO**: esta historia ya no depende de US-CKP-018. Atarla a la TUI la hacía esperar al flujo de escritura de US-CKP-014 y al descarte de US-CKP-017, que `raptor new` no usa. Lo que necesita es la operación de crear worktree, cuya dueña es US-MCP-019; ninguna de las tres historias la construye dos veces. US-CKP-018 se mantiene como la superficie de la TUI para preparar sin lanzar.
- **Compilar la semilla (Pregunta 21, decidida el 2026-11-21)**: aceptada con condiciones por el orquestador, con la delegación de Rene. La orden `worktree.seedBuild` solo se lee del perfil personal o de la configuración local personal; la ejecuta la CLI en primer plano, sin shell y dentro de la semilla, nunca el motor en segundo plano ni el MCP; con el perfil `mcp` se rechaza como operación reservada. Escenarios añadidos arriba.
- **Sin bloqueos de enmienda**: la enmienda de BRD-GRP-001 § 6.4 (lanzar un agente) se aplicó el 2026-11-10 y afecta a US-WTL-013, no a esta historia.
- **Preguntas abiertas que esta historia no fija**: 5b (qué hace `raptor new` con una semilla de otro toolchain o perfil: provisional, crea el worktree sin copia y avisa), 11 (disco sin reflink: provisional, no se copia y se avisa), 14 (reutilizar una rama existente: provisional, se reutiliza), 16 (ignorados que nunca se copian: provisional, lista fija de exclusiones). Se añaden escenarios cuando Rene las cierre; los supuestos provisionales no bloquean.
- **Cross-cutting / transversales**: dónde vive la semilla y cuánto ocupa (Pregunta 9) y cómo se copia con copy-on-write: transversal (lo define el Arquitecto).
