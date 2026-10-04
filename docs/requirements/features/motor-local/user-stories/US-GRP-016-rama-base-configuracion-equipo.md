---
id: US-GRP-016
title: "La rama base la define la configuración del equipo"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-04
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-012
    - US-GRP-013
    - TS-GRD-001
tags:
  - motor-local
  - rama-base
  - configuracion-tres-niveles
---

# US-GRP-016: La rama base la define la configuración del equipo

## Descripción

**Como** desarrollador orquestador, **quiero** que el ahead/behind de cada worktree se calcule contra la rama base que fija la configuración del repo compartida con el equipo, sin que un ajuste personal la cambie, **para** que todo el equipo mida la separación de cada agente contra la misma rama.

**Valor**: un repo que integra sobre `develop` deja de medirse contra `main`, y nadie descuadra la rama base con un ajuste personal.

## Reglas cubiertas

BR-CONS-006 (rama base del equipo, leída de la copia conocida de la rama principal; `main` sin configuración; ahead/behind contra la rama base confirmada, decisión heredada Q-GRD-21) · BR-CONS-007 (solo el nivel de equipo la admite; el motor no escribe la configuración) · BR-EDGE-007 (en una máquina nueva la configuración del equipo aplica desde el primer momento) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-012 (ahead/behind contra `main`, al que esta historia añade la lectura del valor del equipo); US-GRP-013 (en serie: las dos leen la configuración en tres niveles; va primero 013 y su Dev Spec fija la lectura de la configuración que 016 reutiliza); TS-GRD-001 (Guardrails: lectura commiteada de la configuración del equipo y de la rama principal, de donde sale la rama base).
- **Externas**: ninguna bloqueante desde el 2026-10-04. Historia diferida por Q36 hasta que existieran Guardrails (F-001-04), dueño de la configuración del repo compartida con el equipo, y el ADR de formato de la configuración en tres niveles (P8). Los dos existen como ADRs aceptados por Rene Bonilla el 2026-10-04 (ADR-GRD-004 y ADR-GRP-007, que cierra P8), y Rene Bonilla confirmó ese día su desbloqueo con dependencia de US-GRP-013 y TS-GRD-001. Confirmar la rama base es una acción de Guardrails; esta historia no depende de cómo se confirma: parte de la rama base confirmada como precondición y muestra lo pendiente o no confirmado (decisiones heredadas Q-GRD-20, Q-GRD-21 y Q-GRD-23). La coherencia con Guardrails se comprueba en la prueba de integración posterior del [índice de historias de Guardrails](../../guardrails/user-stories.md#relación-con-us-grp-016-motor-local).
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Esquema del escenario: La rama base sale solo de la configuración del equipo**

Dado el repo "demo" observado con las ramas "main", "develop" y "release"
  Y la configuración del equipo de "demo" en la rama principal con rama base "<equipo>", el perfil con rama base "<perfil>" y la configuración local personal de "demo" con rama base "<local>"
  Y la rama base confirmada de "demo" es la que resulta de esa configuración
Cuando se consulta el estado del motor
Entonces la rama base de "demo" es "<efectiva>"
  Y el ahead/behind de cada worktree de "demo" se calcula contra "<efectiva>"

Ejemplos:
| equipo | perfil | local | efectiva |
| develop | sin definir | sin definir | develop |
| develop | sin definir | release | develop |
| develop | release | sin definir | develop |
| sin definir | develop | sin definir | main |
| sin definir | sin definir | release | main |
| sin definir | sin definir | sin definir | main |

**Escenario: Un cambio de rama base en la rama principal queda pendiente de confirmar**

Dado el repo "demo" con la rama base confirmada "develop"
Cuando llega a la copia conocida de la rama principal un cambio de la configuración del equipo que fija la rama base "release"
Entonces el ahead/behind de cada worktree de "demo" se sigue calculando contra "develop"
  Y el estado del motor muestra "release" como rama base pendiente de confirmar
  Y cuando la rama base confirmada de "demo" pasa a ser "release", el ahead/behind se calcula contra "release" sin reinstalar GitRaptor
  Y los tres niveles de configuración contienen solo lo que escribió el desarrollador

**Escenario: Un cambio que no está en la copia conocida de la rama principal no cuenta**

Dado el repo "demo" con la rama base confirmada "develop", que también define la copia conocida del remoto para su rama principal
Cuando el desarrollador cambia la rama base a "release" en el worktree principal de "demo", con o sin commit, sin que el cambio llegue a esa copia
Entonces la rama base de "demo" sigue siendo "develop"
  Y el estado del motor no muestra ningún cambio de rama base pendiente

**Escenario: Sin confirmación inicial, el ahead/behind se marca como no confirmado**

Dado el repo "demo" observado, cuya configuración del equipo en la rama principal define la rama base "develop"
  Y "demo" no tiene rama base confirmada
Cuando se consulta el estado del motor
Entonces el ahead/behind de cada worktree de "demo" se calcula contra "develop"
  Y el estado del motor marca esa rama base como "no confirmada"

**Escenario: En una máquina nueva la rama base del equipo aplica desde el primer momento**

Dado una máquina nueva con un perfil de GitRaptor vacío
  Y el repo "demo" recién clonado, cuya configuración del equipo define la rama base "develop"
Cuando el desarrollador añade "demo"
Entonces la rama base de "demo" es "develop" desde la primera consulta del estado
  Y el estado del motor la marca como "no confirmada" mientras "demo" no tenga rama base confirmada
