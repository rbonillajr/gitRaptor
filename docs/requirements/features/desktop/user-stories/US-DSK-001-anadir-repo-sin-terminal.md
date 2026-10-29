---
id: US-DSK-001
title: "El novato añade su repo a GitRaptor desde la app sin abrir una terminal"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-GRP-001
    - US-GRP-015
    - US-GRP-020
    - US-GRP-022
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - onboarding
  - must
---

# US-DSK-001: El novato añade su repo a GitRaptor desde la app sin abrir una terminal

## Descripción

**Como** desarrollador novato que trabaja con agentes, **quiero** elegir una carpeta de mi disco o un repo que GitRaptor ya encontró y que pase a estar observado, **para** tener historial y "Deshacer" de todo lo que pase en él desde el primer minuto.

**Valor**: BR-27 (UI-1, Must); BR-01. Nivel **UI-1**.

## Reglas

- Solo se añade la raíz de un repo o de una copia de trabajo; una carpeta que no es un repo da un error que dice qué elegir (US-GRP-001).
- Añadir un repo es un comando reservado al humano (ADR-GRP-005 § 6): la app no lo puede hacer con menos control que la terminal (Q-DSK-3).
- Un repo ya observado no se duplica: la app lo abre.

## Dependencias

- **Reutiliza**: US-GRP-001 (añadir y ver el estado), US-GRP-015 (primer repo tras instalar), US-GRP-020 y US-GRP-022 (repos descubiertos); `repo.add`, `repo.locate`, `scope.snapshot`.
- **Qué falta en el motor**: DEP-DSK-1, la prueba de presencia humana para un comando reservado lanzado desde la app (sin ella, la app muestra y deja copiar `raptor repo add <ruta>` y detecta el repo cuando aparece); DEP-DSK-2, la identidad de la app ante el daemon. Ambas transversal (lo define el Arquitecto).
- **Decisión pendiente**: Q-DSK-15.

## Criterios de Aceptación

**Escenario: Añadir un repo elegido por el usuario**

Dado que el motor está en marcha y "~/code/tienda" es un repo no observado
Cuando el usuario elige "~/code/tienda" y confirma que es él quien lo pide
Entonces "~/code/tienda" queda observado
  Y el usuario ve sus copias de trabajo con su estado

**Escenario: Añadir un repo descubierto**

Dado que GitRaptor descubrió "~/code/api" en las carpetas de código del usuario
Cuando el usuario decide observarlo
Entonces "~/code/api" queda observado y deja de aparecer como descubierto

**Escenario: La carpeta no es un repo**

Dado la carpeta "~/Descargas" sin repositorio Git
Cuando el usuario intenta añadirla
Entonces no se observa nada
  Y el usuario sabe que debe elegir la carpeta raíz de un repo

**Escenario: Sin prueba de presencia humana disponible**

Dado que la app no puede demostrar al motor que el usuario es una persona
Cuando el usuario intenta añadir "~/code/tienda"
Entonces el repo no se añade desde la app
  Y el usuario recibe el comando exacto que debe ejecutar en una terminal
  Y cuando lo ejecuta, el repo aparece como observado sin reiniciar la app

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), flujo de onboarding.
- **Dev Spec:** pendiente.
