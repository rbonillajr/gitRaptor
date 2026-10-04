---
id: US-GRD-001
title: "Un agente que intenta hacer force-push en un repo protegido queda bloqueado"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRP-001
    - US-GRP-012
tags:
  - guardrails
  - esqueleto-andante
  - hooks-git
  - minimo-seguro
  - force-push
---

# US-GRD-001: Un agente que intenta hacer force-push en un repo protegido queda bloqueado

## Descripción

**Como** desarrollador orquestador, **quiero** proteger un repo con mi permiso explícito y que, aunque el equipo no haya configurado nada, un force-push o el borrado de la rama base queden denegados con su motivo, **para** que ningún agente reescriba ni borre `main` aunque use Git directo.

**Valor**: la demo del BRD (§ 13): un agente intenta un force-push y queda bloqueado, con la regla explicada.

## Reglas cubiertas

BR-AUTH-002 (instalar solo con permiso explícito; sin repreguntar tras denegar) · BR-EDGE-001 (conjunto mínimo seguro sin configuración, Q-GRD-5) · BR-CALC-001 (la decisión dice qué regla y de qué nivel) · BR-WF-002 (Sin protección → Solo hooks) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001 (motor-local: repo observado; Q-GRD-15) y US-GRP-012 (motor-local: rama base `main`).
- **Externas**: ninguna bloqueante. No necesita el ADR P8 (motor-local): sin configuración aplica el mínimo seguro.
- **Transversal**: los escenarios se cumplen igual en Windows, macOS y Linux; instalar no toca nada fuera del repo (NFR-01, Q17 de motor-local). Cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: El desarrollador protege un repo con su permiso**

Dado el repo "demo" observado, sin configuración de Guardrails, sin hooks y en estado "Sin protección"
Cuando el desarrollador pide proteger "demo"
Entonces GitRaptor le explica qué se instala, en qué repo, por qué y cómo se revierte
  Y al conceder el permiso el estado de protección de "demo" pasa a "Solo hooks"

**Escenario: Un force-push queda denegado por el mínimo seguro**

Dado el repo "demo" en estado "Solo hooks" y sin configuración de Guardrails
Cuando un proceso hace force-push de la rama "feat-x" con Git directo
Entonces la operación no se ejecuta
  Y el motivo nombra la regla "prohibir force-push" del conjunto mínimo por defecto
  Y la rama "feat-x" del remoto no cambia

**Escenario: Borrar la rama base queda denegado**

Dado el repo "demo" en estado "Solo hooks" con rama base "main"
Cuando un proceso intenta borrar la rama "main" con Git directo
Entonces la operación no se ejecuta y el motivo nombra la protección de la rama base
  Y la rama "main" sigue existiendo

**Escenario: Una operación fuera del mínimo seguro se permite**

Dado el repo "demo" en estado "Solo hooks" y sin configuración de Guardrails
Cuando un proceso hace commit en la rama "feat-x"
Entonces el commit se ejecuta con normalidad

**Escenario: Sin permiso no se instala nada ni se vuelve a preguntar**

Dado el repo "demo" observado y en estado "Sin protección"
Cuando el desarrollador deniega el permiso, o no responde
Entonces las rutas operativas de "demo" quedan idénticas a como estaban
  Y "demo" sigue en "Sin protección"
  Y GitRaptor no vuelve a pedir ese permiso hasta que el desarrollador lo active a mano

**Escenario: El permiso de un repo no alcanza a otro**

Dado los repos observados "demo" y "otro", ninguno protegido
Cuando el desarrollador concede el permiso para "demo"
Entonces "otro" sigue en "Sin protección" y sin ningún cambio en sus rutas operativas

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (sin superficie propia; la presentación es del Cockpit y la CLI).
- **Dev Spec:** pendiente (Arquitecto).
