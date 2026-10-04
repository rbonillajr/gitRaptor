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
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-013]
  stories: [US-GRP-001, US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-014, US-GRP-015, TS-GRP-001, TS-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, daemon, ciclo-de-vida, instancia-unica, continuidad, seguridad]
---

## TS-GRP-003: Proceso del motor en segundo plano por usuario

**Valor**: la observación existe aunque no haya ninguna superficie abierta, con un solo proceso y un solo escritor del perfil por usuario.

### Descripción

**Como** Arquitecto
**Quiero** el proceso único del motor por usuario con su ciclo de vida completo
**Para** observar sin huecos mientras la máquina está encendida (BR-CONS-005, Q1) sin duplicar watchers ni escritores

> Dev Spec: `dev-specs/TS-GRP-003-proceso-motor.md` | Pendiente
>
> **Depende de**: TS-GRP-001 (perfil y almacén), TS-GRP-002 (resolución de Git al arrancar). **ADRs**: ADR-GRP-005 (§ 1, § 2 y § 4), ADR-GRP-006, ADR-GRP-009 § 4, ADR-GRP-013 § 1 y § 5 (causa de los huecos). **Seguridad**: SEC-05, SEC-06, SEC-10 y SEC-13; la Dev Spec queda bloqueada hasta que se cumplan las condiciones del gate de seguridad de `docs/architecture/non-functional.md`.
>
> **Complejidad alta**: ruta crítica de todas las historias, tres SO y riesgo de pérdida de datos si la parada no es ordenada.

### Alcance Técnico

- **Crear** el subcomando `raptor daemon` en `apps/cli` como punto de entrada del motor de `crates/core`, sin app nueva (ADR-GRP-005 § 1, PQ-5).
- **Implementar** la instancia única con un bloqueo del SO en la carpeta de estado del perfil; un segundo daemon termina sin observar ni tocar el almacén.
- **Implementar** el arranque: bloqueo, apertura del perfil, resolución de Git y entrada en el estado de BR-WF-002 que corresponda ("Esperando Git", "Sin repos" u "Observando").
- **Implementar** la parada ordenada ante cierre de sesión, `raptor daemon stop` o señal de terminación: vaciar lo pendiente, persistir "observado hasta" con la causa de la parada y liberar el bloqueo (SEC-13).
- **Marcar** al arrancar si la parada anterior fue una caída con alguna sesión activa, para que el hueco lo refleje (SEC-13).
- **Implementar** los logs del daemon en la carpeta de estado, con rotación y sin contenido de archivos, valores de config, entorno ni argv de terceros; el panic hook redacta igual (SEC-05).
- **Garantizar** que el daemon corre con los privilegios del usuario y no depende del PATH de la shell (arranque con entorno mínimo).
- **Ignorar** en el daemon cualquier variable de entorno hostil heredada (`GIT_*`, `LD_PRELOAD`, `DYLD_*`, `XDG_CONFIG_HOME`, PATH relativo) para todo lo que no sea el arranque del propio proceso (SEC-10).
- **Fuera de alcance**: el canal, el handshake y el arranque bajo demanda desde los clientes (TS-GRP-004); el registro del autoarranque con `raptor daemon enable` y `disable` y su endurecimiento SEC-14 (US-GRP-004); la autorización de `raptor daemon stop` como comando reservado (TS-GRP-004); la reconciliación y el registro de huecos al arrancar (US-GRP-005); el observador (US-GRP-002).

### Plan de Verificación

#### Pruebas Automatizadas

- **Instancia única**: dos `raptor daemon` simultáneos con el mismo perfil temporal; uno termina sin observar y el almacén solo recibe escrituras del otro.
- **Estados iniciales**: sin repos → "Sin repos"; con resolución de Git simulada como ausente → "Esperando Git"; con repos y Git válido → "Observando".
- **Parada ordenada**: tras `raptor daemon stop` o la señal de terminación, la marca "observado hasta" está persistida y el bloqueo liberado.
- **Caída**: tras matar el daemon a la fuerza, un daemon nuevo obtiene el bloqueo sin intervención manual; con una sesión activa simulada, el hueco queda como "caída durante sesión activa" (SEC-13).
- **Entorno mínimo**: el daemon arranca con un PATH vacío y llega al estado correcto.
- **Logs (SEC-05)**: con contenido marcado en los archivos de un repo observado y secretos plantados en el entorno, nada de eso aparece en los logs ni en un volcado de pánico; los logs rotan al superar su límite.
- **Entorno hostil (SEC-10)**: arrancado con `GIT_EXEC_PATH`, `LD_PRELOAD`/`DYLD_INSERT_LIBRARIES`, `PATH=.:…` o `XDG_CONFIG_HOME` hostiles, el daemon no cambia de comportamiento.
- **Repo intacto**: todos los escenarios pasan por el arnés de INF-GRP-001.

#### Verificación Manual / Sandbox

- Arrancar y parar el daemon en los tres SO desde una terminal y desde el entorno de sesión, y revisar estado, bloqueo y logs en el perfil.
