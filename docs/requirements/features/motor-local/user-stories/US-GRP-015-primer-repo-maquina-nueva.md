---
id: US-GRP-015
title: "El desarrollador recién instalado sabe cómo añadir su primer repo"
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
    - US-GRP-001
    - US-GRP-002
tags:
  - motor-local
  - primer-uso
---

# US-GRP-015: El desarrollador recién instalado sabe cómo añadir su primer repo

## Descripción

**Como** desarrollador que acaba de instalar GitRaptor, **quiero** que sin repos el motor me diga cómo añadir el primero y que en una máquina nueva empiece de cero sin inventar historia, **para** que mi primer minuto termine con un repo observado.

**Valor**: el estado vacío guía en lugar de parecer un fallo.

## Reglas cubiertas

BR-WF-002 (estado "Sin repos") · BR-EDGE-007 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001, US-GRP-002.
- **Secuencia**: fija los estados "Sin repos" y "Observando" de BR-WF-002; US-GRP-014 va después y añade "Esperando Git". No van en paralelo; el contrato compartido lo fija la Dev Spec.
- **Externas**: el Cockpit (F-001-02) y la CLI presentan el estado vacío guiado. Que la rama base del equipo aplique desde el primer momento en una máquina nueva es de US-GRP-016 (desbloqueada el 2026-10-04); no bloquea esta historia.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Sin repos, el motor expone la guía para añadir el primero**

Dado Git 2.45 instalado y un perfil de GitRaptor sin repos
Cuando arranca el motor
Entonces el estado del motor es "Sin repos"
  Y expone que no hay repos observados y cómo añadir el primero

**Escenario: Añadir el primer repo pasa a observar**

Dado el motor en "Sin repos"
Cuando el desarrollador añade el repo "demo"
Entonces el estado del motor es "Observando" e incluye "demo"

**Escenario: Retirar el último repo vuelve a "Sin repos"**

Dado el motor en "Observando" con "demo" como único repo
Cuando el desarrollador retira "demo"
Entonces el estado del motor es "Sin repos" con la guía para añadir uno

**Escenario: En una máquina nueva la historia previa no se atribuye a nadie**

Dado una máquina nueva con un perfil de GitRaptor vacío
  Y el repo "demo" recién clonado con 50 commits previos, algunos hechos con Claude Code en otra máquina
Cuando el desarrollador añade "demo"
Entonces el estado refleja el estado actual de "demo" y su historia de Git
  Y ninguno de los 50 commits previos tiene un agente atribuido
  Y el perfil no contiene datos de ninguna otra máquina
