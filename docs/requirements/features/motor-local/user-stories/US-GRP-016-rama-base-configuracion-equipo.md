---
id: US-GRP-016
title: "La rama base la define la configuración del equipo"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-012
    - US-GRP-013
tags:
  - motor-local
  - rama-base
  - configuracion-tres-niveles
  - bloqueada
---

# US-GRP-016: La rama base la define la configuración del equipo

## Descripción

**Como** desarrollador orquestador, **quiero** que el ahead/behind de cada worktree se calcule contra la rama base que fija la configuración del repo compartida con el equipo, sin que un ajuste personal la cambie, **para** que todo el equipo mida la separación de cada agente contra la misma rama.

**Valor**: un repo que integra sobre `develop` deja de medirse contra `main`, y nadie descuadra la rama base con un ajuste personal.

## Reglas cubiertas

BR-CONS-006 (rama base del equipo; `main` sin configuración) · BR-CONS-007 (solo el nivel de equipo la admite; el motor no escribe la configuración) · BR-EDGE-007 (en una máquina nueva la configuración del equipo aplica desde el primer momento) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-012 (ahead/behind contra `main`, al que esta historia añade la lectura del valor del equipo); US-GRP-013 (en serie: las dos leen la configuración en tres niveles, bloqueadas por P8; va primero 013, que solo espera al ADR de formato, y su Dev Spec fija la lectura de la configuración que 016 reutiliza).
- **Externas**: **bloqueada** por Guardrails (F-001-04), dueño de la configuración del repo compartida con el equipo, y por el ADR de formato de la configuración en tres niveles (P8). Historia diferida por Q36: no se empieza hasta que existan los dos.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Esquema del escenario: La rama base sale solo de la configuración del equipo**

Dado el repo "demo" observado con las ramas "main", "develop" y "release"
  Y la configuración del equipo de "demo" con rama base "<equipo>", el perfil con rama base "<perfil>" y la configuración local personal de "demo" con rama base "<local>"
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

**Escenario: Un cambio en la configuración del equipo se aplica sin que el motor la escriba**

Dado la configuración del equipo de "demo" con rama base "develop"
Cuando el desarrollador cambia ese valor a "release"
Entonces la rama base de "demo" pasa a ser "release" sin reinstalar GitRaptor
  Y los tres niveles de configuración contienen solo lo que escribió el desarrollador

**Escenario: En una máquina nueva la rama base del equipo aplica desde el primer momento**

Dado una máquina nueva con un perfil de GitRaptor vacío
  Y el repo "demo" recién clonado, cuya configuración del equipo define la rama base "develop"
Cuando el desarrollador añade "demo"
Entonces la rama base de "demo" es "develop" desde la primera consulta del estado
