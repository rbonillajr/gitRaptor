---
id: US-GRP-013
title: "El desarrollador ajusta para un repo cuándo una sesión pasa a inactiva"
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
    - US-GRP-007
tags:
  - motor-local
  - configuracion-tres-niveles
  - estados-sesion
---

# US-GRP-013: El desarrollador ajusta para un repo cuándo una sesión pasa a inactiva

## Descripción

**Como** desarrollador orquestador, **quiero** fijar mi umbral de inactividad en mi perfil y ajustarlo por repo en mi configuración local, sin que el equipo me lo imponga, **para** que "inactivo" signifique lo mismo que mi ritmo de trabajo en cada repo.

**Valor**: el estado de sesión deja de dar falsas alarmas en repos lentos.

## Reglas cubiertas

BR-TIME-001 (umbral configurado; el valor por defecto de 5 minutos sin configuración es de US-GRP-007) · BR-CONS-007 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-007.
- **Externas**: ninguna. Estuvo bloqueada por P8 (formato y ubicación de la configuración en tres niveles) hasta el 2026-10-04, cuando Rene Bonilla aceptó ADR-GRP-007, que la cierra.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Esquema del escenario: El umbral efectivo sale del nivel más específico que lo admite**

Dado el perfil con umbral "<perfil>", la configuración local personal de "demo" con umbral "<local>" y la configuración del equipo de "demo" con umbral "<equipo>"
Cuando una sesión de "Claude Code" en "demo" no tiene actividad
Entonces pasa a "Inactivo" a los "<efectivo>" sin actividad

Ejemplos:
| perfil | local | equipo | efectivo |
| 10 minutos | sin definir | sin definir | 10 minutos |
| 10 minutos | 15 minutos | sin definir | 15 minutos |
| sin definir | sin definir | 30 minutos | 5 minutos |
| 10 minutos | sin definir | 30 minutos | 10 minutos |

**Escenario: El ajuste de un repo no afecta a los demás**

Dado el perfil con umbral de 10 minutos y la configuración local personal de "demo" con 15 minutos
Cuando se consulta el umbral efectivo de "demo" y de "otro"
Entonces el de "demo" es 15 minutos y el de "otro" es 10 minutos

**Escenario: Un cambio en la configuración se aplica sin que el motor la escriba**

Dado la configuración local personal de "demo" con umbral de 15 minutos
Cuando el desarrollador cambia ese valor a 20 minutos
Entonces el umbral efectivo de "demo" pasa a 20 minutos sin reinstalar GitRaptor
  Y los tres niveles de configuración contienen solo lo que escribió el desarrollador
