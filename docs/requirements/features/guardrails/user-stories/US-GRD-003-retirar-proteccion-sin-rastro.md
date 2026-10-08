---
id: US-GRD-003
title: "El desarrollador retira la protección y el repo queda exactamente como estaba"
type: us
status: partially-implemented
priority: high
created: 2026-10-04
updated: 2026-10-08
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

BR-CONS-005 (desinstalar = estado anterior; instalación nunca a medias; solo dentro del repo; instalación huérfana: retirarla o adoptarla) · BR-WF-002 (Solo hooks → Sin protección) · BR-AUTH-001 (desinstalar es una acción reservada con anuncio, ventana para cancelar y auditoría, Q-GRD-19) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (instalar), US-GRD-002 (hooks previos que hay que restaurar), US-GRD-004 (detecta la instalación huérfana que esta historia adopta o retira).
- **Externas**: ninguna.
- **Transversal**: Windows, macOS y Linux; pruebas de interrupción (NFR-12).

## Criterios de Aceptación

**Escenario: Desinstalar restaura el estado exacto**

Dado el repo "demo" con un hook propio de linter, protegido después con permiso
Cuando el desarrollador retira la protección de "demo"
Entonces GitRaptor anuncia la retirada y abre una ventana en la que se puede cancelar
  Y al cerrarse la ventana sin cancelación, las rutas operativas de "demo" quedan como antes de protegerlo, con las únicas diferencias de formato que introduce Git, que están declaradas
  Y el hook propio de linter sigue funcionando
  Y el estado de protección de "demo" pasa a "Sin protección"

**Escenario: Tras retirar la protección, el mínimo seguro deja de aplicarse con Git directo**

Dado el repo "demo" con la protección retirada y sin allowlist del MCP
Cuando un proceso hace force-push con Git directo
Entonces Guardrails no evalúa la operación

**Escenario: Una instalación interrumpida no deja el repo a medias**

Dado el repo "demo" sin proteger
Cuando el proceso de instalación se interrumpe a mitad
Entonces "demo" queda con la protección completa o como estaba antes, con las únicas diferencias de formato que introduce Git, que están declaradas
  Y nunca con una protección parcial

**Escenario: Nada cambia fuera del repo**

Dado los repos "demo" y "otro" y la configuración global de Git del usuario
Cuando el desarrollador protege "demo" y después retira la protección
Entonces la configuración global de Git y el repo "otro" no cambian en ningún momento

**Esquema del escenario: Una protección que perdió su registro se retira o se adopta**

Dado el repo "demo" con la protección instalada y el perfil de GitRaptor perdido, sin registro de esa instalación
Cuando el desarrollador decide "<acción>" esa protección
Entonces "demo" queda "<resultado>"

Ejemplos:
| acción | resultado |
| retirar | con las rutas operativas como estaban antes de instalarla, en "Sin protección" |
| adoptar | en "Solo hooks", con la rama base no confirmada |

**Escenario: La instalación queda registrada**

Dado el repo "demo" sin proteger
Cuando el desarrollador lo protege y después retira la protección
Entonces cada cambio queda registrado con qué se hizo, en qué repo, cuándo y quién lo autorizó

## Requisitos Técnicos

- **Criterio de "como estaba"** (Q-GRD-29; SPIKE-GRD-001 § 5.1; ADR-GRD-001 § 4, Enmienda 2026-10-04): semántico en la entrada de la clave de hooks (mismo valor efectivo y nivel; demás entradas sin cambios); byte a byte en el resto de rutas. Git reescribe una línea escrita a mano y añade el salto de línea final: esas diferencias se declaran. El título de la historia se mantiene porque BR-CONS-005 define qué significa "exacto".

- **Gobierno**: ADR-GRD-001 § 4 (desinstalación en orden inverso, borrado solo de lo que lista el diario y recuperación al arrancar el daemon) y § 7 (solo el módulo `guardrails` alcanza la capa de escritura); ADR-GRD-005 § 1 (instalación huérfana); ADR-GRD-007 § 1 y § 2 (comandos reservados).
- **Acciones reservadas**: desinstalar y retirar una huérfana relajan y usan D5 (Q-GRD-19), con anuncio, ventana cancelable, auditoría completa y aceptación de riesgo por acción. Adoptar no relaja y no lleva ventana (ADR-GRD-007 § 1 y § 2).
- **Instalación huérfana** (registro del perfil perdido): retirar desinstala con los valores del manifiesto, que se muestran antes. Adoptar regenera los dispatchers con las constantes de la instancia actual solo si los del disco coinciden con los esperados; si no, solo se ofrece retirar (ADR-GRD-005 § 1; ADR-GRD-001 § 8).
- **Adoptar no confirma** la rama base ni el suelo: quedan "no confirmados", con la unión protegida, hasta una confirmación explícita (BR-CONS-003; Q-GRD-23; ADR-GRD-007 § 1; ADR-GRD-004 § 3, punto 5). La transición esperada del diario evita el aviso de pérdida; instalar, desinstalar, adoptar y retirar van al registro como `protection-state` y a la auditoría permanente (ADR-GRD-005 § 5; ADR-GRD-006 § 1 y § 4).
- **Crates**: `crates/core` módulo `guardrails` (transacción, recuperación, detección y adopción de huérfanas), `crates/git` capa de escritura de Guardrails, `crates/api` (comandos reservados) y `apps/cli` (desinstalar, adoptar y retirar).
- **Enablers**: la suite de interrupción de INF-GRD-001 **bloquea el merge**. SPIKE-GRD-001 no bloquea, pero fija el criterio de comparación del `config` del repo, byte a byte o semántico (NFR-GRD-01). TS-GRD-001 no aplica.
- **NFR, SEC y verificación**: NFR-GRD-01, 02, 03 y 10; SEC-GRD-02, 05, 10, 12 y 16. ADR-GRD-001 Validación 1, 3, 8 y 12; ADR-GRD-005 Validación 4 y 6; ADR-GRD-006 Validación 9; ADR-GRD-007 Validación 3 a 5. Sin allowlist del MCP el estado final es `unprotected` (ADR-GRD-005 § 2).
- **Enmiendas**: en motor-local, ADR-GRP-005 § 6 con SEC-03 (desinstalar, adoptar y retirar como comandos reservados), ADR-GRP-006 § 4 (id de instancia del perfil), ADR-GRP-013 § 1 e INF-GRP-001 (cero diferencias tras desinstalar). ADR-GRD-007 § 1 asigna las huérfanas a esta historia y fija que adoptar no confirma.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-003](../dev-specs/US-GRD-003-retirar-proteccion-sin-rastro.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #193 (parcial): E1 a E4 y la ventana cancelable de D5. **Falta**: E5 (instalación huérfana: retirar o adoptar, tras US-GRD-004) y E6 (entradas `protection-state` en el registro de decisiones; hoy queda en la auditoría permanente). Dev Spec: [DS-US-GRD-003](../dev-specs/US-GRD-003-retirar-proteccion-sin-rastro.md).
