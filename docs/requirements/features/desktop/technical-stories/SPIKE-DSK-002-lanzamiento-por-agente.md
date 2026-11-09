---
id: SPIKE-DSK-002
title: "Detectar que un agente lanzó la app de escritorio con open -a (evento oapp y keySenderPIDAttr)"
type: spike
status: ready
feature: desktop
domain: GRP
priority: medium
complexity: medium
created: 2026-11-08
updated: 2026-11-08
blocked_by: []
related:
  adrs: [ADR-GRP-017, ADR-GRP-005, ADR-MCP-001, ADR-GRP-002]
  stories: [US-DSK-003, US-DSK-004, SPIKE-DSK-001, SPIKE-GRD-002]
  specs: []
ado:
  id: null
  url: null
tags: [desktop, spike, ascendencia, open-a, launchservices, apple-events, oapp, keysenderpidattr, perfil-mcp, r-6, macos]
---

## SPIKE-DSK-002: Detectar que un agente lanzó la app de escritorio con `open -a`

> **Estado (2026-11-08)**: sin empezar. Reservado desde la tabla "Dependencias de spike" de [ADR-GRP-017](../../../../architecture/decisions/ADR-GRP-017-app-escritorio-cliente-daemon.md) (pregunta 2). **Alcance: macOS**. Windows y Linux: **Pendiente: etapa de validación multiplataforma**.

**Valor**: ADR-GRP-017 § 3 da perfil `mcp` a la app cuando hay un agente en su ascendencia, pero `open -a` hace que la lance `launchd` y la ascendencia queda limpia. Si el daemon no puede distinguir ese caso, un agente obtiene la lectura completa de la app (hueco R-6, aceptado solo para las alphas) y, con identidad firmada, llega a una ventana que puede pedir presencia de sesión. El spike dice si ese caso se detecta y con qué fiabilidad.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-DSK-002-resultados.md` y un prototipo aislado en `spikes/app-launch-origin/`. **Slug**: DSK, por la misma razón que SPIKE-DSK-001. **Depende de**: nada. **Desbloquea**: cerrar o aceptar por escrito el hueco R-6 de ADR-GRP-017 antes de v0.1.0 y la regla de perfil de la app cuando la lanza un agente; informa la historia de presencia de sesión (si la detección no es fiable, la presencia de sesión exige además un gesto en la ventana, decisión que tomaría el Arquitecto con el resultado).

### Pregunta

Cuando un agente ejecuta `open -a GitRaptor` (o `open` sobre el bundle, o `osascript` con `tell application … to activate`), ¿la app recibe en su evento de apertura (`kAEOpenApplication`, `oapp`) el pid del proceso que lo pidió con `keySenderPIDAttr`, se puede subir ese pid hasta el agente con la ascendencia de ADR-GRP-005 § 6.1, y lo puede comprobar el daemon sin fiarse de lo que la app le cuente?

### Hipótesis

- **Remitente**: en un `open -a`, el remitente del `oapp` es LaunchServices o el proceso `open`, no el agente; el pid de `open` sí tiene al agente en su ascendencia mientras `open` vive, pero `open` puede terminar antes de que la app lea el evento.
- **Atributo**: `keySenderPIDAttr` y el audit token del remitente (`keySenderAuditTokenAttr`) están disponibles en el evento en macOS 13 o superior; el token es más fiable que el pid.
- **Confianza**: lo que la app reporta al daemon es una declaración del cliente (S-01); solo vale como **señal para restringir**, nunca para conceder: si la app dice "me lanzó un agente", el daemon baja a perfil `mcp`; si no dice nada, no sube.
- **Alternativas del SO**: la responsabilidad del proceso (`responsible pid`) que guarda el kernel para la app lanzada desde una terminal apunta a la terminal y no a `open`, y no distingue al agente del humano en la misma terminal.

### Experimento

- **Lanzamientos**: la app lanzada por doble clic en Finder, desde el Dock, con Spotlight, con `open -a` desde Terminal.app por el humano, con `open -a` desde un proceso hijo de Claude Code, con `osascript` desde ese mismo proceso y con `launchctl asuser`. Anotar en cada caso el remitente del `oapp`, su pid, su audit token y si `open` sigue vivo cuando la app lo lee.
- **Ascendencia**: subir desde el remitente con el algoritmo de ADR-GRP-005 § 6.1 y comprobar si se encuentra al agente (proceso de Claude Code con su marca) y en qué casos se pierde.
- **Carrera**: forzar que `open` termine antes de que la app procese el evento y ver si el pid queda reutilizable o el token deja de resolver.
- **Responsible pid**: comparar con la responsabilidad del proceso que el SO asigna a la app.
- **Vía al daemon**: diseñar en el prototipo cómo la app pasaría la señal al daemon y comprobar que una app que miente solo puede bajar su propio perfil.

### Criterios de Éxito

- Tabla de los siete lanzamientos con lo observado y si el agente se detecta.
- Veredicto: **detectable con fiabilidad** (cierra R-6 por esta vía), **detectable a veces** (señal de solo restricción, R-6 sigue abierto con su residuo escrito) o **no detectable** (R-6 se cierra solo por otra vía: gesto en la ventana antes de la presencia de sesión o factor por acción también para lo local).
- Recomendación al Arquitecto para ADR-GRP-017 § 3 y su "Riesgos residuales", con la condición que lo gobierna y su test.

### Time-box

2 días de trabajo, en macOS (Apple silicon). Si se agota sin resultado concluyente, se cierra como "no detectable" con lo medido.
