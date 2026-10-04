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
>
> **Revisión 2026-10-03 (D-TMC-10 a D-TMC-23)**: Rene Bonilla acepta las recomendaciones de P1-P14. Cobertura en dos niveles que refina BR-08 y NFR-01 (D-TMC-9, D-TMC-10); garantías de los snapshots (D-TMC-11); "Tú u otro (sin atribuir)" (D-TMC-12); solape (D-TMC-13); undo solo local (D-TMC-14); retención de 30 días en perfil y local personal (D-TMC-15); sin ignorados (D-TMC-16); solicitante atribuido y confirmación interactiva (D-TMC-17, D-TMC-23); registro del undo inmutable (D-TMC-18); timeline por repo con filtros (D-TMC-19); alcance de la restauración (D-TMC-20). "Repo mediano" se fija en el spike (a) (D-TMC-21) y las historias de undo por agente esperan a P17 de motor-local (D-TMC-22). Se confirman S1-S4 y S7-S9; S5 y S6 siguen como supuestos.
>
> **Revisión 2026-10-03 (aceptación de riesgos)**: Rene Bonilla acepta los Known Risks 1, 3 y 5 y los supuestos S5 (redo con solape) y S6 (operación de Git en curso); se retiran sus marcas de supuesto en BR-TMC-WF-001 y BR-TMC-EDGE-004.
>
> **Revisión 2026-10-03 (TQ-1 a TQ-17 del Arquitecto)**: Rene Bonilla acepta las recomendaciones de [overview de arquitectura](../../../architecture/time-machine/overview.md) § 6. Tres cambian decisiones de producto: TQ-16 actualiza D-TMC-16 (lista cerrada de credenciales excluida por defecto), TQ-14 actualiza D-TMC-23 (sin confirmación interactiva en Windows en el MVP) y TQ-17 deja `raptor tm forget` fuera del MVP (D-TMC-24). Las TQ-5, TQ-7, TQ-9, TQ-10, TQ-11 y TQ-15 y la liberación del lock propio al arrancar precisan reglas de forma observable (D-TMC-25).

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
    - Contenido del snapshot: working tree sin commitear, archivos sin seguimiento y estado de ramas y worktrees; sin los archivos ignorados, sin la lista cerrada de credenciales sin seguimiento (salvo que el perfil los incluya) y sin repos anidados, todos declarados como exclusión (BR-TMC-CONS-002, D-TMC-16).
    - Captura del trabajo hecho fuera de GitRaptor (Git crudo, ediciones en el editor), con su nivel de cobertura declarado (BR-TMC-CONS-003, D-TMC-10).
    - Undo, redo, undo por agente y por periodo, y restauración a un punto (BR-TMC-WF-001 a BR-TMC-WF-003).
    - Timeline por repo con filtros por worktree, agente y tiempo; atribución vigente; huecos explícitos (BR-TMC-CONS-005, BR-TMC-EDGE-002).
    - Presentación de "sin atribuir" al usuario (D-TMC-12).
    - Retención configurable de snapshots (BR-TMC-TIME-001, D-TMC-15).
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
    - Snapshot de archivos ignorados por `.gitignore` (dependencias, `.env`), de la lista cerrada de credenciales sin seguimiento si el perfil no los incluye y de repos anidados (D-TMC-16, D-TMC-25).
    - Borrar contenido ya capturado en snapshots (`raptor tm forget`): aplazado a una US futura; en el MVP solo existe la exclusión por defecto de credenciales (D-TMC-24, TQ-17).
    - App de escritorio y extensión de editor (Fase 3).
    - Integración con Entire Checkpoints (BR-20, Fase 2).
    - Llevar snapshots a otra máquina o compartirlos con el equipo.

### Dependencias con otras features

| Feature | Relación | Dirección |
|---------|----------|-----------|
| F-001-01 Motor local | Aporta los eventos de Git con momento y actor ("agente X" con origen, o "sin atribuir"; nunca "humano", Q34), la atribución vigente tras una corrección (Q37), la observación sin huecos (Q1) y los huecos marcados (BR-EDGE-005). P17 de motor-local (qué pasa al retirar una corrección) sigue abierta y afecta al undo por agente. | Time Machine depende del motor |
| F-001-02 Cockpit | Sus acciones destructivas (descartar worktree y rama, merge, rebase; BR-07) deben quedar cubiertas por un snapshot previo. Presenta el timeline dentro de la TUI. | Cockpit depende de la Time Machine |
| F-001-04 Guardrails | Sus hooks de Git, cuando existan, permiten un snapshot previo a operaciones de Git crudo; la Time Machine no depende de ellos (D-TMC-10). Sus políticas pueden restringir quién deshace qué (BR-TMC-AUTH-001). Define el formato de la configuración (P8 de motor-local). | Bidireccional |
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
| **Desarrollador orquestador** | Persona que supervisa los agentes (D3). GitRaptor no puede probar que una petición viene de él (Q34): un agente puede lanzar `raptor undo` desde su propia shell. | Consulta el timeline completo y ajusta la retención en su perfil o en la configuración local personal (D-TMC-15). Deshacer trabajo de otro actor exige su confirmación interactiva en ese momento, que un agente no puede dar; en Windows, en el MVP, no se ofrece y la petición se rechaza (BR-TMC-AUTH-001, D-TMC-17, D-TMC-23). |
| **Agente Claude Code** | Soporte completo en el MVP (Q32). Opera con Git crudo, con la CLI desde su shell o vía MCP. | Pide snapshots; si la petición de undo se le atribuye, solo deshace sus propias operaciones (D-TMC-17, D-TMC-23). Lo que haga con Git crudo queda cubierto según BR-TMC-CONS-003. |
| **Otro agente** (Codex, Cursor u otro registrado) | Sin soporte completo (Q32). Cursor solo si se registra de forma explícita; lo hecho en el editor del humano queda "sin atribuir" (Q32). | Igual que Claude Code, con su atribución por registro. |
| **"Sin atribuir"** | No es un actor: es lo que el motor no pudo atribuir a un agente (Q34, Q35). | Se presenta como "Tú u otro (sin atribuir)" (D-TMC-12). Nunca se trata como "humano" ni entra en un undo por agente. Es también el valor del solicitante de un undo que no se puede atribuir (D-TMC-23). |
| **Motor local** | Fuente de eventos y atribución. | Solo observa; no escribe (Q21). Las escrituras son de la Time Machine. |

---

## 5. Restricciones y Limitaciones (BRD + Arquitectura)

### Regulatorias (BRD + Arquitectura)
- No hay restricciones regulatorias específicas: herramienta interna y 100% local (NFR-03). `[POR VERIFICAR]` la política interna de ASSA (P1 de motor-local).
- Los snapshots contienen código y trabajo sin commitear: son datos confidenciales y no salen de la máquina (domain-context).

### Técnicas (Arquitectura)
- Usa el Git del sistema (≥ 2.38, NFR-07) y respeta su configuración, hooks y credenciales en las operaciones de usuario; las escrituras internas de la Time Machine (snapshot, undo, redo, restauración) no ejecutan los hooks ni la configuración del usuario (TQ-13; lo detalla el Arquitecto).
- Sin hooks propios (Q22): la cobertura del Git crudo no puede depender de que existan los de Guardrails (D-TMC-10).
- Los snapshots no pueden empujarse al remoto por accidente, borrarse con un `git gc` ni alterarse cuando un agente trabaja en el working tree (D-TMC-11). El mecanismo y la ubicación (el BRD propone "refs ocultas y oplog propio") los decide el Arquitecto con esas garantías.
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
| **Seguridad: privacidad** | Snapshots 100% locales; sin archivos ignorados (`.env`) ni la lista cerrada de credenciales sin seguimiento (NFR-03, D-TMC-16) | Sí | Evita copiar secretos y dependencias. |
| **Espacio en disco** | Retención configurable; por defecto 30 días (D-TMC-15). Tope por archivo en la captura por observación, cuota por repo y espacio libre mínimo; cifras ajustadas por el spike (TQ-5, D-TMC-25) | No | Los snapshots crecen con el uso. |
| **Escalabilidad** | 10 o más worktrees activos y repos de más de 100K commits (NFR-05) | Sí | Caso de uso típico. |
| **i18n** | Mensajes en inglés y español (NFR-10) | No | Convención del producto. |

---

## 7. Integraciones Externas (PRD + Arquitectura)

| Sistema/API | Propósito | Tipo de Integración | Criticidad |
|-------------|-----------|---------------------|------------|
| Git del sistema (≥ 2.38) | Leer el estado y escribir snapshots y restauraciones | Lectura y escritura local; mecanismo: lo decide el Arquitecto | Alta |
| Motor local (F-001-01) | Eventos, atribución vigente y huecos | Interna; consumo de su capacidad | Alta |
| Hooks de Git de Guardrails (F-001-04) | Snapshot previo a operaciones de Git crudo, cuando existan | Interna y opcional (D-TMC-10) | Media |
| Servidor MCP (F-001-05) y Cockpit (F-001-02) | Consumen `snapshot`, `undo` y el timeline | Interna | Alta |
| Configuración en tres niveles | Leer la retención | Solo lectura; formato en ADR pendiente (P8 de motor-local) | Baja |

---

## 8. Características Únicas del Feature (PRD)

- **Undo por agente y por periodo**: "deshaz lo que hizo claude-1 en los últimos 20 minutos", sin tocar el trabajo de otros (BRD § 4).
- **Incluye lo no commiteado**: el working tree y los archivos nuevos forman parte del snapshot.
- **Cobertura honesta**: el timeline dice qué está protegido con snapshot previo y qué se capturó por observación (D-TMC-10).
- **Nunca afirma "humano"**: lo no atribuido se presenta como "Tú u otro (sin atribuir)" (Q34, D-TMC-12).

---

## 9. Glosario del Dominio (PRD)

| Término | Definición | Notas |
|---------|------------|-------|
| **Snapshot** | Punto recuperable del repo: working tree sin commitear, archivos sin seguimiento y estado de ramas y worktrees. | Sin archivos ignorados, sin la lista cerrada de credenciales (salvo opción del perfil) y sin repos anidados (D-TMC-16). |
| **Operación** | Acción que modifica el estado del repo (commit, checkout, reset, rebase, merge, borrar rama o worktree, undo, restauración). | Leer no es operación. |
| **Operación lanzada por GitRaptor** | La que se pide desde la CLI, la TUI/Cockpit o el MCP. | Cobertura garantizada (D-TMC-10, nivel a). |
| **Git crudo** | Operación de Git o edición hecha fuera de GitRaptor (por el humano o por un agente). | Cobertura por observación (D-TMC-10, nivel b). |
| **Undo / Redo** | Volver al estado previo a la última operación / revertir el último undo. | Ambos crean un snapshot previo. Ámbito por defecto: el worktree desde el que se invoca (S8). Undos seguidos retroceden una operación más (pila por worktree, TQ-9). |
| **Solicitante** | Quien pide un undo, redo o restauración. Se registra como "agente X" o "sin atribuir", nunca como "humano" (Q34). | D-TMC-23. |
| **Restauración** | Volver a un snapshot concreto del timeline. | |
| **Timeline** | Lista ordenada de operaciones y snapshots con momento, actor y cobertura. | Muestra los huecos. |
| **Atribución vigente** | La atribución actual de un evento, tras las correcciones (Q37). | El undo por agente usa esta. |
| **Hueco** | Periodo sin observación; lo ocurrido queda "sin atribuir" (BR-EDGE-005). | Visible en el timeline. |
| **Solape** | Cuando deshacer lo de un agente tocaría cambios posteriores de otro actor en los mismos archivos. | D-TMC-13. |

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
| S1 | La cobertura tiene dos niveles: garantizada para lo lanzado por GitRaptor y por observación para el Git crudo (P1). | Confirmado (D-TMC-10, 2026-10-03) |
| S2 | "Sin atribuir" se presenta como "Tú u otro (sin atribuir)" (P3). | Confirmado (D-TMC-12, 2026-10-03) |
| S3 | Los archivos ignorados no entran en el snapshot (P7). | Confirmado (D-TMC-16, 2026-10-03); ampliado con la lista cerrada de credenciales (TQ-16) |
| S4 | Retención por defecto de 30 días, configurable en perfil y local personal (P6). | Confirmado (D-TMC-15, 2026-10-03) |
| S5 | redo revierte el último undo; si después hubo cambios en los mismos archivos, aplica la regla de solape (BR-TMC-WF-001). No depende de P1-P14: la regla de solape quedó decidida (D-TMC-13), su aplicación al redo no. | ✅ Aceptado por Rene Bonilla (2026-10-03) |
| S6 | con una operación de Git en curso (rebase o merge a medias), el undo y la restauración se detienen y piden terminarla o abortarla antes (BR-TMC-EDGE-004). No depende de P1-P14. | ✅ Aceptado por Rene Bonilla (2026-10-03) |
| S7 | Restaurar un punto afecta al worktree en el que se pide y a las ramas y worktrees que cambiaron después de ese punto; los demás worktrees no se tocan (P11). | Confirmado (D-TMC-20, 2026-10-03) |
| S8 | `raptor undo` sin flags deshace la última operación del worktree desde el que se invoca; si es de un actor distinto del solicitante, aplican BR-TMC-AUTH-001 y la regla de solape; nunca actúa sobre otros worktrees sin pedirlo (BR-TMC-WF-001; P10, P11, P14). | Confirmado (D-TMC-19, D-TMC-20, D-TMC-23, 2026-10-03) |
| S9 | El solicitante de un undo se atribuye como los eventos ("agente X" o "sin atribuir"). Atribuido a un agente, solo deshace lo suyo; "sin atribuir", deshacer trabajo de otro actor exige confirmación interactiva del desarrollador en ese momento, que un agente no puede dar (BR-TMC-AUTH-001; P8, P14). | Confirmado (D-TMC-17, D-TMC-23, 2026-10-03); en Windows, rechazo sin confirmación en el MVP (TQ-14) |

---

## Riesgos

| # | Riesgo | Prob. | Impacto | Mitigación (de negocio) |
|---|--------|-------|---------|-------------------------|
| R1 | Overhead del snapshot por encima de 200 ms en repos grandes (BRD § 10). | Media | Medio | Spike (a) del BRD § 13 antes de las historias de snapshot. |
| R2 | Operaciones de Git crudo sin snapshot previo (sin hooks propios, Q22): lo editado entre la última captura y una operación destructiva de Git crudo puede perderse. | Alta | Alto | Captura continua del working tree; snapshot previo vía hooks de Guardrails cuando existan; cobertura declarada en el timeline (P1). |
| R7 | Un agente lanza `raptor undo` desde su shell y deshace trabajo de otro actor. | Media | Crítico | El solicitante se atribuye como un evento; si queda "sin atribuir", deshacer lo de otro exige confirmación interactiva (P14); en Windows se rechaza (TQ-14) y por MCP siempre se rechaza (TQ-7). |
| R3 | Un undo por agente deshace trabajo de otro actor por una atribución errónea. | Media | Crítico | Atribución vigente (Q37), "sin atribuir" nunca entra en el undo por agente y el solape detiene el undo (P4). |
| R4 | Los snapshots ocupan demasiado disco. | Media | Medio | Retención configurable con aviso antes de purgar (P6). |
| R5 | Un secreto en un archivo ignorado se pierde al restaurar, o se copia si se incluyera. | Baja | Medio | No se incluyen ignorados ni la lista cerrada de credenciales sin seguimiento, y se avisa en la documentación (P7, TQ-16). Un secreto fuera de esa lista sí se captura y, en el MVP, no se puede borrar de los snapshots (TQ-17). |
| R6 | El usuario cree que el undo también deshace el remoto. | Media | Alto | Aviso explícito cuando lo deshecho ya está en el remoto (P5). |

---

## Decisiones tomadas

| # | Decisión | Fuente | Fecha | Decidido por | Reglas afectadas |
|---|----------|--------|-------|--------------|------------------|
| D-TMC-1 | El motor nunca emite "humano"; solo "agente X" (con origen) o "sin atribuir". Esta feature decide cómo se presenta (D-TMC-12). Refina BR-10 del BRD ("humano o agente X"). | Q34, Q35 (motor-local) | 2026-10-03 | Rene Bonilla (motor-local) | BR-TMC-CONS-005, BR-TMC-AUTH-001 |
| D-TMC-2 | El timeline y el undo por agente usan la atribución vigente; corregir reatribuye la sesión desde su inicio. | Q33, Q37 (motor-local) | 2026-10-03 | Rene Bonilla (motor-local) | BR-TMC-CONS-005, BR-TMC-WF-002 |
| D-TMC-3 | El motor no escribe; toda escritura en el repo de esta feature es de la Time Machine, explícita y recuperable. | Q21 (motor-local) | 2026-10-02 | Rene Bonilla (motor-local) | BR-TMC-CONS-004 |
| D-TMC-4 | Los hooks de Git son de Guardrails; la Time Machine no instala hooks. | Q22 (motor-local) | 2026-10-02 | Rene Bonilla (motor-local) | BR-TMC-CONS-003 |
| D-TMC-5 | Configuración en tres niveles que se lee; cada valor declara los niveles que admite; el comando de edición es de Guardrails. | Q23, Q24, Q27 (motor-local) | 2026-10-02 | Rene Bonilla (motor-local) | BR-TMC-TIME-001 |
| D-TMC-6 | Soporte completo solo de Claude Code; los demás, "otro agente" por registro. | Q32 (motor-local), D2 | 2026-10-03 | Rene Bonilla (motor-local) | — |
| D-TMC-7 | Observación sin huecos; lo ocurrido en un hueco queda "sin atribuir" y el timeline lo muestra. | Q1, Q6, BR-EDGE-005 (motor-local) | 2026-10-02 | Rene Bonilla (motor-local) | BR-TMC-EDGE-002 |
| D-TMC-8 | Fuera de alcance: deshacer en el remoto, app de escritorio y Entire Checkpoints. | BRD § 6 | — | BRD | BR-TMC-EDGE-001 |
| D-TMC-9 | La cobertura en dos niveles (garantizada para lo lanzado por GitRaptor; por observación para el Git crudo) **refina BR-08 y NFR-01** del BRD, que piden snapshot "antes de cada operación" y "toda operación destructiva". Confirmada con D-TMC-10. | BRD § 6.1, § 7; Q22 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-001, BR-TMC-CONS-003 |
| D-TMC-10 | (P1) Cobertura en dos niveles: (a) garantizada antes de toda operación lanzada por GitRaptor (CLI, TUI/Cockpit, MCP) y de cada undo o restauración; (b) para Git crudo y ediciones externas, captura continua del working tree más snapshot previo vía hooks de Guardrails cuando existan, sin depender de ellos. La cobertura de cada tipo de operación se declara. | P1 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-001, BR-TMC-CONS-003 |
| D-TMC-11 | (P2) Los snapshots no se empujan al remoto por accidente, no los borra un `git gc` y un agente que trabaja en el working tree no los altera. El mecanismo y la ubicación los decide el Arquitecto con esas tres garantías. | P2 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-004 |
| D-TMC-12 | (P3) "Sin atribuir" se presenta como "Tú u otro (sin atribuir)", sin afirmar nunca que fue el humano. | P3 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-005 |
| D-TMC-13 | (P4) Un undo nunca sobrescribe trabajo posterior de otro actor en los mismos archivos o fragmentos: se detiene, muestra el solape y deja decidir al desarrollador. | P4 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-005, BR-TMC-WF-002 |
| D-TMC-14 | (P5) El undo es solo local; avisa si lo deshecho ya está en el remoto y nunca hace push ni force-push. | P5 | 2026-10-03 | Rene Bonilla | BR-TMC-EDGE-001 |
| D-TMC-15 | (P6) Retención configurable en perfil y local personal (no en equipo); 30 días por defecto; nunca se purga el snapshot previo a la última operación destructiva; se avisa antes de purgar. | P6 | 2026-10-03 | Rene Bonilla | BR-TMC-TIME-001 |
| D-TMC-16 | (P7; **actualizada por TQ-16**) El snapshot incluye los archivos sin seguimiento y no los ignorados por `.gitignore` (dependencias, `.env`). Además excluye por defecto una lista cerrada de archivos de credenciales sin seguimiento: `.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc`, `.netrc`, `*.tfstate*`, `credentials*.json`. Se declaran como exclusión; el perfil permite incluirlos; al restaurar no se tocan. El riesgo R5 queda documentado. | P7, TQ-16 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-002 |
| D-TMC-17 | (P8) Un solicitante atribuido a un agente solo deshace sus propias operaciones; deshacer trabajo de otro actor exige confirmación interactiva del desarrollador (D-TMC-23). Guardrails puede restringir más. | P8 | 2026-10-03 | Rene Bonilla | BR-TMC-AUTH-001 |
| D-TMC-18 | (P9) El registro de un undo (solicitante y sobre qué actuó) no se reescribe; el timeline muestra la atribución vigente. | P9 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-005 |
| D-TMC-19 | (P10) Timeline por repo, con filtros por worktree, agente y tiempo. | P10 | 2026-10-03 | Rene Bonilla | BR-TMC-CONS-005, BR-TMC-EDGE-002, BR-TMC-WF-001 |
| D-TMC-20 | (P11) Una restauración alcanza el worktree donde se pide y las ramas y worktrees que cambiaron después del punto; nada más. | P11 | 2026-10-03 | Rene Bonilla | BR-TMC-WF-003, BR-TMC-WF-001 |
| D-TMC-21 | (P12) La referencia de "repo mediano" para NFR-04 se fija en el spike (a). Sigue siendo una dependencia del Arquitecto. | P12 | 2026-10-03 | Rene Bonilla | — (NFR-04) |
| D-TMC-22 | (P13) Las historias de undo por agente quedan bloqueadas hasta que se cierre P17 de motor-local. | P13 | 2026-10-03 | Rene Bonilla | BR-TMC-WF-002 |
| D-TMC-23 | (P14) El solicitante de un undo, redo o restauración se atribuye como un evento ("agente X" o "sin atribuir", nunca "humano"). Atribuido a un agente, solo deshace lo suyo. "Sin atribuir": deshacer trabajo de otro actor exige una confirmación interactiva del desarrollador en ese momento, que un agente no puede dar. **Actualizada por TQ-14**: en el MVP esa confirmación solo se ofrece en macOS y Linux; en Windows, hasta que exista una forma fiable de probar que no la da un agente, la petición "sin atribuir" que toca trabajo de otro actor se rechaza con su motivo. En la Fase 2 se planea la presencia verificada por el SO (Touch ID, Windows Hello, polkit). Por MCP una petición "sin atribuir" se rechaza siempre (TQ-7). El mecanismo de identificación lo decide el Arquitecto. | P14, TQ-14, TQ-7 | 2026-10-03 | Rene Bonilla | BR-TMC-AUTH-001, BR-TMC-WF-001 |
| D-TMC-24 | (TQ-17) Borrar contenido ya capturado en snapshots (`raptor tm forget`) queda fuera del MVP y se aplaza a una US futura, que tendrá que conciliarse con D-TMC-15, BR-TMC-TIME-001 y BR-TMC-CONS-004. D-TMC-15 y BR-TMC-TIME-001 no cambian. | TQ-17 | 2026-10-03 | Rene Bonilla | — (pendiente futuro) |
| D-TMC-25 | Precisiones observables de las TQ aceptadas: (TQ-5) en la captura por observación se omiten los archivos que superan un tope de tamaño (captura parcial con la lista) y, al alcanzar la cuota del repo o el espacio libre mínimo, la captura se detiene con un hueco "sin espacio" declarado; el snapshot previo garantizado no tiene tope y la cuota no adelanta la purga; (TQ-9) undos seguidos forman una pila por worktree y una operación nueva invalida el redo; (TQ-10) un fallo detectado a mitad de un undo o restauración no hace rollback automático: queda "interrumpida" y se ofrece undo; (TQ-11) solo se purga si el aviso se mostró en la CLI o la TUI y pasaron 24 h; (TQ-15) los repos anidados sin seguimiento se excluyen, se declaran y nunca se escriben ni se borran al restaurar; al arrancar, la Time Machine libera su propio lock de Git sin tocar contenido. Cifras de TQ-5 según el spike. | TQ-5, TQ-9, TQ-10, TQ-11, TQ-15, ADR-TMC-003 | 2026-10-03 | Rene Bonilla | BR-TMC-WF-001, BR-TMC-CONS-002, BR-TMC-CONS-003, BR-TMC-CONS-004, BR-TMC-TIME-001, BR-TMC-EDGE-002, BR-TMC-EDGE-003 |

---

## Preguntas abiertas

| # | Pregunta | Para quién |
|---|----------|------------|
| P1 | ~~¿Cómo se cubre "antes de toda operación" sin hooks propios (Q22)?~~ **Resuelta** el 2026-10-03 (D-TMC-10): cobertura en dos niveles, declarada por tipo de operación. | — |
| P2 | ~~¿Dónde viven los snapshots?~~ **Resuelta** el 2026-10-03 (D-TMC-11): tres garantías de negocio (sin push accidental, sin borrado por `git gc`, sin alteración por agentes); mecanismo y ubicación para el Arquitecto. | Arquitecto (mecanismo) |
| P3 | ~~¿Cómo se presenta "sin atribuir"?~~ **Resuelta** el 2026-10-03 (D-TMC-12): "Tú u otro (sin atribuir)". | — |
| P4 | ~~`undo --agent X` con cambios posteriores de otro actor en los mismos archivos o fragmentos.~~ **Resuelta** el 2026-10-03 (D-TMC-13): se detiene, muestra el solape y decide el desarrollador. | — |
| P5 | ~~Undo de algo ya empujado al remoto.~~ **Resuelta** el 2026-10-03 (D-TMC-14): solo local, con aviso; nunca push ni force-push. | — |
| P6 | ~~Retención de snapshots.~~ **Resuelta** el 2026-10-03 (D-TMC-15): perfil y local personal; 30 días por defecto; protección del último snapshot previo a una operación destructiva; aviso antes de purgar. | — |
| P7 | ~~¿Qué incluye un snapshot?~~ **Resuelta** el 2026-10-03 (D-TMC-16): sin seguimiento sí, ignorados no. | — |
| P8 | ~~¿Quién puede deshacer qué?~~ **Resuelta** el 2026-10-03 (D-TMC-17): un agente, solo lo suyo; trabajo de otro actor, con confirmación interactiva. | — |
| P9 | ~~Reatribución (Q37) de eventos ya deshechos.~~ **Resuelta** el 2026-10-03 (D-TMC-18): el registro del undo no se reescribe. | — |
| P10 | ~~Granularidad del timeline.~~ **Resuelta** el 2026-10-03 (D-TMC-19): por repo, con filtros por worktree, agente y tiempo. | — |
| P11 | ~~¿Qué alcanza una restauración a un punto?~~ **Resuelta** el 2026-10-03 (D-TMC-20): worktree donde se pide más lo cambiado después del punto. | — |
| P12 | ~~¿Qué es un "repo mediano" para NFR-04?~~ **Resuelta** el 2026-10-03 (D-TMC-21): se fija en el spike (a). Sigue siendo una dependencia del Arquitecto. | Arquitecto (spike a) |
| P13 | ~~Dependencia de P17 de motor-local.~~ **Resuelta** el 2026-10-03 (D-TMC-22): las historias de undo por agente quedan bloqueadas hasta que se cierre P17 de motor-local. | Rene Bonilla (P17 de motor-local) |
| P14 | ~~¿Cómo se identifica a quien pide un undo y qué pasa si no se puede saber?~~ **Resuelta** el 2026-10-03 (D-TMC-23): se atribuye como un evento; "sin atribuir" exige confirmación interactiva para tocar trabajo ajeno; mecanismo para el Arquitecto. | Arquitecto (mecanismo) |

---

## ✅ Quality Review (Auto-evaluación del Contexto)

> Ejecutada el 2026-10-03 según `methodology.md` § 7. Resultado: **5 ✅ · 9 ⚠️ · 0 🔴**. Calidad global: 6/10, parcialmente listo: el alcance y las garantías están claros, pero varias reglas dependen de P1, P4, P6, P8 y P14.
>
> **Re-ejecución 2026-10-03 (RESERVAS del Artifact Judge)**: el solicitante de un undo deja de suponerse humano (Q34) y pasa a P14; AUTH-001 queda como supuesto; se fija el ámbito por defecto del undo (S8); D-TMC-9 declara que la cobertura en dos niveles refina BR-08 y NFR-01. Se añade la fila "Permisos del undo" (⚠️).
>
> **Re-ejecución 2026-10-03 (D-TMC-10 a D-TMC-23)**: Rene Bonilla cierra P1-P14. Cobertura, Integraciones, Permisos del undo, Restauración a un punto y Retención pasan a ✅. Resultado: **10 ✅ · 4 ⚠️ · 0 🔴**. Quedan pendientes los supuestos S5 y S6, que no dependían de P1-P14.
>
> **Nota 2026-10-03 (aceptación de riesgos)**: Rene Bonilla acepta los Known Risks 1, 3 y 5 y los supuestos S5 y S6. No cambia ninguna calificación: las cuatro secciones ⚠️ siguen con su riesgo, ahora aceptado. Resultado: **10 ✅ · 4 ⚠️ · 0 🔴**.

| Sección | Resultado | Nota |
|---------|-----------|------|
| Problema de Negocio | ✅ | Específico (BRD P4), con impacto en NFR-01 y en la demo. |
| Valor Esperado / ROI | ⚠️ | Sin línea base del tiempo de recuperación manual. |
| KPIs de Éxito | ✅ | Cinco métricas cuantificadas. |
| Usuarios/Actores | ✅ | Roles diferenciados; "sin atribuir" tratado como no actor; el solicitante nunca es "humano" (D-TMC-23). |
| Alcance OUT of scope | ✅ | 11 exclusiones, cada una con su dueño. |
| Restricciones | ⚠️ | Las garantías de los snapshots están decididas (D-TMC-11); la política interna de ASSA sigue sin confirmar. |
| RNFs | ⚠️ | "Repo mediano" se fija en el spike (a) (D-TMC-21): NFR-04 aún no es verificable. Retención decidida (D-TMC-15). |
| Integraciones | ✅ | Los hooks de Guardrails son opcionales; la cobertura no depende de ellos (D-TMC-10). |
| Glosario | ✅ | Términos clave definidos, incluidos cobertura, hueco, solape y solicitante. |
| Cobertura de snapshots | ✅ | Dos niveles decididos y declarados (D-TMC-9, D-TMC-10); el riesgo residual R2 sigue en Riesgos. |
| Undo por agente | ⚠️ | Solape y ámbito decididos (D-TMC-13, S8), pero sus historias quedan bloqueadas por P17 de motor-local (D-TMC-22). |
| Permisos del undo | ✅ | Solicitante atribuido y confirmación interactiva decididos (D-TMC-17, D-TMC-23); mecanismo para el Arquitecto. |
| Restauración a un punto | ✅ | Alcance decidido (D-TMC-20). |
| Retención | ✅ | Niveles, valor por defecto y protecciones decididos (D-TMC-15). |

## ⚠️ Known Risks (from Quality Review)

| # | Sección | Riesgo | Impacto | Aceptado por |
|---|---------|--------|---------|--------------|
| 1 | Valor Esperado / ROI | Sin línea base del tiempo de recuperación manual. | No se podrá demostrar el ahorro; solo el KPI de uso (≥ 3 undos). | Rene Bonilla (2026-10-03) |
| 2 | Restricciones | ~~Ubicación de los snapshots sin decidir (P2).~~ | **Resuelto** el 2026-10-03 (D-TMC-11): tres garantías de negocio; el mecanismo lo decide el Arquitecto. | — |
| 3 | RNFs | "Repo mediano" sin definir; se fija en el spike (a) (D-TMC-21). | NFR-04 no es verificable hasta el spike (a). | Rene Bonilla (2026-10-03) |
| 4 | Integraciones / Cobertura | ~~El Git crudo depende de hooks de Guardrails aún inexistentes (P1).~~ | **Resuelto** el 2026-10-03 (D-TMC-10). El riesgo de producto R2 (lo editado entre la última captura y una operación destructiva de Git crudo) sigue en Riesgos. | — |
| 5 | Undo por agente | Las historias de undo por agente dependen de P17 de motor-local (D-TMC-22). Solape y ámbito por defecto ya decididos (D-TMC-13, S8). | Esas historias no se pueden generar ni cerrar hasta que se resuelva P17; riesgo R3. | Rene Bonilla (2026-10-03) |
| 6 | Restauración a un punto | ~~Alcance supuesto (P11).~~ | **Resuelto** el 2026-10-03 (D-TMC-20). | — |
| 7 | Retención | ~~Valor por defecto y niveles supuestos (P6).~~ | **Resuelto** el 2026-10-03 (D-TMC-15). | — |
| 8 | Permisos del undo | ~~Identificación del solicitante y confirmación interactiva sin decidir (P8, P14).~~ | **Resuelto** el 2026-10-03 (D-TMC-17, D-TMC-23); el mecanismo de identificación lo decide el Arquitecto. | — |
