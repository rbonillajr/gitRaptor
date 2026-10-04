---
id: US-GRD-003
title: "El desarrollador retira la protección y el repo queda exactamente como estaba"
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
    - US-GRD-001
    - US-GRD-002
tags:
  - guardrails
  - hooks-git
  - desinstalacion
  - nfr-01
---

# US-GRD-003: El desarrollador retira la protección y el repo queda exactamente como estaba

## Descripción

**Como** desarrollador orquestador, **quiero** desinstalar la protección de hooks de un repo y recuperar su estado anterior exacto, también si la instalación se interrumpe, **para** probar Guardrails sin miedo a dejar el repo distinto de como lo encontré.

**Valor**: instalar y retirar la protección es reversible del todo (NFR-01).

## Reglas cubiertas

BR-CONS-005 (desinstalar = estado anterior; instalación nunca a medias; solo dentro del repo) · BR-WF-002 (Solo hooks → Sin protección) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (instalar), US-GRD-002 (hooks previos que hay que restaurar).
- **Externas**: ninguna.
- **Transversal**: Windows, macOS y Linux; pruebas de interrupción (NFR-12).

## Criterios de Aceptación

**Escenario: Desinstalar restaura el estado exacto**

Dado el repo "demo" con un hook propio de linter, protegido después con permiso
Cuando el desarrollador retira la protección de "demo"
Entonces las rutas operativas de "demo" son idénticas a las de antes de protegerlo
  Y el hook propio de linter sigue funcionando
  Y el estado de protección de "demo" pasa a "Sin protección"

**Escenario: Tras retirar la protección, el mínimo seguro deja de aplicarse con Git directo**

Dado el repo "demo" con la protección retirada y sin allowlist del MCP
Cuando un proceso hace force-push con Git directo
Entonces Guardrails no evalúa la operación

**Escenario: Una instalación interrumpida no deja el repo a medias**

Dado el repo "demo" sin proteger
Cuando el proceso de instalación se interrumpe a mitad
Entonces "demo" queda con la protección completa o exactamente como estaba antes
  Y nunca con una protección parcial

**Escenario: Nada cambia fuera del repo**

Dado los repos "demo" y "otro" y la configuración global de Git del usuario
Cuando el desarrollador protege "demo" y después retira la protección
Entonces la configuración global de Git y el repo "otro" no cambian en ningún momento

**Escenario: La instalación queda registrada**

Dado el repo "demo" sin proteger
Cuando el desarrollador lo protege y después retira la protección
Entonces cada cambio queda registrado con qué se hizo, en qué repo, cuándo y quién lo autorizó

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
