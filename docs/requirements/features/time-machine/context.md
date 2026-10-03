---
id: CTX-TMC-001
title: "Contexto — Time Machine"
type: context
status: draft
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
scope: feature
stakeholders:
  - rene-bonilla
related:
  rules:
    - BR-TMC-001
  context:
    - CTX-GRP-001
tags:
  - time-machine
  - snapshots
  - undo
  - redo
  - timeline
  - cero-perdida-de-datos
  - atribucion
  - mvp
---

# Contexto del Feature: Time Machine

> **Fundamento**: Este documento sigue un enfoque híbrido BRD-PRD adaptado para arquitectura pre-desarrollo. Combina objetivos de negocio (BRD) con requisitos de producto (PRD) para informar decisiones arquitectónicas en el framework AADD.
>
> **Origen**: [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5) § 6.1, capacidades **BR-08, BR-09 y BR-10**. Épica **E-001**, feature **F-001-03** del [backlog](../../backlog.md). Reglas en [business-rules.md](./business-rules.md) (BR-TMC-001). Depende del [Motor local](../motor-local/context.md) (CTX-GRP-001) y respeta sus decisiones Q1, Q6, Q21-Q24, Q27 y Q32-Q37.

---

## 1. Visión General (BRD)

La Time Machine es la **red de seguridad** de GitRaptor. Guarda automáticamente un punto recuperable (snapshot) del repo antes de cada operación que lo modifica, incluido el trabajo sin commitear, y permite volver atrás con un solo comando: deshacer la última operación, deshacer solo lo que hizo un agente en un periodo o restaurar cualquier punto anterior. Un timeline navegable muestra qué cambió, cuándo y quién lo hizo.

Es la garantía concreta de **NFR-01 (cero pérdida de datos)** y la promesa principal del producto para quien teme que un agente rompa algo (BRD § 5). A diferencia del Motor local, que solo observa (Q21), la Time Machine **sí escribe** en el repo: guarda snapshots y restaura estados. Toda escritura suya es explícita, propia y recuperable (BR-TMC-CONS-004).

### Problema de Negocio (BRD)

Deshacer el trabajo de un agente hoy es manual: reflog, cherry-pick y adivinar (BRD P4). Con 3 a 10 agentes en paralelo el problema empeora:
- El trabajo sin commitear no existe en ningún otro sitio. Un `reset --hard`, un `checkout` o un descarte de worktree lo borra sin vuelta atrás.
- Saber qué hizo cada agente y deshacer solo eso exige reconstruir la historia a mano, con riesgo de deshacer trabajo de otro agente o del propio desarrollador.
- Sin red de seguridad, el desarrollador limita lo que deja hacer a los agentes, y pierde el beneficio de trabajar en paralelo.

Impacto de no resolverlo: una sola pérdida de trabajo rompe la confianza en el producto (KPI "0 incidentes", BRD § 9), y la demo de validación (`raptor undo --agent` que lo restaura todo, BRD § 13) no es posible.

### Valor Esperado (BRD)

- **ROI estimado**: no se cuantifica en dinero (herramienta interna, D1). El retorno es evitar la pérdida de trabajo y el tiempo de recuperación manual. ⚠️ **ASSUMPTION**: no hay línea base del tiempo que hoy cuesta recuperar un error de un agente `[POR VERIFICAR]`.
- **KPIs de éxito**: ver § 3.
- **Beneficiarios**: el desarrollador orquestador y el dev junior o semi-senior que usa agentes (directos); el Cockpit, Guardrails y el servidor MCP, que se apoyan en los snapshots para ofrecer acciones seguras.

---

## 2. Dominio Específico (PRD)

- **Tipo de funcionalidad**: capacidad transaccional de protección y recuperación (snapshots, undo/redo, restauración) con una vista de historial (timeline).
- **Usuarios principales**: desarrollador orquestador; agente Claude Code (soporte completo, Q32) y "otro agente" registrado, que deshacen sus propias operaciones vía MCP.
- **Casos de uso principales**:
  - **Protegerse antes de operar**: toda operación lanzada por GitRaptor (CLI, TUI/Cockpit, MCP) queda precedida de un snapshot que incluye el working tree sin commitear y los archivos nuevos sin seguimiento.
  - **Deshacer y rehacer**: `raptor undo` vuelve al estado previo a la última operación; `raptor redo` revierte ese undo.
  - **Deshacer lo de un agente**: `raptor undo --agent claude-1 --since 20m` deshace solo las operaciones atribuidas a ese agente en ese periodo, sin tocar el trabajo de otros actores.
  - **Restaurar un punto**: volver a cualquier snapshot del timeline.
  - **Consultar el timeline**: qué cambió, cuándo y quién, filtrable por worktree, agente y tiempo, con los huecos de observación visibles.
- **Alcance**:
  - **IN scope**:
    - Snapshot automático antes de toda operación lanzada por GitRaptor y antes de cada undo, redo o restauración (BR-TMC-CONS-001).
    - Contenido del snapshot: working tree sin commitear, archivos sin seguimiento y estado de ramas y worktrees; sin los archivos ignorados (BR-TMC-CONS-002, P7).
    - Captura del trabajo hecho fuera de GitRaptor (Git crudo, ediciones en el editor), con su nivel de cobertura declarado (BR-TMC-CONS-003, P1).
    - Undo, redo, undo por agente y por periodo, y restauración a un punto (BR-TMC-WF-001 a BR-TMC-WF-003).
    - Timeline por repo con filtros por worktree, agente y tiempo; atribución vigente; huecos explícitos (BR-TMC-CONS-005, BR-TMC-EDGE-002).
    - Presentación de "sin atribuir" al usuario (P3).
    - Retención configurable de snapshots (BR-TMC-TIME-001, P6).
    - Recuperación tras una interrupción a mitad de un snapshot o de un undo (BR-TMC-EDGE-003).
    - La capacidad `snapshot` y `undo` que consumen el MCP y el Cockpit.
  - **OUT of scope**:
    - Deshacer en el remoto: un push no se des-pushea; la Time Machine nunca hace push ni force-push (BR-TMC-EDGE-001).
    - Las herramientas MCP `snapshot` y `undo` como tales (nombre, parámetros, seguridad): son del **Servidor MCP** (F-001-05, BR-14).
    - Definir políticas (p. ej. prohibir force-push o restringir quién deshace): son de **Guardrails** (F-001-04, BR-11).
    - Instalar hooks de Git: son de Guardrails (BR-12, Q22).
    - Detectar agentes y atribuir eventos: es del **Motor local** (F-001-01); la Time Machine consume esa atribución.
    - Diseño visual del timeline en la TUI: se coordina con el Cockpit y el design system; aquí solo el qué.
    - El comando para editar la configuración: es de Guardrails (Q27).
    - Snapshot de archivos ignorados por `.gitignore` (dependencias, `.env`) (P7).
    - App de escritorio y extensión de editor (Fase 3).
    - Integración con Entire Checkpoints (BR-20, Fase 2).
    - Llevar snapshots a otra máquina o compartirlos con el equipo.

### Dependencias con otras features

| Feature | Relación | Dirección |
|---------|----------|-----------|
| F-001-01 Motor local | Aporta los eventos de Git con momento y actor ("agente X" con origen, o "sin atribuir"; nunca "humano", Q34), la atribución vigente tras una corrección (Q37), la observación sin huecos (Q1) y los huecos marcados (BR-EDGE-005). P17 de motor-local (qué pasa al retirar una corrección) sigue abierta y afecta al undo por agente. | Time Machine depende del motor |
| F-001-02 Cockpit | Sus acciones destructivas (descartar worktree y rama, merge, rebase; BR-07) deben quedar cubiertas por un snapshot previo. Presenta el timeline dentro de la TUI. | Cockpit depende de la Time Machine |
| F-001-04 Guardrails | Sus hooks de Git, cuando existan, permiten un snapshot previo a operaciones de Git crudo; la Time Machine no depende de ellos (P1). Sus políticas pueden restringir quién deshace qué (BR-TMC-AUTH-001). Define el formato de la configuración (P8 de motor-local). | Bidireccional |
| F-001-05 Servidor MCP | Expone `snapshot` y `undo` a los agentes (BR-14) y sus operaciones seguras quedan cubiertas por un snapshot previo. | MCP depende de la Time Machine |

---

## 3. Objetivos de Negocio (BRD)

| Objetivo | Métrica de Éxito | Prioridad |
|----------|------------------|-----------|
| No perder nunca trabajo por culpa de GitRaptor | 0 incidentes de pérdida de datos (BRD § 9, NFR-01) | Alta |
| Que toda operación de GitRaptor sea reversible | 100% de las operaciones lanzadas por GitRaptor con snapshot previo (BR-TMC-CONS-001) | Alta |
| Que la red de seguridad se use | ≥ 3 undos por usuario activo y mes (BRD § 9) | Alta |
| Proteger sin frenar | Overhead < 200 ms por snapshot en repos medianos (NFR-04); "repo mediano" `[POR VERIFICAR]` | Alta |
| Resistir fallos a mitad de operación | 100% de las pruebas de caos (NFR-12) sin pérdida: matar el proceso durante un snapshot o un undo deja el repo recuperable | Alta |

---

## 4. Stakeholders y Actores (BRD + PRD)

### Stakeholders de Negocio (BRD)

| Stakeholder | Interés | Expectativa |
|-------------|---------|-------------|
| Rene Bonilla (producto, revisión e integración) | Único humano del proyecto (D3) y primer usuario (dogfooding). | Deshacer cualquier error de sus agentes sin miedo a perder trabajo. |
| Desarrolladores internos y equipos piloto (BRD § 9) | Usuarios futuros. | Una red de seguridad de un comando. `[POR VERIFICAR]` quiénes son. |

### Actores del Sistema (PRD)

| Actor | Descripción | Permisos/Capacidades |
|-------|-------------|----------------------|
| **Desarrollador orquestador** | Persona que supervisa los agentes (D3). GitRaptor no puede probar que una petición viene de él (Q34): un agente puede lanzar `raptor undo` desde su propia shell. | Consulta el timeline completo y ajusta la retención en su perfil o en la configuración local personal (P6). Deshacer trabajo de otro actor exige su confirmación interactiva en ese momento, que un agente no puede dar ⚠️ **ASSUMPTION** (BR-TMC-AUTH-001, P8, P14). |
| **Agente Claude Code** | Soporte completo en el MVP (Q32). Opera con Git crudo, con la CLI desde su shell o vía MCP. | Pide snapshots; si la petición de undo se le atribuye, solo deshace sus propias operaciones (P8, P14). Lo que haga con Git crudo queda cubierto según BR-TMC-CONS-003. |
| **Otro agente** (Codex, Cursor u otro registrado) | Sin soporte completo (Q32). Cursor solo si se registra de forma explícita; lo hecho en el editor del humano queda "sin atribuir" (Q32). | Igual que Claude Code, con su atribución por registro. |
| **"Sin atribuir"** | No es un actor: es lo que el motor no pudo atribuir a un agente (Q34, Q35). | Se presenta como "Tú u otro (sin atribuir)" (P3). Nunca se trata como "humano" ni entra en un undo por agente. Es también el valor del solicitante de un undo que no se puede atribuir (P14). |
| **Motor local** | Fuente de eventos y atribución. | Solo observa; no escribe (Q21). Las escrituras son de la Time Machine. |

---

## 5. Restricciones y Limitaciones (BRD + Arquitectura)

### Regulatorias (BRD + Arquitectura)
- No hay restricciones regulatorias específicas: herramienta interna y 100% local (NFR-03). `[POR VERIFICAR]` la política interna de ASSA (P1 de motor-local).
- Los snapshots contienen código y trabajo sin commitear: son datos confidenciales y no salen de la máquina (domain-context).

### Técnicas (Arquitectura)
- Usa el Git del sistema (≥ 2.38, NFR-07) y respeta su configuración, hooks y credenciales.
- Sin hooks propios (Q22): la cobertura del Git crudo no puede depender de que existan los de Guardrails (P1).
- Los snapshots no pueden empujarse al remoto por accidente, borrarse con un `git gc` ni alterarse cuando un agente trabaja en el working tree (P2). El mecanismo y la ubicación (el BRD propone "refs ocultas y oplog propio") los decide el Arquitecto con esas garantías.
- Stack decidido en ADR-GRP-001; el diseño del oplog y los snapshots es un ADR pendiente (BRD § 11).

### De Negocio (BRD)
- Una persona orquestando agentes (D3): historias pequeñas y verificables.
- Soporte completo solo para Claude Code en el MVP (D2 revisada, Q32).
- Sin fecha objetivo para el MVP (BRD § 12.2).

---

## 6. Requisitos No Funcionales Destacados (PRD + Arquitectura)

| RNF | Valor Objetivo | Crítico | Justificación |
|-----|----------------|---------|---------------|
| **Seguridad: cero pérdida de datos** | Toda operación de GitRaptor que modifica el repo tiene snapshot previo; sin snapshot, no se ejecuta (NFR-01) | Sí | El trabajo sin commitear no existe en otro sitio. |
| **Performance** | Overhead < 200 ms por snapshot en repos medianos (NFR-04). "Repo mediano" `[POR VERIFICAR]` | Sí | Si el snapshot frena a los agentes, el usuario lo desactiva. |
| **Robustez** | Matar el proceso a mitad de un snapshot o de un undo deja el repo recuperable (NFR-12) | Sí | Los fallos ocurren justo en las operaciones de riesgo. |
| **Seguridad: privacidad** | Snapshots 100% locales; sin archivos ignorados (`.env`) (NFR-03, P7) | Sí | Evita copiar secretos y dependencias. |
| **Espacio en disco** | Retención configurable; por defecto 30 días `[POR VERIFICAR]` (P6) | No | Los snapshots crecen con el uso. |
| **Escalabilidad** | 10 o más worktrees activos y repos de más de 100K commits (NFR-05) | Sí | Caso de uso típico. |
| **i18n** | Mensajes en inglés y español (NFR-10) | No | Convención del producto. |

---

## 7. Integraciones Externas (PRD + Arquitectura)

| Sistema/API | Propósito | Tipo de Integración | Criticidad |
|-------------|-----------|---------------------|------------|
| Git del sistema (≥ 2.38) | Leer el estado y escribir snapshots y restauraciones | Lectura y escritura local; mecanismo: lo decide el Arquitecto | Alta |
| Motor local (F-001-01) | Eventos, atribución vigente y huecos | Interna; consumo de su capacidad | Alta |
| Hooks de Git de Guardrails (F-001-04) | Snapshot previo a operaciones de Git crudo, cuando existan | Interna y opcional (P1) | Media |
| Servidor MCP (F-001-05) y Cockpit (F-001-02) | Consumen `snapshot`, `undo` y el timeline | Interna | Alta |
| Configuración en tres niveles | Leer la retención | Solo lectura; formato en ADR pendiente (P8 de motor-local) | Baja |

---

## 8. Características Únicas del Feature (PRD)

- **Undo por agente y por periodo**: "deshaz lo que hizo claude-1 en los últimos 20 minutos", sin tocar el trabajo de otros (BRD § 4).
- **Incluye lo no commiteado**: el working tree y los archivos nuevos forman parte del snapshot.
- **Cobertura honesta**: el timeline dice qué está protegido con snapshot previo y qué se capturó por observación (P1).
- **Nunca afirma "humano"**: lo no atribuido se presenta como "Tú u otro (sin atribuir)" (Q34, P3).

---

## 9. Glosario del Dominio (PRD)

| Término | Definición | Notas |
|---------|------------|-------|
| **Snapshot** | Punto recuperable del repo: working tree sin commitear, archivos sin seguimiento y estado de ramas y worktrees. | Sin archivos ignorados (P7). |
| **Operación** | Acción que modifica el estado del repo (commit, checkout, reset, rebase, merge, borrar rama o worktree, undo, restauración). | Leer no es operación. |
| **Operación lanzada por GitRaptor** | La que se pide desde la CLI, la TUI/Cockpit o el MCP. | Cobertura garantizada (P1-a). |
| **Git crudo** | Operación de Git o edición hecha fuera de GitRaptor (por el humano o por un agente). | Cobertura por observación (P1-b). |
| **Undo / Redo** | Volver al estado previo a la última operación / revertir el último undo. | Ambos crean un snapshot previo. Ámbito por defecto: el worktree desde el que se invoca (S8). |
| **Solicitante** | Quien pide un undo, redo o restauración. Se registra como "agente X" o "sin atribuir", nunca como "humano" (Q34). | P14. |
| **Restauración** | Volver a un snapshot concreto del timeline. | |
| **Timeline** | Lista ordenada de operaciones y snapshots con momento, actor y cobertura. | Muestra los huecos. |
| **Atribución vigente** | La atribución actual de un evento, tras las correcciones (Q37). | El undo por agente usa esta. |
| **Hueco** | Periodo sin observación; lo ocurrido queda "sin atribuir" (BR-EDGE-005). | Visible en el timeline. |
| **Solape** | Cuando deshacer lo de un agente tocaría cambios posteriores de otro actor en los mismos archivos. | P4. |

---

## 10. Estándares Aplicables (Arquitectura)

- **Interoperabilidad**: Git estándar; el repo sigue siendo un repo Git normal para cualquier otra herramienta. MCP estándar para los agentes (NFR-08).
- **Codificación y Terminología**: fechas y duraciones legibles por el usuario (`20m`, `2h`); formato exacto: lo decide el Arquitecto.
- **Seguridad y Autenticación**: sin autenticación (local). Seguridad del MCP según NFR-02 (F-001-05).
- **Compliance**: solo políticas internas; NFR-01 y NFR-03.

---

## 11. Referencias (BRD + PRD + Arquitectura)

- [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5): § 2 (P4), § 5, § 6.1 (BR-07 a BR-10, BR-12, BR-14), § 7 (NFR-01, NFR-04, NFR-12), § 9, § 10, § 11, § 13.
- [Contexto del Motor local](../motor-local/context.md) (CTX-GRP-001) y sus [reglas](../motor-local/business-rules.md): BR-CONS-001, BR-CONS-002, BR-CONS-003, BR-CONS-005, BR-EDGE-003, BR-EDGE-004, BR-EDGE-005.
- [Reglas de esta feature](./business-rules.md) (BR-TMC-001) · [Backlog](../../backlog.md) · [domain-context](../../../domain-context.md).

---

## Supuestos

| # | Supuesto | Estado |
|---|----------|--------|
| S1 | ⚠️ **ASSUMPTION**: la cobertura tiene dos niveles: garantizada para lo lanzado por GitRaptor y por observación para el Git crudo (P1). | Pendiente (Rene Bonilla) |
| S2 | ⚠️ **ASSUMPTION**: "sin atribuir" se presenta como "Tú u otro (sin atribuir)" (P3). | Pendiente |
| S3 | ⚠️ **ASSUMPTION**: los archivos ignorados no entran en el snapshot (P7). | Pendiente |
| S4 | ⚠️ **ASSUMPTION**: retención por defecto de 30 días, configurable en perfil y local personal (P6). | Pendiente |
| S5 | ⚠️ **ASSUMPTION**: redo revierte el último undo; si después hubo cambios en los mismos archivos, aplica la regla de solape (BR-TMC-WF-001). | Pendiente |
| S6 | ⚠️ **ASSUMPTION**: con una operación de Git en curso (rebase o merge a medias), el undo y la restauración se detienen y piden terminarla o abortarla antes (BR-TMC-EDGE-004). | Pendiente |
| S7 | ⚠️ **ASSUMPTION**: restaurar un punto afecta al worktree en el que se pide y a las ramas y worktrees que cambiaron después de ese punto; los demás worktrees no se tocan (P11). | Pendiente |
| S8 | ⚠️ **ASSUMPTION**: `raptor undo` sin flags deshace la última operación del worktree desde el que se invoca; si es de un actor distinto del solicitante, aplican BR-TMC-AUTH-001 y la regla de solape; nunca actúa sobre otros worktrees sin pedirlo (BR-TMC-WF-001; P10, P11, P14). | Pendiente |
| S9 | ⚠️ **ASSUMPTION**: el solicitante de un undo se atribuye como los eventos ("agente X" o "sin atribuir"). Atribuido a un agente, solo deshace lo suyo; "sin atribuir", deshacer trabajo de otro actor exige confirmación interactiva del desarrollador en ese momento, que un agente no puede dar (BR-TMC-AUTH-001; P8, P14). | Pendiente |

---

## Riesgos

| # | Riesgo | Prob. | Impacto | Mitigación (de negocio) |
|---|--------|-------|---------|-------------------------|
| R1 | Overhead del snapshot por encima de 200 ms en repos grandes (BRD § 10). | Media | Medio | Spike (a) del BRD § 13 antes de las historias de snapshot. |
| R2 | Operaciones de Git crudo sin snapshot previo (sin hooks propios, Q22): lo editado entre la última captura y una operación destructiva de Git crudo puede perderse. | Alta | Alto | Captura continua del working tree; snapshot previo vía hooks de Guardrails cuando existan; cobertura declarada en el timeline (P1). |
| R7 | Un agente lanza `raptor undo` desde su shell y deshace trabajo de otro actor. | Media | Crítico | El solicitante se atribuye como un evento; si queda "sin atribuir", deshacer lo de otro exige confirmación interactiva (P14). |
| R3 | Un undo por agente deshace trabajo de otro actor por una atribución errónea. | Media | Crítico | Atribución vigente (Q37), "sin atribuir" nunca entra en el undo por agente y el solape detiene el undo (P4). |
| R4 | Los snapshots ocupan demasiado disco. | Media | Medio | Retención configurable con aviso antes de purgar (P6). |
| R5 | Un secreto en un archivo ignorado se pierde al restaurar, o se copia si se incluyera. | Baja | Medio | No se incluyen ignorados y se avisa en la documentación (P7). |
| R6 | El usuario cree que el undo también deshace el remoto. | Media | Alto | Aviso explícito cuando lo deshecho ya está en el remoto (P5). |

---

## Decisiones tomadas

| # | Decisión | Fuente |
|---|----------|--------|
| D-TMC-1 | El motor nunca emite "humano"; solo "agente X" (con origen) o "sin atribuir". Esta feature decide cómo se presenta (P3). Refina BR-10 del BRD ("humano o agente X"). | Q34, Q35 (motor-local) |
| D-TMC-2 | El timeline y el undo por agente usan la atribución vigente; corregir reatribuye la sesión desde su inicio. | Q33, Q37 (motor-local) |
| D-TMC-3 | El motor no escribe; toda escritura en el repo de esta feature es de la Time Machine, explícita y recuperable. | Q21 (motor-local) |
| D-TMC-4 | Los hooks de Git son de Guardrails; la Time Machine no instala hooks. | Q22 (motor-local) |
| D-TMC-5 | Configuración en tres niveles que se lee; cada valor declara los niveles que admite; el comando de edición es de Guardrails. | Q23, Q24, Q27 (motor-local) |
| D-TMC-6 | Soporte completo solo de Claude Code; los demás, "otro agente" por registro. | Q32 (motor-local), D2 |
| D-TMC-7 | Observación sin huecos; lo ocurrido en un hueco queda "sin atribuir" y el timeline lo muestra. | Q1, Q6, BR-EDGE-005 (motor-local) |
| D-TMC-8 | Fuera de alcance: deshacer en el remoto, app de escritorio y Entire Checkpoints. | BRD § 6 |
| D-TMC-9 | La cobertura en dos niveles (garantizada para lo lanzado por GitRaptor; por observación para el Git crudo) **refina BR-08 y NFR-01** del BRD, que piden snapshot "antes de cada operación" y "toda operación destructiva". Pendiente de confirmar en P1. | BRD § 6.1, § 7; Q22 |

---

## Preguntas abiertas

| # | Pregunta | Recomendación del PO | Para quién |
|---|----------|----------------------|------------|
| P1 | ¿Cómo se cubre "antes de toda operación" sin hooks propios (Q22)? | Dos niveles: (a) garantizado antes de lo lanzado por GitRaptor y de cada undo o restauración; (b) para Git crudo y ediciones externas, captura continua del working tree más snapshot previo vía hooks de Guardrails cuando existan, sin depender de ellos. Cobertura declarada por tipo de operación. | Rene Bonilla |
| P2 | ¿Dónde viven los snapshots? | Garantías de negocio: no se empujan por accidente, no los borra un `git gc`, un agente no los altera. Mecanismo y ubicación: Arquitecto. | Rene Bonilla → Arquitecto |
| P3 | ¿Cómo se presenta "sin atribuir"? | "Tú u otro (sin atribuir)", sin afirmar nunca que fue el humano. | Rene Bonilla |
| P4 | `undo --agent X` con cambios posteriores de otro actor en los mismos archivos o fragmentos. | No sobrescribir nunca trabajo de otro actor: detenerse, mostrar el solape y dejar decidir. | Rene Bonilla |
| P5 | Undo de algo ya empujado al remoto. | Solo local; avisa de que ya está en el remoto; nunca hace push ni force-push. | Rene Bonilla |
| P6 | Retención de snapshots. | Configurable en perfil y local personal (no en equipo); por defecto 30 días `[POR VERIFICAR]`; nunca purgar el snapshot previo a la última operación destructiva; avisar antes de purgar. | Rene Bonilla |
| P7 | ¿Qué incluye un snapshot? | Archivos sin seguimiento sí; ignorados no (dependencias, `.env`), con el riesgo R5 documentado. | Rene Bonilla |
| P8 | ¿Quién puede deshacer qué? | Un solicitante atribuido a un agente, solo sus propias operaciones; deshacer trabajo de otro actor exige confirmación interactiva del desarrollador (ver P14). Guardrails puede restringir más. | Rene Bonilla |
| P9 | Reatribución (Q37) de eventos ya deshechos. | El registro del undo (quién lo ejecutó y sobre qué) no se reescribe; el timeline muestra la atribución vigente. | Rene Bonilla |
| P10 | Granularidad del timeline. | Por repo, con filtros por worktree, agente y tiempo. | Rene Bonilla |
| P11 | ¿Qué alcanza una restauración a un punto? | Supuesto S7: el worktree donde se pide y las ramas y worktrees que cambiaron después; nada más. | Rene Bonilla |
| P12 | ¿Qué es un "repo mediano" para NFR-04? | Fijar una referencia (archivos y tamaño) en el spike (a). | Rene Bonilla → Arquitecto |
| P13 | Dependencia de P17 de motor-local: si al retirar una corrección los eventos vuelven a la detectada, el undo por agente cambia de alcance. | Resolver P17 antes de las historias de undo por agente. | Rene Bonilla |
| P14 | ¿Cómo se identifica a quien pide un undo (CLI frente a MCP) y qué pasa si no se puede saber? | Se atribuye como un evento ("agente X" o "sin atribuir", nunca "humano", Q34). Atribuido a un agente: solo deshace lo suyo. "Sin atribuir": deshacer trabajo de otro actor exige una confirmación interactiva del desarrollador en ese momento, que un agente no puede dar. | Rene Bonilla → Arquitecto (mecanismo) |

---

## ✅ Quality Review (Auto-evaluación del Contexto)

> Ejecutada el 2026-10-03 según `methodology.md` § 7. Resultado: **5 ✅ · 9 ⚠️ · 0 🔴**. Calidad global: 6/10, parcialmente listo: el alcance y las garantías están claros, pero varias reglas dependen de P1, P4, P6, P8 y P14.
>
> **Re-ejecución 2026-10-03 (RESERVAS del Artifact Judge)**: el solicitante de un undo deja de suponerse humano (Q34) y pasa a P14; AUTH-001 queda como supuesto; se fija el ámbito por defecto del undo (S8); D-TMC-9 declara que la cobertura en dos niveles refina BR-08 y NFR-01. Se añade la fila "Permisos del undo" (⚠️).

| Sección | Resultado | Nota |
|---------|-----------|------|
| Problema de Negocio | ✅ | Específico (BRD P4), con impacto en NFR-01 y en la demo. |
| Valor Esperado / ROI | ⚠️ | Sin línea base del tiempo de recuperación manual. |
| KPIs de Éxito | ✅ | Cinco métricas cuantificadas. |
| Usuarios/Actores | ✅ | Roles diferenciados; "sin atribuir" tratado como no actor. |
| Alcance OUT of scope | ✅ | 11 exclusiones, cada una con su dueño. |
| Restricciones | ⚠️ | Ubicación de los snapshots abierta (P2); política interna de ASSA sin confirmar. |
| RNFs | ⚠️ | "Repo mediano" sin definir (P12); retención por defecto sin validar (P6). |
| Integraciones | ⚠️ | Depende de los hooks de Guardrails, aún sin definir, para cubrir el Git crudo (P1). |
| Glosario | ✅ | Términos clave definidos, incluidos cobertura, hueco y solape. |
| Cobertura de snapshots | ⚠️ | P1 sin decidir: la promesa de "antes de toda operación" depende de ella. |
| Undo por agente | ⚠️ | P4 y P13 (P17 de motor-local) sin decidir; ámbito por defecto supuesto (S8). |
| Permisos del undo | ⚠️ | No se puede probar que el solicitante es el humano; identificación y confirmación interactiva sin decidir (P8, P14, S9). |
| Restauración a un punto | ⚠️ | Alcance supuesto (S7, P11). |
| Retención | ⚠️ | Valor por defecto y niveles supuestos (P6). |

## ⚠️ Known Risks (from Quality Review)

| # | Sección | Riesgo | Impacto | Aceptado por |
|---|---------|--------|---------|--------------|
| 1 | Valor Esperado / ROI | Sin línea base del tiempo de recuperación manual. | No se podrá demostrar el ahorro; solo el KPI de uso (≥ 3 undos). | Pendiente de revisión (Rene Bonilla) |
| 2 | Restricciones | Ubicación de los snapshots sin decidir (P2). | El Arquitecto debe probar las tres garantías de negocio en su ADR. | Pendiente de revisión (Rene Bonilla) |
| 3 | RNFs | "Repo mediano" sin definir (P12). | NFR-04 no es verificable hasta el spike (a). | Pendiente de revisión (Rene Bonilla) |
| 4 | Integraciones / Cobertura | El Git crudo depende de captura continua y de hooks de Guardrails aún inexistentes (P1). | Una operación de Git crudo puede quedar sin snapshot previo; riesgo R2. | Pendiente de revisión (Rene Bonilla) |
| 5 | Undo por agente | Solape, retiro de correcciones y ámbito por defecto sin decidir (P4, P13, S8). | Las historias de undo por agente no se pueden cerrar; riesgo R3. | Pendiente de revisión (Rene Bonilla) |
| 8 | Permisos del undo | Identificación del solicitante y confirmación interactiva sin decidir (P8, P14). | Un agente podría deshacer trabajo ajeno desde su shell; riesgo R7. | Pendiente de revisión (Rene Bonilla) |
| 6 | Restauración a un punto | Alcance supuesto (P11). | Una restauración podría tocar más o menos de lo esperado. | Pendiente de revisión (Rene Bonilla) |
| 7 | Retención | Valor por defecto y niveles supuestos (P6). | Riesgo R4 de disco y de purgar algo necesario. | Pendiente de revisión (Rene Bonilla) |
