---
id: US-GRP-005
title: "El desarrollador distingue lo que el motor no vio ocurrir"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-004
    - US-GRP-009
tags:
  - motor-local
  - continuidad
  - atribucion
---

# US-GRP-005: El desarrollador distingue lo que el motor no vio ocurrir

## Descripción

**Como** desarrollador orquestador, **quiero** que los cambios ocurridos mientras el motor no observaba queden "sin atribuir" y el periodo quede señalado, **para** que un deshacer por agente nunca alcance algo que nadie vio hacer a ese agente.

**Valor**: la atribución solo afirma lo que el motor vio.

## Reglas cubiertas

BR-EDGE-005 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-004; US-GRP-009 (agente registrado antes del hueco). No depende de US-GRP-015: tras perder el perfil solo se exige la lista de repos vacía, no el estado "Sin repos" con su guía.
- **Externas**: ninguna.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Los cambios de un hueco se reconcilian y quedan sin atribuir**

Dado el repo "demo" observado con "Claude Code" registrado en el worktree "feat-login"
  Y el motor deja de ejecutarse
Cuando, mientras tanto, se hacen 2 commits en la rama "feat-login" y se crea la rama "hotfix"
  Y el motor vuelve a arrancar
Entonces el estado de "feat-login" refleja los 2 commits nuevos y la rama "hotfix" existe en el estado
  Y los 2 commits figuran como "sin atribuir", no como de "Claude Code"

**Escenario: El hueco queda señalado**

Dado que el repo "demo" tuvo un periodo sin observar mientras el motor no se ejecutaba
Cuando se consulta el historial de eventos de "demo"
Entonces ese periodo figura como hueco de observación, sin atribución a ningún agente

**Escenario: Perder el perfil no detiene el motor**

Dado el repo "demo" observado con historial y atribuciones
Cuando se borra el perfil de GitRaptor
Entonces el motor sigue funcionando
  Y la lista de repos observados está vacía
Cuando el desarrollador vuelve a añadir "demo"
Entonces el estado refleja el estado actual de "demo"
  Y los commits anteriores a volver a añadirlo no tienen ningún agente atribuido
