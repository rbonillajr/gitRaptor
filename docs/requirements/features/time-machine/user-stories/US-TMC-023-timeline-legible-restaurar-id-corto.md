---
id: US-TMC-023
title: "El desarrollador lee el timeline de un vistazo y restaura un punto con el id corto que ve en la misma línea"
type: us
status: implemented
priority: high
created: 2026-10-17
updated: 2026-10-17
domain: GRP
epic: E-001
feature: time-machine
source: inline
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-006, US-TMC-007, US-TMC-009]
covers: [BR-TMC-CONS-005, BR-TMC-CONS-003, D-TMC-12, D-TMC-19]
blocked_by: []
tags: [time-machine, timeline, restore, dogfooding, ux]
---

# US-TMC-023: El desarrollador lee el timeline de un vistazo y restaura un punto con el id corto que ve en la misma línea

## Descripción

**Como** desarrollador orquestador
**Quiero** un `raptor timeline` con una línea por evento, columnas alineadas, agrupado por día, con lo de poco valor resumido y el id de restauración en la misma línea
**Para** encontrar en segundos el punto al que quiero volver y restaurarlo sin copiar un id largo

**Origen**: dogfooding de Rene (2026-10-17). Cada evento ocupaba tres líneas, `files: not available` salía en todas, los cambios sin agente salían como `(no agent)` y Rene no encontró el id para `raptor restore`. Amplía la presentación de US-TMC-006 (que sigue siendo el contenido) y el id que acepta US-TMC-009.

## Reglas cubiertas

BR-TMC-CONS-005 (lo no atribuido nunca figura como humano) · BR-TMC-CONS-003 (el nivel de protección sigue visible) · D-TMC-12 ("Tú u otro") · D-TMC-19 (timeline por repo) — ver [business-rules.md](../business-rules.md). Reglas de presentación: sección "Decisiones" de esta ficha.

## Formato de salida (contrato)

Salida sin color, con símbolos ASCII y sin truncar (no interactiva). Repo `gitRaptor`, consultado el 2026-10-17 a las 10:30 desde el worktree `uxtl`. Con símbolos Unicode, `restore 47bfc884` se escribe `↺ 47bfc884`; el resto no cambia.

**E1 (en)**

```text
Last 24 h of gitRaptor: 11 entries, 7 of them summarized (--verbose shows them all)

2026-10-16 (yesterday)
  22:41  main  commit on main                                           Claude Code   observed  restore 3e9d07b1
  23:05  main  undo                                                     you or other  prior     restore 5c2a8e40

2026-10-17 (today)
  09:12  uxtl  2 branches created and deleted: rbonillajr/uxtl, +1      Claude Code   none      -
  09:20  uxtl  rebase of feat/ux-timeline-status                        Claude Code   prior     restore 47bfc884
  09:41  uxtl  reset on feat/ux-timeline-status without moving it (x3)  you or other  none      -
  10:02  uxtl  commit on feat/ux-timeline-status                        Claude Code   observed  restore 9a1c0d22

you or other: no agent was detected; it may be you or another tool
restore a point with raptor restore <id>; a unique prefix of 6 or more characters is enough
```

**E2 (es)**

```text
Últimas 24 h de gitRaptor: 11 entradas, 7 de ellas resumidas (--verbose las muestra todas)

2026-10-16 (ayer)
  22:41  main  commit en main                                     Claude Code  observado  restore 3e9d07b1
  23:05  main  undo                                               tú u otro    previo     restore 5c2a8e40

2026-10-17 (hoy)
  09:12  uxtl  2 ramas creadas y borradas: rbonillajr/uxtl, +1    Claude Code  ninguno    -
  09:20  uxtl  rebase de feat/ux-timeline-status                  Claude Code  previo     restore 47bfc884
  09:41  uxtl  reset en feat/ux-timeline-status sin moverla (x3)  tú u otro    ninguno    -
  10:02  uxtl  commit en feat/ux-timeline-status                  Claude Code  observado  restore 9a1c0d22

tú u otro: no se detectó ningún agente; puede ser tuyo o de otra herramienta
restaura un punto con raptor restore <id>; basta un prefijo único de 6 caracteres o más
```

**E3, prefijo ambiguo (en / es)**

```text
raptor restore: 47bfc8 matches 2 restore points: 47bfc884 (2026-10-17 09:20, uxtl, rebase of feat/ux-timeline-status), 47bfc8e1 (2026-10-16 22:41, main, commit on main); type more characters; nothing changed
raptor restore: 47bfc8 coincide con 2 puntos de restauración: 47bfc884 (2026-10-17 09:20, uxtl, rebase de feat/ux-timeline-status), 47bfc8e1 (2026-10-16 22:41, main, commit en main); escribe más caracteres; nada cambió
```

**E4, ventana vacía (en / es)**

```text
no activity in the last 24 h; widen it with --since, such as --since 7d
no hay actividad en las últimas 24 h; amplía la ventana con --since, por ejemplo --since 7d
```

## Criterios de Aceptación

**Escenario: El timeline cabe en una línea por evento y dice a qué punto volver**

Dado el repo "gitRaptor" con la actividad de los ejemplos E1 y E2 en las últimas 24 horas
Cuando el desarrollador consulta el timeline sin opciones
Entonces cada evento o resumen ocupa una sola línea con hora, worktree, qué pasó, quién, nivel de protección y acción, en columnas alineadas
  Y los eventos se agrupan por día, del más antiguo al más reciente
  Y cada evento con punto de restauración muestra su id corto en la misma línea
  Y ningún cambio sin agente detectado figura como "humano", "no agent" ni "sin agente": figura como "you or other" / "tú u otro" y la leyenda lo explica una vez
  Y la salida coincide con E1 en inglés y con E2 en español

**Escenario: Restaurar con el id corto**

Dado el punto de restauración "47bfc884" del timeline, y que ningún otro punto del repo empieza por "47bfc8"
Cuando el desarrollador pide restaurar el punto "47bfc8"
Entonces el worktree vuelve a ese punto igual que con el id completo, con la misma confirmación y el mismo estado previo guardado

**Escenario: Un prefijo que no identifica un único punto**

Dado dos puntos de restauración del repo que empiezan por "47bfc8"
Cuando el desarrollador pide restaurar el punto "47bfc8"
Entonces la restauración se rechaza como uso incorrecto, sin cambiar nada
  Y el mensaje lista los puntos candidatos con su id corto, fecha, hora, worktree y qué pasó, como en E3

**Escenario: Lo de poco valor se resume y lo recuperable nunca**

Dado en un mismo día y worktree dos ramas creadas y borradas sin otro evento entre medias y tres resets seguidos que no movieron la rama, ninguno con punto de restauración
  Y un reset que no movió la rama pero sí guardó un punto de restauración
Cuando el desarrollador consulta el timeline sin "--verbose"
Entonces las dos ramas figuran en una línea de resumen y los tres resets en otra con "(x3)"
  Y el reset con punto de restauración figura en su propia línea con su id
  Y la cabecera dice cuántas entradas hay y cuántas se resumieron

**Escenario: Sin actividad en la ventana por defecto**

Dado un repo cuya última actividad fue hace tres días
Cuando el desarrollador consulta el timeline sin "--since"
Entonces recibe el aviso de E4, que no dice que el repo no tenga actividad
  Y con "--since 7d" ve esa actividad

**Escenario: El detalle y la salida para máquinas**

Dado el repo de E1
Cuando el desarrollador consulta el timeline con "--verbose"
Entonces ve todas las entradas sin resumir, con el actor y su origen, el nivel con su texto completo, el id completo, lo que deshizo cada undo y la lista de archivos solo cuando hay archivos que mostrar
  Y con "--json" la respuesta es la misma que antes de esta historia: sin ventana por defecto ni resúmenes

## Criterios de Certificación

- Verificar que, con una TTY y color, el texto de "qué pasó" se colorea por tipo: operación de la Time Machine (undo, redo, restore, operación protegida) con el acento, evento de Git con el texto normal, snapshot manual con información y líneas de resumen atenuadas; y que con `NO_COLOR`, `--no-color` o sin TTY la salida es idéntica a E1/E2.
- Verificar que en una TTY de 80 columnas ninguna línea pasa del ancho: se recorta solo "qué pasó" con "…" (ASCII "..."), nunca la hora, el nivel ni el id.
- Confirmar que "hace 3 días" sin actividad en 24 h no dice "no activity yet", y que un repo sin ninguna actividad sigue diciendo "no activity yet" (US-TMC-006).
- Regresión: US-TMC-006 (contenido), US-TMC-007 (filtros, que se combinan con la ventana) y US-TMC-009 (restaurar con el id completo).

## Decisiones

**Decisión del orquestador (2026-10-17), validada por PO.**

1. **Ventana por defecto**: últimas 24 h. `--since` la sustituye; `--limit` sigue en 50 por defecto. Solo afecta a la salida de texto; `--json` conserva el comportamiento actual.
2. **Orden**: del más antiguo al más reciente, para que lo último quede junto al prompt. Cabecera de día `AAAA-MM-DD`, con "(today)/(hoy)" y "(yesterday)/(ayer)"; sin nombres de día traducidos.
3. **Columnas**: dos espacios de sangría y dos entre columnas. Hora `HH:MM` en hora local. Worktree y quién crecen hasta su texto más ancho, con tope de 12 y "…" al recortar. Nivel: `prior`/`previo`, `observed`/`observado`, `manual`/`manual`, `none`/`ninguno`. Acción: `restore <id>` (Unicode `↺ <id>`) o `-` sin punto. Sin TTY no se recorta nada; con TTY solo se recorta "qué pasó", con un mínimo de 20 columnas.
4. **Quién**: el agente sin su origen (`Claude Code`, `codex-1`); el origen ("detectado", "registrado") pasa a `--verbose`. Lo no atribuido es "you or other" / "tú u otro", coherente con D-TMC-12 y con la leyenda del Cockpit ("cambios tuyos o de otra herramienta"). Se descarta "tú" a secas: afirmaría que fue el humano (BR-TMC-CONS-005). Sin detección, "agent not available" / "agente no disponible", como hoy.
5. **Id corto**: los 8 primeros caracteres del id del punto, o más si 8 no bastan para distinguirlo de los demás puntos del repo. `raptor restore` acepta el id completo o un prefijo de 6 o más caracteres que sea único entre los puntos del repo. Con menos de 6 se rechaza como uso incorrecto ("type at least 6 characters of the id that raptor timeline shows" / "escribe al menos 6 caracteres del id que muestra raptor timeline"). Si es ambiguo, se rechaza como uso incorrecto y se listan hasta 5 candidatos como en E3. Nada cambia en ninguno de los dos casos.

**Decisión del orquestador (2026-10-17): mínimo 6 por el coordinador, por seguridad (enumeración).** Los ejemplos de prefijo usan `47bfc8` (6 caracteres); la búsqueda del prefijo solo recorre los puntos del repo del solicitante (todos sus worktrees), según `tm_scope_for`.
6. **Resúmenes** (solo sin `--verbose`): nunca se resume una entrada con punto de restauración. Se resumen, por día y worktree: (a) una rama creada y borrada sin otra entrada sobre ella entre medias, en una línea "N branches created and deleted: <primera>, +K" / "N ramas creadas y borradas: <primera>, +K" con la hora de la primera creación (desde 1 par); (b) dos o más resets seguidos que no movieron la rama, en una línea con "(xN)". No depende del prefijo `rbonillajr/`. Si las entradas resumidas tienen actores distintos, quién dice "several" / "varios".
7. **Pie**: la leyenda de "tú u otro" solo si aparece; la pista de `raptor restore` solo si hay algún punto. `files` desaparece de la vista compacta; en `--verbose` aparece solo cuando la lista existe y no está vacía.
8. **Claves de mensaje propuestas** (el Arquitecto puede renombrarlas en Requisitos Técnicos; cada una en `en` y `es`): `timeline.header`, `timeline.day.today`, `timeline.day.yesterday`, `timeline.level.short.{prior,observation,manual,none}`, `timeline.action.restore`, `timeline.summary.branches`, `timeline.summary.resets`, `timeline.actor.several`, `timeline.legend.you-or-other`, `timeline.hint.restore`, `timeline.window-empty`, `actor.you-or-other`, `restore.ambiguous`, `restore.candidate`, `restore.too-short`. Se retira `timeline.point` de la vista compacta.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** el contrato de salida está en esta ficha (E1 a E4). El símbolo `↺` (ASCII `restore`) es un símbolo nuevo del design system: enmienda pendiente de `docs/design-system/README.md` § 2.2.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-006 (contenido del timeline), US-TMC-009 (restaurar), US-TMC-007 (filtros; esta historia no la bloquea ni la requiere).
- **Externas**: el Cockpit (F-001-02) mantiene su "Sin agente" con leyenda; alinear su texto con "tú u otro" queda fuera de esta historia.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.

Implementado en: PR #265.
