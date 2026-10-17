---
id: US-GRD-020
title: "El desarrollador entiende si su repo está protegido y qué repetir cuando una operación se deniega sin el servicio"
type: us
status: implemented
priority: medium
created: 2026-10-17
updated: 2026-10-17
domain: GRP
epic: E-001
feature: guardrails
source: inline
related:
  context:
    - CTX-GRD-001
  stories: [US-GRD-017, US-GRD-001, US-TMC-005]
blocked_by: []
tags: [guardrails, mensajes, dogfooding, ux]
---

# US-GRD-020: El desarrollador entiende si su repo está protegido y qué repetir cuando una operación se deniega sin el servicio

## Descripción

**Como** desarrollador orquestador
**Quiero** que `raptor guard status` diga en una línea si mi repo está protegido y que la denegación sin el servicio nombre la operación que de verdad se denegó
**Para** saber sin dudar si estoy cubierto y qué tengo que repetir para seguir trabajando

**Origen**: dogfooding de Rene (2026-10-17). Tras instalar bien la protección, `raptor guard status` decía `state: hooks only`, que se lee como "a medias". Y al denegar un borrado de rama sin el servicio (US-GRD-017), el mensaje añadía el párrafo del conjunto mínimo y pedía repetir el push.

**Fuera de alcance**: los mensajes de `raptor guard install` y `raptor guard uninstall` (otra rama, W77) y el resto de líneas de `raptor guard status`.

## Formato de salida (contrato)

Salida sin color, en stderr para el hook. Rama `feat-x`, sin el servicio en marcha.

**E1, primera línea de `raptor guard status` (en / es)**

```text
state: protected, base branch main
estado: protegido, rama base main
```

```text
state: protected, base branch not confirmed (protected meanwhile: main, master)
estado: protegido, rama base sin confirmar (mientras tanto se protegen: main, master)
```

```text
state: partly protected: the hooks are not active (details below)
estado: protección incompleta: los hooks no están activos (detalle abajo)
```

`state: unprotected` / `estado: sin protección` no cambia. La línea aparte de la rama base protegida desaparece, porque ya va en la primera.

**E2, borrado de rama denegado sin el servicio (en / es)**

```text
GitRaptor: branch feat-x was not deleted: a recovery point could not be saved first (GitRaptor is not running or no longer observes this repository).
To go on: start GitRaptor (`raptor daemon status` starts it) and delete the branch again; if it is already running, observe the repo again with raptor repo add.
```

```text
GitRaptor: no se borró la rama feat-x: no se pudo guardar antes un punto de recuperación (GitRaptor no está en marcha o ya no observa este repositorio).
Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a borrar la rama; si ya está en marcha, vuelve a observar el repo con raptor repo add.
```

**E3, rebase denegado sin el servicio (en / es)**

```text
GitRaptor: the rebase of feat-x did not run: a recovery point could not be saved first (GitRaptor is not running or no longer observes this repository).
To go on: start GitRaptor (`raptor daemon status` starts it) and run the rebase again; if it is already running, observe the repo again with raptor repo add.
```

```text
GitRaptor: no se ejecutó el rebase de feat-x: no se pudo guardar antes un punto de recuperación (GitRaptor no está en marcha o ya no observa este repositorio).
Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a lanzar el rebase; si ya está en marcha, vuelve a observar el repo con raptor repo add.
```

## Criterios de Aceptación

**Escenario: Protección instalada con la rama base confirmada**

Dado un repo con la protección instalada, los hooks activos y la rama base "main" confirmada
Cuando el desarrollador consulta el estado de la protección
Entonces la primera línea dice que el repo está protegido y nombra la rama base, como en E1
  Y la salida no contiene "hooks only" ni "solo hooks"

**Escenario: Protección instalada con la rama base sin confirmar**

Dado un repo con la protección instalada, los hooks activos y una configuración de equipo que deja sin confirmar la rama base, protegiendo "main" y "master"
Cuando el desarrollador consulta el estado de la protección
Entonces la primera línea dice que el repo está protegido, que la rama base está sin confirmar y qué ramas se protegen mientras tanto, como en E1

**Escenario: Hooks instalados pero no activos**

Dado un repo con la protección instalada en el que otra herramienta cambió los hooks que usa Git
Cuando el desarrollador consulta el estado de la protección
Entonces la primera línea dice que la protección está incompleta, como en E1, y nunca que el repo está protegido
  Y las líneas siguientes siguen explicando la causa como hasta ahora

**Escenario: Un borrado de rama denegado sin el servicio nombra el borrado**

Dado un repo protegido y el servicio de GitRaptor parado
Cuando un agente borra la rama local "feat-x"
Entonces el borrado no se hace y la rama sigue en el repo
  Y el mensaje coincide con E2 en el idioma del desarrollador: nombra el borrado de "feat-x" y pide volver a borrar la rama
  Y el mensaje no menciona el push, el envío al remoto ni el conjunto mínimo

**Escenario: Un rebase denegado sin el servicio nombra el rebase**

Dado un repo protegido y el servicio de GitRaptor parado
Cuando un agente lanza un rebase de "feat-x"
Entonces el rebase no se ejecuta y la rama queda como estaba
  Y el mensaje coincide con E3: nombra el rebase y pide volver a lanzarlo

**Escenario: Un push denegado sin el servicio sigue pidiendo el push**

Dado un repo protegido y el servicio de GitRaptor parado
Cuando un agente hace un force-push a "main"
Entonces el force-push se deniega como hasta ahora
  Y la pista para seguir pide volver a hacer el push

## Criterios de Certificación

- Verificar con el binario de release, en macOS, que E2 y E3 son exactamente lo que ve el agente en stderr en inglés y en español.
- Confirmar que, con el servicio en marcha y otra causa (plazo agotado, sin espacio en disco), el mensaje también nombra la operación ("delete the branch again in a moment" / "vuelve a borrar la rama en un momento") en lugar de "repeat the operation" / "repite la operación".
- Regresión: US-GRD-017 (sin punto previo no se ejecuta), US-GRD-001 (force-push denegado) y US-TMC-005 (punto previo de los hooks).

## Decisiones

**Decisión del orquestador (2026-10-17), validada por PO.**

1. **Estado**: tres textos según lo que de verdad protege: protegido (hooks activos), protegido con la rama base sin confirmar, y protección incompleta (hooks instalados pero no activos, huérfanos o desaparecidos). "Hooks only" desaparece del texto; el valor de `--json` no cambia.
2. **Rama base en la primera línea**: confirmada → "base branch <nombre>" / "rama base <nombre>"; sin confirmar → "base branch not confirmed (protected meanwhile: <ramas>)" / "rama base sin confirmar (mientras tanto se protegen: <ramas>)". Se retira la línea aparte de la rama base protegida.
3. **Denegación por falta de punto previo**: un único mensaje de dos líneas que nombra la operación (borrado de la rama `<nombre>`, rebase de `<rama>` o, si no se conoce la rama, "the rebase" / "el rebase") y su causa, y una pista que pide repetir esa misma operación. No se escribe el aviso de servicio no disponible con el conjunto mínimo ni la pista del push.
4. **Pista para seguir**: siempre nombra la operación denegada: "push again" / "vuelve a hacer el push", "delete the branch again" / "vuelve a borrar la rama", "run the rebase again" / "vuelve a lanzar el rebase"; para cualquier otra, "run the Git command again" / "vuelve a ejecutar el comando de Git". La frase "a person's push is told apart from an agent's" pasa a "a person's operation is told apart from an agent's" / "la operación de una persona se distingue de la de un agente".
5. **Claves de mensaje propuestas** (el Arquitecto puede renombrarlas en Requisitos Técnicos; `en` y `es`): `guard.state.protected`, `guard.state.protected-unconfirmed`, `guard.state.partial`, `snapshotreq.deny.branch-delete`, `snapshotreq.deny.rebase`, `snapshotreq.deny.rebase-unnamed`, `snapshotreq.hint.start.{push,branch-delete,rebase,other}`, `guard.degraded.retry.{push,branch-delete,rebase,other}`. Se retiran `guard.state.hooks-only`, `guard.status.base-confirmed` y `guard.status.base-unconfirmed`.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** el contrato de salida está en esta ficha (E1 a E3).
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-GRD-017 (denegación sin punto previo, PR #260), US-GRD-001 (force-push), US-TMC-005 (punto previo de los hooks).
- **Externas**: W77 cambia en otra rama los mensajes de `raptor guard install` y `raptor guard uninstall`; esta historia no los toca. Conviene mergear una antes de la otra porque las dos editan los mismos catálogos de mensajes.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.

Implementado en: PR #265.
