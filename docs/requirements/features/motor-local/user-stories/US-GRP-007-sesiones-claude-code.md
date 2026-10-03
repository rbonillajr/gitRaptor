---
id: US-GRP-007
title: "El desarrollador sabe qué sesión de Claude Code trabaja en cada worktree y si sigue activa"
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
    - US-GRP-002
tags:
  - motor-local
  - deteccion-agentes
  - claude-code
---

# US-GRP-007: El desarrollador sabe qué sesión de Claude Code trabaja en cada worktree y si sigue activa

## Descripción

**Como** desarrollador orquestador, **quiero** que el motor detecte solo las sesiones de Claude Code de cada worktree y diga si están activas, inactivas o terminadas, **para** saber qué hace cada Claude Code sin preguntarlo ni registrarlo a mano.

**Valor**: es el soporte completo de agente del MVP (Q32): Claude Code, sin hooks propios.

## Reglas cubiertas

BR-WF-001 · BR-TIME-001 (umbral por defecto de 5 minutos, sin ninguna configuración; el ajuste configurado es de US-GRP-013) · BR-CONS-003 · BR-AUTH-002 · BR-EDGE-006 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-002.
- **Secuencia**: esta historia fija el modelo de sesión (estados, origen, presencia) que reutiliza US-GRP-009; van en serie, 007 antes que 009, y el contrato compartido lo fija la Dev Spec.
- **Externas**: el mecanismo de detección se valida en el spike (c) del BRD; meta de precisión del 90% en dogfooding (Q9).
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Una sesión de Claude Code se detecta sin registrarla**

Dado el repo "demo" observado y ningún registro de agentes
Cuando el desarrollador lanza Claude Code en el worktree "feat-login"
Entonces el estado muestra una sesión de "Claude Code" en "feat-login" en estado "Activo" con origen "detectado"
Cuando Claude Code hace un commit en "feat-login"
Entonces el evento del commit tiene como actor "Claude Code" con origen "detectado"

**Escenario: La sesión pasa a inactiva y vuelve a activa según la actividad**

Dado una sesión de "Claude Code" activa en "feat-login"
  Y ningún nivel de la configuración define un umbral de inactividad
Cuando pasan 5 minutos sin actividad en "feat-login"
Entonces la sesión pasa a "Inactivo"
Cuando se modifica un archivo en "feat-login"
Entonces la sesión vuelve a "Activo"

**Escenario: La sesión termina cuando Claude Code se cierra**

Dado una sesión de "Claude Code" activa en "feat-login"
Cuando Claude Code se cierra
Entonces la sesión pasa a "Terminado"
Cuando Claude Code se vuelve a lanzar en "feat-login"
Entonces aparece una sesión nueva en "Activo" y la anterior sigue en "Terminado"

**Escenario: La detección funciona sin hooks y el motor no instala ninguno**

Dado el repo "demo" sin hooks de Git, o con los hooks de Guardrails desactivados
Cuando el desarrollador lanza Claude Code en "feat-login"
Entonces el motor detecta la sesión de "Claude Code" en "feat-login"
  Y los hooks de Git y la configuración de Git del repo siguen idénticos

**Escenario: Claude Code instalado después se detecta sin tocar GitRaptor**

Dado el repo "demo" observado en una máquina sin Claude Code instalado
  Y el estado de "demo" muestra sus worktrees sin ninguna sesión de agente
Cuando el desarrollador instala Claude Code y lo lanza en "feat-login"
Entonces el motor detecta la sesión de "Claude Code" en "feat-login" sin reinstalar ni reconfigurar GitRaptor
