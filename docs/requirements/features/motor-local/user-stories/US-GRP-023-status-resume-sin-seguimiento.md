---
id: US-GRP-023
title: "El desarrollador ve en raptor status sus cambios importantes primero y los archivos sin seguimiento resumidos por carpeta"
type: us
status: draft
priority: medium
created: 2026-10-17
updated: 2026-10-17
domain: GRP
epic: E-001
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  stories: [US-GRP-001, US-GRP-002]
blocked_by: []
tags: [motor-local, status, dogfooding, ux]
---

# US-GRP-023: El desarrollador ve en raptor status sus cambios importantes primero y los archivos sin seguimiento resumidos por carpeta

## Descripción

**Como** desarrollador orquestador
**Quiero** que `raptor status` resuma por carpeta los archivos sin seguimiento cuando son muchos y muestre antes lo preparado y lo modificado
**Para** ver de un vistazo qué cambió de verdad en cada worktree sin bajar por cientos de líneas

**Origen**: dogfooding de Rene (2026-10-17): `raptor status` listó uno por uno 220 archivos sin seguimiento de `bitacora/` y enterró los dos archivos modificados que importaban.

## Formato de salida (contrato)

Salida sin color. Worktree principal con 1 archivo preparado, 2 modificados sin preparar y 220 sin seguimiento: 214 en `bitacora/`, 4 en `docs/` y 2 en la raíz.

**E1 (en)**

```text
/Users/rene/code/gitRaptor
  main worktree /Users/rene/code/gitRaptor
    branch main
    0 ahead and 0 behind main
    223 uncommitted changes: 1 staged, 2 not staged, 220 untracked
      staged modified docs/backlog.md
      not staged modified README.md
      not staged deleted notes.txt
      220 untracked: bitacora/ 214, docs/ 4, (root) 2

untracked files one by one: raptor status --verbose
```

**E2 (es)**

```text
/Users/rene/code/gitRaptor
  worktree principal /Users/rene/code/gitRaptor
    rama main
    0 por delante y 0 por detrás de main
    223 cambios sin commitear: 1 preparados, 2 sin preparar, 220 sin rastrear
      preparado modificado docs/backlog.md
      sin preparar modificado README.md
      sin preparar borrado notes.txt
      220 sin rastrear: bitacora/ 214, docs/ 4, (raíz) 2

archivos sin rastrear uno por uno: raptor status --verbose
```

Las líneas que ya existen (repo, worktree, rama, divergencia, recuento y cada cambio) conservan su texto actual en cada idioma. Lo nuevo es la línea de resumen y el pie. En español la salida usa "sin rastrear", el término vigente del catálogo, y no "sin seguimiento".

## Criterios de Aceptación

**Escenario: Muchos archivos sin seguimiento se resumen por carpeta**

Dado el worktree de los ejemplos E1 y E2
Cuando el desarrollador consulta el estado sin opciones
Entonces lo preparado figura primero, después lo modificado sin preparar, cada archivo en su línea
  Y los 220 archivos sin seguimiento figuran en una sola línea con el total y el recuento por carpeta de primer nivel, de mayor a menor
  Y al final figura una vez la pista de "--verbose"
  Y la salida coincide con E1 en inglés y con E2 en español

**Escenario: Pocos archivos sin seguimiento se siguen listando**

Dado un worktree con 3 archivos sin seguimiento
Cuando el desarrollador consulta el estado
Entonces cada uno de los 3 figura en su línea, como hasta ahora
  Y no aparece la línea de resumen ni la pista de "--verbose"

**Escenario: El detalle bajo demanda**

Dado el worktree de E1
Cuando el desarrollador consulta el estado con "--verbose"
Entonces cada archivo sin seguimiento que el motor envió figura en su línea
  Y con "--json" la respuesta es la misma que antes de esta historia

**Escenario: El motor no envió la lista completa**

Dado un worktree con 5000 archivos sin seguimiento de los que el motor solo envió una parte
Cuando el desarrollador consulta el estado
Entonces el resumen cuenta por carpeta los archivos enviados y añade cuántos quedaron sin listar, de modo que la suma da el total real
  Y nunca presenta el recuento parcial como si fuera el total

## Criterios de Certificación

- Verificar con el repo real de Rene (en modo lectura) que el resumen sustituye a los 220 archivos de `bitacora/`.
- Confirmar que un nombre de carpeta con secuencias de escape se escribe saneado, como el resto de rutas.
- Regresión: US-GRP-001 (estado de los worktrees) y US-GRP-002 (cambios en vivo).

## Decisiones

**Decisión del orquestador (2026-10-17), validada por PO.**

1. **Umbral**: con 10 o menos archivos sin seguimiento en un worktree se listan uno por uno; con más de 10, una línea de resumen.
2. **Línea de resumen**: "N untracked: <carpeta>/ <n>, ..." / "N sin rastrear: <carpeta>/ <n>, ...". La carpeta es el primer componente de la ruta dentro del worktree; los archivos de la raíz cuentan como "(root)" / "(raíz)". Orden por recuento descendente y, a igualdad, por nombre. Como mucho 5 carpetas; el resto se suma en "others <n>" / "otras <n>". Si el motor no envió toda la lista, se añade ", <n> not listed" / ", <n> sin listar". Separador ", " para que sea ASCII.
3. **Orden de los cambios**: preparado, sin preparar y sin seguimiento. Lo preparado y lo sin preparar no se resumen en esta historia.
4. **Pie**: "untracked files one by one: raptor status --verbose" / "archivos sin rastrear uno por uno: raptor status --verbose", una vez al final y solo si algún worktree se resumió.
5. **`--verbose`** es una opción nueva de `raptor status` y no cambia `--json`.
6. **Claves de mensaje propuestas** (el Arquitecto puede renombrarlas en Requisitos Técnicos; `en` y `es`): `status.untracked-summary`, `status.untracked-folder`, `status.untracked-root`, `status.untracked-others`, `status.untracked-unlisted`, `status.hint.verbose`.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** el contrato de salida está en esta ficha (E1 y E2).
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-GRP-001 y US-GRP-002.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real salvo la lectura de la certificación; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
