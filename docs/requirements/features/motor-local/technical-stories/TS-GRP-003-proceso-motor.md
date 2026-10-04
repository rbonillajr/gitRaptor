---
id: TS-GRP-003
title: "Proceso del motor en segundo plano por usuario"
type: ts
status: Dev Spec Pending
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-009]
  stories: [US-GRP-001, US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-014, US-GRP-015, TS-GRP-001, TS-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, daemon, ciclo-de-vida, instancia-unica, continuidad]
---

## TS-GRP-003: Proceso del motor en segundo plano por usuario

**Valor**: la observación existe aunque no haya ninguna superficie abierta, con un solo proceso y un solo escritor del perfil por usuario.

### Descripción

**Como** Arquitecto
**Quiero** el proceso único del motor por usuario con su ciclo de vida completo
**Para** observar sin huecos mientras la máquina está encendida (BR-CONS-005, Q1) sin duplicar watchers ni escritores

> Dev Spec: `dev-specs/TS-GRP-003-proceso-motor.md` | Pendiente
>
> **Depende de**: TS-GRP-001 (perfil y almacén), TS-GRP-002 (resolución de Git al arrancar). **ADRs**: ADR-GRP-005 (§ 1, § 2 y § 4), ADR-GRP-006, ADR-GRP-009 § 4.
>
> **Complejidad alta**: ruta crítica de todas las historias, tres SO y riesgo de pérdida de datos si la parada no es ordenada.

### Alcance Técnico

- **Crear** el subcomando `raptor daemon` en `apps/cli` como punto de entrada del motor de `crates/core`, sin app nueva (ADR-GRP-005 § 1, PQ-5).
- **Implementar** la instancia única con un bloqueo del SO en la carpeta de estado del perfil; un segundo daemon termina sin observar ni tocar el almacén.
- **Implementar** el arranque: bloqueo, apertura del perfil, resolución de Git y entrada en el estado de BR-WF-002 que corresponda ("Esperando Git", "Sin repos" u "Observando").
- **Implementar** la parada ordenada ante cierre de sesión, `raptor daemon stop` o señal de terminación: vaciar lo pendiente, persistir "observado hasta" y liberar el bloqueo.
- **Implementar** los logs del daemon en la carpeta de estado, con rotación y sin contenido de archivos del usuario.
- **Garantizar** que el daemon corre con los privilegios del usuario y no depende del PATH de la shell (arranque con entorno mínimo).
- **Fuera de alcance**: el canal, el handshake y el arranque bajo demanda desde los clientes (TS-GRP-004); el registro del autoarranque con `raptor daemon enable` y `disable` (US-GRP-004); la reconciliación y el registro de huecos al arrancar (US-GRP-005); el observador (US-GRP-002).

### Plan de Verificación

#### Pruebas Automatizadas

- **Instancia única**: dos `raptor daemon` simultáneos con el mismo perfil temporal; uno termina sin observar y el almacén solo recibe escrituras del otro.
- **Estados iniciales**: sin repos → "Sin repos"; con resolución de Git simulada como ausente → "Esperando Git"; con repos y Git válido → "Observando".
- **Parada ordenada**: tras `raptor daemon stop` o la señal de terminación, la marca "observado hasta" está persistida y el bloqueo liberado.
- **Caída**: tras matar el daemon a la fuerza, un daemon nuevo obtiene el bloqueo sin intervención manual.
- **Entorno mínimo**: el daemon arranca con un PATH vacío y llega al estado correcto.
- **Logs**: con contenido marcado en los archivos de un repo observado, ese contenido no aparece en los logs; los logs rotan al superar su límite.
- **Repo intacto**: todos los escenarios pasan por el arnés de INF-GRP-001.

#### Verificación Manual / Sandbox

- Arrancar y parar el daemon en los tres SO desde una terminal y desde el entorno de sesión, y revisar estado, bloqueo y logs en el perfil.
