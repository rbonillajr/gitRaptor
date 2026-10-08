---
id: TS-GRP-006
title: "Observación por niveles: activo, dormido con centinela y despertar"
type: ts
status: implemented
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-07
updated: 2026-10-08
related:
  adrs: [ADR-GRP-010, ADR-GRP-011, ADR-GRP-013, ADR-GRP-015, ADR-GRP-006, ADR-GRP-007, ADR-GRP-016, ADR-TMC-004]
  stories: [US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-017, US-GRP-020, US-CKP-001, TS-GRP-003, TS-GRP-004, TS-GRP-005, INF-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, recursos, escala, niveles, dormido, centinela, despertar, res-11, res-12, nfr-04, nfr-05, br-cons-005]
---

## TS-GRP-006: Observación por niveles: activo, dormido con centinela y despertar

**Valor**: GitRaptor puede observar más de 100 repos con el mismo consumo en reposo que con 10 worktrees, porque el coste escala con los repos activos y no con los observados, sin reducir la protección.

### Descripción

**Como** Arquitecto
**Quiero** que el motor observe cada repo en un nivel (activo, despertando o dormido) y que un repo dormido conserve solo un centinela y unas redes de seguridad baratas
**Para** cumplir RES-11 y RES-12 con 100 repos observados sin romper NFR-04 en los activos ni la condición Q49 del PO, según la que dormir no reduce la protección (ADR-GRP-010, Enmienda 2026-10-07)

> Dev Spec: [`dev-specs/TS-GRP-006-dev-spec.md`](../dev-specs/TS-GRP-006-dev-spec.md) | En curso: se entrega por tramos (tramo 1, el observador)
>
> **Estado**: `ready`. **Decisión de Rene (2026-10-07)**: acepta la Enmienda (2026-10-07) de ADR-GRP-010 y ADR-GRP-011, incluido el diseño de centinela (Q49).
>
> **Por qué es un enabler** (Enabler Decision Gate): no tiene una historia dueña única, porque lo consumen la observación en vivo (US-GRP-002), la observación continua (US-GRP-004), los huecos (US-GRP-005), la vista de recursos (US-GRP-017), el descubrimiento (US-GRP-020) y la flota del Cockpit (US-CKP-001). Su resultado observable directo ya está en la enmienda de US-GRP-017 (Q49 del contexto). El descubrimiento **no** forma parte de este enabler: tiene resultado observable y su dueña es US-GRP-020.

### Alcance Técnico

- **Mantener** un nivel por repo (activo, despertando o dormido), persistido con la hora de la última actividad, y publicarlo en el estado del repo para los clientes, como cambio aditivo del contrato del canal.
- **Dormir** un repo cuando lleva el umbral configurado sin actividad, sin ninguna sesión presente y sin ningún cliente suscrito. Al dormir se vacía su debounce, se guarda su huella, se cierra su almacén del perfil y se suelta su estado en memoria.
- **Impedir** que duerma un repo con algún worktree en modo degradado, porque sin centinela dormir reduciría la protección.
- **Conservar** las vigilancias de un repo dormido como centinela: el primer cambio que pasa el filtro de ignorados lo despierta, sin recomputar ni abrir el almacén.
- **Crear** el barrido de metadatos de los dormidos: una única tarea de prioridad baja para todos los repos, que solo comprueba metadatos del sistema de archivos, sin lanzar procesos de Git ni abrir el repo con la capa de lectura.
- **Crear** la reconciliación lenta de los dormidos, con un intervalo mínimo y un presupuesto de CPU que alarga el intervalo cuando no caben, y exponer el intervalo efectivo.
- **Retirar** de los repos dormidos el sondeo de respaldo y la reconciliación periódica de los activos.
- **Despertar** el repo ante el centinela, una sesión detectada, la suscripción o la petición de un cliente, una llamada del hook de Guardrails o un cambio encontrado por una red de seguridad. Al despertar, se publica "reconciliando", se reconcilian todos sus worktrees en paralelo y los eventos de Git se reconstruyen de los reflogs.
- **Registrar** como hueco con causa "observación en reposo" solo lo que encuentre una red de seguridad sin que el centinela lo señalara, y llevar la cuenta de esos hallazgos como diagnóstico.
- **Arrancar** dormidos los repos sin actividad reciente, comparando su huella en lugar de reconciliarlos por completo.
- **Leer** las tres claves de niveles de la sección del motor (umbral, intervalo del barrido e intervalo de la reconciliación lenta), solo en los niveles admitidos y con los intervalos más largos en modo de ahorro de energía.
- **Exponer** en la vista de recursos los repos y worktrees por nivel, las vigilancias por nivel y la CPU de las redes de seguridad (dato de US-GRP-017).
- **Fuera de alcance**: el descubrimiento en raíces y sus comandos (US-GRP-020, US-GRP-022); cómo se presenta el nivel en la CLI y en la TUI (US-GRP-017, Cockpit); la captura de la Time Machine al despertar, que ya consume los eventos publicados (ADR-TMC-004); el escenario de banco (INF-GRP-002, Enmienda 2026-10-07); dormir worktrees sueltos dentro de un repo activo.

### Plan de Verificación

#### Pruebas Automatizadas

- **Centinela**: en un repo dormido, una edición en el working tree lo despierta y queda publicada sin hueco. Una ráfaga produce un único despertar.
- **Protección (Q49)**: una edición seguida de un `reset --hard` en un repo dormido deja el contenido capturado en la Time Machine, cuando exista la captura por observación. Mientras no exista, se verifica que la edición se publica antes del reset.
- **Almacén cerrado**: un repo dormido no tiene abierto ningún archivo de su almacén. Un repo dormido sin cambios no abre el almacén en ningún ciclo del barrido.
- **Barrido sin procesos**: con un shim de `git` en el `PATH` que cuenta las invocaciones (como INF-GRP-001), 100 ciclos del barrido dejan 0 invocaciones.
- **Red de seguridad**: con el centinela desactivado en un test, un commit lo encuentra el barrido y una edición sin `git add` la encuentra la reconciliación lenta. Los dos quedan en un hueco "observación en reposo", "sin atribuir", y el contador sube.
- **Despertar**: una sesión simulada, una suscripción a ese repo y la llamada de un hook lo despiertan. Una suscripción a la flota no lo despierta. Con escrituras durante el despertar, todo queda publicado.
- **Reflog**: tres commits seguidos en un repo dormido producen tres eventos de Git, en orden y con la hora del reflog.
- **Degradado**: un worktree en modo degradado impide que su repo duerma.
- **Configuración**: las claves de niveles en el nivel de equipo se ignoran con diagnóstico, y el modo de ahorro alarga los intervalos.
- **Rendimiento** (en el banco, nunca en tests de debug): RES-11 y RES-12 con el escenario `tiered-scale` de INF-GRP-002.
- Tests con repos y perfiles temporales, nunca este repo ni el perfil real (NFR-01). Repo intacto con la suite de INF-GRP-001.

#### Verificación Manual / Sandbox

- **macOS, dogfooding**: dejar 20 o más repos sin tocar durante el umbral y comprobar en `raptor status --resources` que pasan a dormidos y que el consumo en reposo no crece. Abrir la TUI en uno de ellos y ver "reconciliando" y después el estado activo.
- **Linux y Windows**: **Pendiente: etapa de validación multiplataforma**.

## Estado de la implementación (2026-10-08)

Implementado en: PR #164, #167, #173 (ajuste en #178).

Notas (fuera del alcance de esta ficha o sin bloquearla):
- Nivel local de `dormantAfterHours` (US-GRP-013) y el escenario `tiered-scale` del banco (INF-GRP-002).
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
