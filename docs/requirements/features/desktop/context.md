---
id: CTX-DSK-001
title: "Contexto — App de escritorio para novatos"
type: context
status: approved
created: 2026-10-28
updated: 2026-11-08
domain: GRP
epic: E-001
feature: desktop
scope: feature
stakeholders:
  - rene-bonilla
related:
  business:
    - BRD-GRP-001
  research:
    - RES-DSK-BENCH-2026-10
    - RES-GRP-COMP-2026-10
  design:
    - DSF-DSK-001
  context:
    - CTX-CKP-001
    - CTX-GRP-001
    - CTX-TMC-001
    - CTX-GRD-001
    - CTX-MCP-001
  adrs:
    - ADR-GRP-001
    - ADR-GRP-003
    - ADR-GRP-004
    - ADR-GRP-005
    - ADR-GRP-013
    - ADR-CKP-002
    - ADR-GRD-007
    - ADR-GRD-008
    - ADR-TMC-005
tags:
  - desktop
  - tauri
  - novatos
  - gui
  - mvp
  - aprobado
---

# Contexto del Feature: App de escritorio para novatos

> **Origen**: decisión **D7** de Rene Bonilla (2026-10-28) y capacidad **BR-27** del [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.9, § 6.1 y § 12.1). La app de escritorio (Tauri + React) sale de BR-22 y entra en el MVP, en paralelo; la extensión de VS Code/Cursor sigue en la Fase 3 (BR-22). Épica **E-001**.
>
> **Estado**: aprobado por Rene Bonilla el 2026-11-08, que aceptó las recomendaciones del orquestador para las 14 decisiones de alcance (Q-DSK-7 a Q-DSK-20, § 11). Las historias de [user-stories.md](./user-stories.md) pasan a `ready` con ese mismo origen. Siguen abiertas las enablers del Arquitecto (DEP-DSK-1 a 7), la enmienda de BR-CKP-CONS-002 (Q-DSK-14) y las acciones de Rene del final (compra de la cuenta de Apple Developer y reserva de los spikes).
>
> **Convención de ids**: lo propio lleva el prefijo DSK: decisiones **Q-DSK-n**, supuestos **S-DSK-n**, riesgos **R-DSK-n**, dependencias **DEP-DSK-n**. Lo ajeno se califica siempre ("Q-CKP-4", "BR-CKP-CONS-002", "US-GRD-006").
>
> **Restricción dura** (heredada de BRD § 11 y de BR-27): la app es un cliente más del motor, por el mismo canal que la TUI y la CLI. No lee Git, no abre el perfil y no tiene vía de escritura ni de autorización propia. Lo que le falta al motor se anota como dependencia (DEP-DSK-n) para el Arquitecto, sin diseñarlo aquí.
>
> **Documentos hermanos**: flujos y pantallas de UI-1 en [design-flow.md](./design/design-flow.md) (DSF-DSK-001, del diseñador UX); benchmark de GitKraken, GitHub Desktop, Fork y las apps de agentes en [research/benchmark-2026-10.md](./research/benchmark-2026-10.md) (RES-DSK-BENCH-2026-10); las ADRs de la app las escribe el Arquitecto.

---

## 1. Problema

Hoy GitRaptor solo se usa desde la terminal (TUI `raptor`, CLI) y desde el agente (MCP). Muchos de los usuarios que trabajan con agentes de IA son inexpertos en Git: no usan una TUI, y cuando un agente hace un `reset --hard`, un force-push o choca con otro agente, no saben qué pasó ni cómo volver atrás (P3 y P4 del BRD § 2). Es justo la persona "Dev junior o semi-senior con agentes" del BRD § 5, cuya necesidad principal es una red de seguridad, y hoy GitRaptor no le llega.

Las GUIs de Git que usa este público no le dan esa red: GitKraken deshace solo su última acción y cobra por las funciones de agentes en repos privados; GitHub Desktop no sabe nada de agentes; ninguna aplica políticas por repo ni predice choques entre worktrees ([benchmark](./research/benchmark-2026-10.md) § 4).

## 2. Objetivo

Que un novato, sin abrir una terminal, ponga su repo bajo GitRaptor y vea los tres pilares que nadie más le da: **el choque antes de que pase, el "Deshacer" que lo cubre todo y el bloqueo explicado**. Después, que la app sea suficiente como cliente Git diario para no tener que volver a GitKraken (UI-2).

**Medida de éxito** (⚠️ **ASSUMPTION** S-DSK-5, a validar por Rene):

| KPI | Meta | Fuente |
|---|---|---|
| Un novato añade y protege su primer repo desde la app sin ayuda | ≤ 5 min, sin terminal (si se resuelve DEP-DSK-1) | Prueba con usuario en el dogfooding |
| Undos ejecutados desde la app por usuario activo y mes | ≥ 3 (el mismo KPI del BRD § 9, medido aparte por superficie) | Timeline de la Time Machine (US-TMC-006) |
| Usuarios internos que usan la app cada semana | ≥ 3 de los ≥ 5 del BRD § 9 | Dogfooding |
| Incidentes de pérdida de datos causados por la app | **0** (NFR-01) | Registro de incidentes |

## 3. Usuarios

| Persona (BRD § 5) | Papel en la app | Prioridad |
|---|---|---|
| Dev junior o semi-senior con agentes | Usuario primario de UI-1: quiere ver qué hizo el agente y deshacerlo sin aprender Git | Primaria (Q-DSK-5) |
| "Agent wrangler" (dev senior) | Usa la app como panel de la flota y del grafo junto a la terminal | Secundaria |
| Tech lead / revisor | Revisa el diff de un agente e integra (UI-2) | Secundaria |
| El agente de IA | No usa la app. Nada de lo que la app ofrece puede dar al agente más poder del que tiene por el MCP o por Git directo | Restricción |

## 4. Hipótesis

1. **H-DSK-1**: si un novato puede deshacer lo que hizo un agente con un botón y ver por qué se frenó una operación, confía lo bastante en los agentes para dejarlos trabajar en paralelo. Se valida con el KPI de undos y con entrevistas en el dogfooding.
2. **H-DSK-2**: la app no compite con GitKraken en riqueza visual (BRD § 3.4, punto 6); gana con los tres pilares juntos, local, sin cuenta y gratis. Si en UI-1 los novatos piden sobre todo commit y push, UI-2 sube de prioridad (Q-DSK-10).
3. **H-DSK-3**: un cliente gráfico del mismo motor no añade riesgo de seguridad si no tiene ninguna vía propia de escritura ni de autorización (Q-DSK-2, Q-DSK-3). Se valida con la revisión de seguridad de NFR-02 aplicada a la app.

## 5. Alcance

El alcance va en niveles. **UI-1** es la primera versión útil para un novato (unas 3 semanas, ⚠️ **ASSUMPTION** S-DSK-1; la estima el Scrum Master). **UI-2** es lo que hace falta para que la app se sienta como GitKraken. **UI-3** son los opcionales, sin historia todavía. Cada historia está en [user-stories.md](./user-stories.md) con lo que reutiliza del backend y lo que falta. **Decisiones de Rene (2026-11-08) que acotan el alcance:** UI-1 es el hito propio paralelo **M2-UI** y no bloquea el cierre de M2 (Q-DSK-7); la plataforma es macOS y Windows entra en M3 (Q-DSK-12); UI-1 va en el núcleo gratuito y lo de equipo es de pago (Q-DSK-11); la app soporta un repo sin agentes (flota vacía como estado válido) sin promocionarlo como cliente Git general hasta UI-2 (Q-DSK-13); antes de v0.1.0 entran de UI-2 el diff, el commit, las ramas, integrar y push y pull (Q-DSK-10).

### UI-1: primera versión útil para un novato (Must)

| Capacidad | Historias | Reutiliza |
|---|---|---|
| Añadir un repo sin terminal (incluidos los descubiertos); sin el factor del SO, con el comando para copiar (Q-DSK-15) | US-DSK-001 | US-GRP-001, US-GRP-015, US-GRP-020, US-GRP-022; `repo.add`, `repo.locate` |
| Proteger un repo con consentimiento y poder quitarlo; sin el factor del SO, con el comando para copiar (Q-DSK-15) | US-DSK-002 | US-GRD-001, US-GRD-003, US-GRD-004, US-GRD-020; `guard.plan`, `guard.status`, `guard.install`, `guard.decline` |
| Estado del motor, de la conexión y estados vacíos | US-DSK-003 | US-CKP-003, US-GRP-004; `engine.snapshot`, `scope.snapshot`, `daemon.replace` |
| Flota de agentes en vivo | US-DSK-004 | US-CKP-001, US-CKP-002, US-CKP-026, US-GRP-007, US-GRP-012; `scope.subscribe`, `events.subscribe` |
| Grafo con un carril por agente | US-DSK-005 | US-CKP-022 (DS-US-CKP-022), `graph.lanes` (DEP-CKP-2) |
| Historial con "Deshacer" y "Rehacer" de un botón | US-DSK-006 | US-CKP-021, US-TMC-002, US-TMC-003, US-TMC-006, US-TMC-012, US-TMC-024; `timemachine.timeline`, `timemachine.undo`, `timemachine.redo`, `timemachine.notices` |
| Bloqueo de Guardrails explicado; la excepción consciente se hace con el comando `raptor guard exec`, que la app muestra y deja copiar (Q-DSK-9) | US-DSK-007 | US-CKP-019 (DS-US-CKP-019), US-GRD-005, US-GRD-006; `guard.log`, `audit.list`, `reserved.audit` |
| Conflictos previstos entre agentes | US-DSK-008 | US-CKP-006 a US-CKP-009, TS-CKP-001; `prediction.list`, `prediction.pairs`, `prediction.changed` |

### UI-2: sentirse como GitKraken

| Capacidad | Historias | MoSCoW | Imprescindible u opcional |
|---|---|---|---|
| Diff visual de una copia de trabajo y de un commit | US-DSK-009 | Must, antes de v0.1.0 | Imprescindible |
| Commit desde la app (archivos completos) | US-DSK-010 | Must, antes de v0.1.0 | Imprescindible |
| Staging por fragmento y por línea | US-DSK-011 | Should | Imprescindible |
| Ramas: crear, cambiar y borrar | US-DSK-012 | Must, antes de v0.1.0 | Imprescindible |
| Integrar (merge) y poner al día (rebase) la rama de un agente | US-DSK-013 | Must, antes de v0.1.0 | Imprescindible |
| Resolver un conflicto de merge dentro de la app | US-DSK-014 | Should | Imprescindible |
| Push, pull y fetch, solo como acción explícita del humano decidida por Guardrails (Q-DSK-14) | US-DSK-015 | Must, antes de v0.1.0 | Imprescindible |
| Preparar una copia de trabajo para un agente y descartar la de uno que terminó | US-DSK-016 | Should | Imprescindible |
| Búsqueda de commits, ramas y archivos | US-DSK-017 | Should | Imprescindible |
| Atajos de teclado y paleta de comandos | US-DSK-018 | Should | Imprescindible |

### UI-3: opcionales (sin historia todavía)

| Capacidad | MoSCoW | Motivo |
|---|---|---|
| Crear PRs en GitHub y Azure DevOps (enlaza BR-19, Fase 2) | Could | BR-19 es Fase 2; GitKraken ya lo hace bien (no rehacerlo, RES-GRP-COMP-2026-10 § 5.2, punto 8) |
| Cola de confirmación en la app (US-GRD-015) | Could | Depende de US-GRD-015 y del factor del SO; la cola sale de M2 (aplicado, § 10) |
| Rebase interactivo (reordenar, unir, editar commits) | Won't-now | Alto riesgo para un novato; exige operación nueva en el catálogo |
| Stash | Could | La Time Machine ya guarda lo sin commitear; el valor para el novato es bajo |
| Perfiles e identidades de Git (varias cuentas) | Could | Lo resuelve la configuración de Git del usuario (NFR-07) |
| Blame e historial de un archivo | Could | Útil para el revisor, no para el novato |
| Editar las políticas del repo desde la app (US-GRD-013) | Could | US-GRD-013 está pendiente y relajar exige el factor del SO (ADR-GRD-008 § 5) |
| Varias ventanas o pestañas multi-repo | Won't-now | Q-CKP-1: una vista por repo con selector |
| Notificaciones del sistema operativo | Could | Q-CKP-7 las dejó en Won't para la TUI; en una app de escritorio son naturales, pero no son de UI-1 |
| App en Windows y Linux | Could | Decisión de Rene (2026-11-08, Q-DSK-12): macOS primero; Windows en M3; Linux sin fecha |

## 6. Fuera de alcance

- La extensión de VS Code/Cursor (sigue en BR-22, Fase 3).
- Lanzar o conversar con agentes desde la app (BRD § 6.4: GitRaptor observa y protege, no orquesta).
- Cualquier vía de escritura que no pase por el catálogo de operaciones y el ejecutor del daemon (Q-DSK-2).
- Cualquier forma de autorizar en la app algo que en la TUI exija un comando reservado o una confirmación, con un control más débil (Q-DSK-3).
- Edición de equipo: políticas centralizadas (BR-23) y dashboard (BR-25).
- IA generativa (mensajes de commit, descripciones de PR): RES-GRP-COMP-2026-10 § 5.2, punto 8.

## 7. Riesgos

| Id | Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|---|
| R-DSK-1 | La app compite con M2 por la revisión de Rene y el CI de macOS, que ya son el cuello de botella (release-plan, Ruta crítica); recortar Shoulds no libera revisión | Alta | Alto | Recorte de § 10 (aplicado); hito propio M2-UI (Q-DSK-7) para que UI-1 no bloquee el cierre de M2; historias pequeñas |
| R-DSK-2 | **La app es un lanzador con ascendencia limpia**: un agente puede abrirla (`open -a`) y conseguir lo que la terminal le niega (riesgo A-2 de ADR-GRP-005, Validación 9) | Media | Crítico | Q-DSK-3: la app no rebaja ningún control; DEP-DSK-1 la resuelve el Arquitecto; revisión de seguridad de NFR-02 sobre la app |
| R-DSK-3 | Sin DEP-DSK-1, "añadir y proteger sin terminal" no se cumple: el onboarding cae a "copia este comando", que contradice la promesa al novato | Alta | Alto | Decisión de Rene (2026-11-08, Q-DSK-15 y Q-DSK-17): SPIKE-GRD-002 se ejecuta ya dentro de UI-1 y "sin terminal" espera al factor del SO; mientras tanto, la versión con el comando para copiar, y medirla |
| R-DSK-4 | El novato usa la app como cliente Git general, echa en falta commit y push y vuelve a GitKraken | Media | Alto | Q-DSK-10 (aplicado): diff, commit, ramas, integrar, push y pull entran antes de v0.1.0; Q-DSK-13: el uso sin agentes se soporta y no se promociona hasta UI-2 |
| R-DSK-5 | Instalar hooks sin explicarlo repite el incidente de GitKraken ([benchmark](./research/benchmark-2026-10.md) § 3, P10) | Media | Alto | Q-DSK-6: consentimiento explícito, qué se instala y "Desproteger" visible |
| R-DSK-6 | Expectativa de paridad con GitKraken (años de producto) y crecimiento del alcance | Alta | Medio | Niveles UI-1, UI-2 y UI-3 con MoSCoW; Rene cierra el alcance |
| R-DSK-7 | La app se queda lenta con 10 copias de trabajo o un grafo grande (NFR-04 y NFR-05 hablan de la TUI) | Media | Medio | El Arquitecto fija el NFR de la app (DEP-DSK-6) |
| R-DSK-8 | Windows no puede confirmar trabajo ajeno (BR-TMC-AUTH-001, ADR-TMC-005): el "Deshacer" de lo de un agente no funciona igual en los tres SO | Alta (en Windows) | Medio | Q-DSK-12 (decidido): macOS primero; Windows en M3, declarado como diferencia |

## 8. Dependencias

| Id | Qué falta | Dueño | Historias afectadas |
|---|---|---|---|
| DEP-DSK-1 | **Prueba de presencia humana desde la app.** Añadir un repo, instalar la protección, la excepción consciente y confirmar trabajo ajeno exigen hoy terminal de control y líder de sesión limpio (ADR-GRP-005 § 6, controles 1 a 3; ADR-TMC-005 § 3). Una app gráfica no tiene terminal de control. **Resuelto en propuesta por [ADR-GRP-017](../../../architecture/decisions/ADR-GRP-017-app-escritorio-cliente-daemon.md) (`proposed`)**: el factor del SO de ADR-GRD-008 (Touch ID o contraseña de macOS), invocado por el daemon, sustituye al control 3 para la app con identidad verificada; presencia de sesión solo para operaciones recuperables en local y factor por acción para lo reservado y lo remoto. Sin factor (hoy: SPIKE-GRD-002 sin hacer), la app es de solo lectura y muestra el comando para copiar. **Decisión de Rene (2026-11-08):** SPIKE-GRD-002 se ejecuta ya dentro de UI-1 (Q-DSK-17) y "sin terminal" espera al factor (Q-DSK-15) | Arquitecto (hecho en propuesta); Rene ejecuta el spike | US-DSK-001, 002, 006, 007, 013, 016 |
| DEP-DSK-2 | Cómo se identifica la app ante el daemon (solicitante, capa y superficie) para que reciba lo mismo que la TUI y nunca más. **Resuelto en propuesta por ADR-GRP-017 § 3** (identidad de app verificada por firma de código; sin firma Developer ID no hay presencia de sesión; si la lanza un agente, perfil `mcp`). Las alphas sin firmar quedan en solo lectura para la capa `cockpit` | Arquitecto (hecho en propuesta; spike pendiente del binding de firma) | Todas |
| DEP-DSK-3 | Lecturas que la TUI no necesitaba: el diff de un commit cualquiera, los commits de una rama que no es de un agente y la búsqueda | Arquitecto | US-DSK-009, 012, 017 |
| DEP-DSK-4 | Operaciones que el catálogo no tiene: preparar y quitar del índice, cambiar de rama, crear y borrar una rama suelta, continuar un merge tras resolver, commit desde la capa del humano (hoy "No en el MVP" para el Cockpit, ADR-CKP-002 § 1) y la red (push, pull, fetch; Q-DSK-14 decidida: acción explícita del humano, no del agente ni del MCP). Cada una con punto previo (NFR-01) y decisión de Guardrails | Arquitecto (ADR-CKP-002) | US-DSK-010 a 016 |
| DEP-DSK-5 | Arrancar el motor desde la app y distribuir la app con el binario del motor (autoarranque de US-GRP-004 y DEP-CKP-12; ADR-GRP-014) | Arquitecto | US-DSK-003 |
| DEP-DSK-6 | Design system para la web (tokens de `packages/design-tokens`, ADR-GRP-003), estado del frontend (ADR-GRP-004) y NFR de rendimiento de la app. **Hecho en este PR:** el BRD (D7, BR-27), AGENTS.md, ADR-GRP-001 a 004 (enmiendas del 2026-10-28) y DSYS-GRP-001 0.9 ya reflejan D7. Quedan los tokens web concretos (DSF-DSK-001, H-1) | Arquitecto y orquestador | Todas |
| DEP-DSK-7 | i18n en/es en la app (NFR-10) y accesibilidad sin depender solo del color (NFR-09) | Diseño y Arquitecto | Todas |

## 9. Supuestos

| Id | Supuesto |
|---|---|
| S-DSK-1 | ⚠️ **ASSUMPTION**: UI-1 cabe en unas 3 semanas de la flota, porque casi todo el backend existe. Lo estima el Scrum Master cuando el Arquitecto cierre las enablers de la app |
| S-DSK-2 | ⚠️ **ASSUMPTION**: la app arranca en macOS (la plataforma del dogfooding y de M2) |
| S-DSK-3 | ⚠️ **ASSUMPTION**: el novato sabe qué es un commit y una rama, pero no usa la terminal a diario ni conoce worktrees, reflog ni rebase |
| S-DSK-4 | ⚠️ **ASSUMPTION**: la app reutiliza las reglas de negocio del Cockpit (BR-CKP-*), de la Time Machine y de Guardrails sin cambiarlas; no hay `business-rules.md` propio hasta que aparezca una regla que solo sea de la app |
| S-DSK-5 | ⚠️ **ASSUMPTION**: las metas de los KPIs de § 2 |

## 10. Recorte de M2 (aplicado el 2026-11-08)

**Decisión de Rene (2026-11-08, Q-DSK-8): se aplica la tanda 1; la tanda 2 no se aplica.** Antes de mover cada ficha se comprobó su estado en `main` (frontmatter, backlog y `release-status.md`): solo US-TMC-010 está implementada (#319) y **no se recorta**; las otras 10 están en `draft` (US-CKP-023 en `blocked`) y se mueven. El detalle y las dudas abiertas están en [release-plan.md](../../release-plan.md#recorte-de-shoulds-de-m2-aplicado). Objetivo: liberar unas 3 semanas de flota para UI-1 sin mover la fecha central de M2 (2027-01-14). Con la velocidad media de los últimos 14 días (0,86 fichas equivalentes por día, [release-plan](../../release-plan.md) § Ruta crítica), 3 semanas son unas **18 fichas equivalentes**. Fuente: la tabla de Composición de M2, fichas Should en estado Pend (y alguna P). Las fichas se movieron en `release-plan.md` y `backlog.md`, y el criterio de salida 1 de M2 dice ahora "o Rene lo sacó **de M2**" en vez de "del MVP".

**Tanda 1 (aplicada; 10 fichas movidas, sin US-TMC-010)**

| # | Fichas | Qué | Mover a | Razón |
|---|---|---|---|---|
| 1 | US-GRD-015, US-CKP-023, US-MCP-014 | Cola de confirmación (Guardrails, TUI y MCP) | v0.1.0 o Fase 2 | Cadena bloqueada que el propio plan cifra en 3 a 6 días sobre la ruta; aprobar exige el factor del SO (ADR-GRD-008 § 5). En la app tendría mejor casa (UI-3). Hoy `ask` ya deniega a los agentes (Q-GRD-46), así que nadie queda desprotegido |
| 2 | US-GRD-013 | Configuración por comando | M3 o v0.1.0 | Espera a SPIKE-GRD-002 y a Q-GRD-32; editar la configuración a mano ya funciona y endurecer no pide factor |
| 3 | US-CKP-011 | Consulta de la flota y los conflictos por CLI | v0.1.0 | La app, la TUI y `check_conflicts` del MCP ya cubren esa consulta; es para scripts |
| 4 | US-TMC-008 | El timeline refleja las correcciones de atribución | v0.1.0 | Caso poco frecuente: depende de corregir atribuciones (US-GRP-010), que el novato no hará |
| 5 | US-TMC-010 | Deshacer los últimos N minutos | **No se mueve: ya está implementada (#319)** (propuesta original: v0.1.0) | US-TMC-011 (`undo --agent --since`, Must y parte de la demo) y la restauración a un punto (US-TMC-009, implementada) cubren el caso |
| 6 | US-TMC-022 | Tope de disco fijado por el usuario | M3 | US-TMC-016 (Should, se queda) ya evita llenar el disco; el tope configurable es un ajuste fino |
| 7 | US-GRP-006 | Historial de un repo retirado y vuelto a añadir | v0.1.0 | Caso raro en el dogfooding; no afecta a la demo ni a los gates |
| 8 | US-GRP-013 | Umbral de inactividad por repo | v0.1.0 | El umbral global ya existe; ajuste fino |
| 9 | US-GRP-016 | La rama base la define la configuración del equipo | M3 | US-GRD-014 (implementada) ya fija y protege la base del equipo con `raptor guard confirm`; queda el resto del motor, que el Arquitecto debe confirmar que no hace falta para M2 |

**Tanda 2 (no se aplica)**

| # | Fichas | Qué | Mover a | Razón |
|---|---|---|---|---|
| 10 | US-CKP-004 y US-CKP-013 | Recordar la vista y abrir en el editor | v0.1.0 | Tienen Dev Spec desde el 2026-10-28 y son baratas, así que recortarlas ahorra poco; van juntas porque US-CKP-013 guarda su aprobación en las preferencias de US-CKP-004. La app las reimplementa a su manera |
| 11 | US-TMC-014 | Aviso de que lo deshecho sigue en el remoto | v0.1.0 | Útil, pero mientras GitRaptor no haga push (BR-CKP-CONS-002) es informativo |
| 12 | US-GRD-004 (lo pendiente) | Aviso de protección caída: escenario 3 y vigilante de `config` | M3 | Lo principal está hecho (PR #201); lo que falta es de repos retirados y de Linux y Windows |

**No recortar** (razón): US-TMC-003 (rehacer: lo pide el "Deshacer" de la app), US-TMC-007 (lo espera US-MCP-017), US-TMC-015 y US-TMC-016 (seguridad de datos, NFR-01), US-TMC-020 (criterio de salida 4 de M2), TD-TMC-001 (lo exige US-GRD-017), US-MCP-015 (criterio de salida 5 de M2), US-GRP-011 (la demo tiene 4 sesiones de Claude Code y puede haber worktrees compartidos), US-GRD-018 y US-CKP-025 (ya en parte).

**Cautela**: el plan dice que el cuello de botella son la revisión y el CI de macOS, no solo la flota (R-DSK-1). Recortar fichas libera flota, no horas de revisión de Rene. Por eso UI-1 es un hito propio, M2-UI, que no bloquea el cierre de M2 (Q-DSK-7).

## 11. Decisiones

Q-DSK-1 a 6 y 21 son decisión del orquestador (2026-10-28), validada por PO. Q-DSK-7 a 20 son **decisión de Rene (2026-11-08)**, que aceptó las recomendaciones del orquestador. Q-DSK-14 cambia una regla de otra feature (BR-CKP-CONS-002); la enmienda la escribe el Arquitecto.

| # | Decisión | Estado |
|---|---|---|
| Q-DSK-1 | La app muestra solo lo que publica el motor por el canal; no lee Git ni el perfil ni calcula estado propio (la misma restricción dura del Cockpit, ADR-GRP-005 y ADR-GRP-013) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-2 | Toda escritura desde la app es una operación del catálogo ejecutada por el daemon, con punto previo de la Time Machine (NFR-01) y decisión de Guardrails. Si una operación no está en el catálogo, la app no la ofrece hasta que exista (DEP-DSK-4) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-3 | La app no rebaja ninguna autorización: lo que en la TUI exige un comando reservado, una confirmación o el factor del SO, en la app exige lo mismo o algo más fuerte (NFR-02, BR-AUTH-*). Mientras no exista DEP-DSK-1, la app explica la acción, muestra el comando exacto para la terminal, deja copiarlo y refleja el resultado en vivo | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-4 | Lenguaje llano para el novato: "copia de trabajo" para worktree, "punto de recuperación" para snapshot y "protección" para Guardrails, con el término técnico en un glosario contextual (benchmark § 3, P3). El texto final lo afina el diseñador | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-5 | Persona primaria de UI-1: el dev junior o semi-senior con agentes (BRD § 5) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-6 | Proteger un repo exige el consentimiento explícito del usuario, explica qué se instala y deja "Desproteger" a la vista (benchmark § 3, P10). Rechazar la protección queda recordado (`guard.decline`) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-7 | ¿UI-1 es Must de M2 o un hito propio paralelo ("M2-UI")? | Decisión de Rene (2026-11-08): hito propio paralelo **M2-UI**, que no bloquea el cierre de M2. Criterio de salida: las 8 historias de UI-1 en macOS y la demo del novato (añadir, proteger, ver un choque, deshacer lo de un agente), más lo mínimo de UI-2 antes de v0.1.0 |
| Q-DSK-8 | ¿Qué Shoulds de M2 se recortan para liberar las 3 semanas? (§ 10) | Decisión de Rene (2026-11-08): tanda 1 de § 10; la tanda 2 no se aplica. US-TMC-010 no se recorta porque ya está implementada (#319) |
| Q-DSK-9 | ¿La excepción consciente se hace dentro de la app en UI-1 o la app solo muestra el bloqueo y el comando? | Decisión de Rene (2026-11-08): en UI-1 la app explica el bloqueo y muestra el comando `raptor guard exec` para copiarlo; dentro de la app cuando exista el factor del SO |
| Q-DSK-10 | ¿Qué de UI-2 entra antes de v0.1.0? | Decisión de Rene (2026-11-08): diff (US-DSK-009), commit (010), ramas (012), integrar (013) y push y pull (015) |
| Q-DSK-11 | ¿En qué edición va la app: núcleo gratuito o edición de equipo? (pregunta abierta 5 del BRD) | Decisión de Rene (2026-11-08): UI-1 en el núcleo gratuito (FSL-1.1-ALv2); lo de equipo, de pago |
| Q-DSK-12 | ¿Plataformas de UI-1? (propuesta: solo macOS) | Decisión de Rene (2026-11-08): macOS; Windows en M3 |
| Q-DSK-13 | ¿Se soporta y se promociona el uso en un repo sin agentes (cliente Git general para novatos)? | Decisión de Rene (2026-11-08): se soporta, con la flota vacía como estado válido; no se promociona hasta UI-2 |
| Q-DSK-14 | ¿Puede la app hacer push, pull y fetch? Contradice BR-CKP-CONS-002 ("nunca push ni fetch") y la Validación 16 de ADR-CKP-002 | Decisión de Rene (2026-11-08): sí, solo como acción explícita del humano en la app, decidida por Guardrails y con punto previo para pull; nunca por agentes ni por el MCP. BR-CKP-CONS-002 se enmienda (lo escribe el Arquitecto, fuera de este documento) y US-DSK-015 deja de estar condicionada |
| Q-DSK-15 | ¿UI-1 espera a la prueba de presencia humana (DEP-DSK-1) para añadir y proteger sin terminal, o sale con la versión degradada? | Decisión de Rene (2026-11-08): espera al factor del SO, porque SPIKE-GRD-002 se ejecuta ya (Q-DSK-17); mientras tanto, la versión con el comando para copiar |
| Q-DSK-16 | ¿Cuándo se compra la cuenta de Apple Developer para firmar y notarizar? (ADR-GRP-017) | Decisión de Rene (2026-11-08): en v0.1.0; las alphas sin firmar no tienen presencia de sesión (lectura y factor por acción). **La compra la hace Rene: pendiente** |
| Q-DSK-17 | ¿Se ejecuta ya SPIKE-GRD-002 (macOS) con el módulo de presencia, dentro de UI-1? (ADR-GRP-017) | Decisión de Rene (2026-11-08): sí, se ejecuta ya, dentro de UI-1 |
| Q-DSK-18 | ¿Updater de Tauri en v0.1.0: automático, a petición o ninguno? (ADR-GRP-017, NFR-03) | Decisión de Rene (2026-11-08): solo a petición |
| Q-DSK-19 | ¿Cask de Homebrew de la app además del `.dmg`? (ADR-GRP-017) | Decisión de Rene (2026-11-08): `.dmg` más cask de Homebrew en v0.1.0; las alphas, `.dmg` aarch64 sin firmar |
| Q-DSK-20 | Layout, tema, notificaciones, idioma y arranque del motor (DSF-DSK-001 § 10, R-1 a R-5) | Decisión de Rene (2026-11-08): Rene eligió la propuesta **B1** del mock, estilo "grafito glass": ventana flotante y paneles grafito redondeados (radio de unos 20 px, separación de 12 px), tipografía del sistema, azul para lo accionable, verde y naranja para el estado, coral ✳ para los agentes de Claude, tema claro y oscuro. Barra lateral única (Agentes con el número en vivo y Time Machine arriba; Repos como árbol de repo, copias de trabajo, ramas locales y remotas; Guardrails y Ajustes al pie; se pliega con ⌘B y ⌥⌘B). Centro: grafo de carriles fino con la fila de trabajo sin commit y ajustes de columnas, y barra de herramientas centrada (Deshacer, Rehacer, Fetch, Push, Rama, Worktree, Stash, Pop; "Integrar cambios" a la derecha). La Time Machine no tiene tira inferior: es una vista completa (filtros, línea de tiempo por actor con selección por arrastre y lista de operaciones con chip de reversibilidad y vista previa), desde la barra lateral o con ⌘Z. Detalle a la derecha: cambios sin commit con staging y compositor, o el commit, con el coautor en el detalle y no en la columna Author. Pantallas en [design-flow.md](./design/design-flow.md) y tokens en el [design system](../../../design-system/README.md) |
| Q-DSK-21 | La app es el cliente del canal que precisa ADR-GRP-017: el webview solo usa una lista cerrada de comandos IPC y el backend Tauri solo enlaza la biblioteca cliente del canal; el stack de ADR-GRP-001 no se reabre | Decisión del orquestador (2026-10-28), validada por Arquitecto y security-expert |

## 12. Fricción con ids

La herramienta `aadd-us-target.mjs --reserve` (nassa-aadd 0.13.81), lanzada con `--cwd docs --feature desktop --slug DSK`, devolvió **US-DSK-001** en las 18 llamadas seguidas: una reserva del mismo worktree se reutiliza mientras el archivo no exista en disco (no hay `--count`), y una de las llamadas devolvió `reserved: false`. Además, el nombre del archivo sale del `--title`, así que con el título completo da un slug truncado (`US-DSK-001-el-novato-anade-su-repo-a.md`). Solución aplicada: reservar, escribir el archivo y volver a reservar; de US-DSK-002 a US-DSK-018 la herramienta devolvió el id siguiente en cada llamada. Los slugs de los archivos son cortos y los elegí yo (el de US-DSK-001 es `anadir-repo-sin-terminal`, no el truncado que propuso la herramienta). La consulta final sin reservar devuelve US-DSK-019 ([user-stories.md](./user-stories.md), "Comprobación de ids"). Propuesta para el plugin: un `--count N` o que la reserva no se reutilice dentro de la misma ejecución.

## 13. Revisión de calidad del contexto

| Sección | Estado | Riesgo aguas abajo |
|---|---|---|
| Problema y objetivo | Claros, con persona y KPI supuestos | Los KPIs son supuestos (S-DSK-5) |
| Alcance | En niveles, con MoSCoW y reutilización; aprobado por Rene (2026-11-08) | El corte UI-1 y UI-2 está cerrado; lo mínimo de UI-2 antes de v0.1.0 es 009, 010, 012, 013 y 015 |
| Autorización desde la app | Decidido (Q-DSK-15 y 17): espera al factor del SO; SPIKE-GRD-002 se ejecuta ya. ⚠️ DEP-DSK-1 sigue abierta hasta que el spike confirme el factor en macOS | Alto: sin el factor, UI-1 sale con el comando para copiar y no cumple "sin terminal" (R-DSK-3) |
| Red (push y pull) | Decidido (Q-DSK-14): acción explícita del humano. ⚠️ BR-CKP-CONS-002 sigue vigente hasta que el Arquitecto la enmiende | US-DSK-015 no pasa a Dev Spec hasta que la enmienda esté escrita |
| Estimación | ⚠️ Supuesta (S-DSK-1) | El Scrum Master la recalcula |

## 14. Requisitos No Funcionales

Heredados del BRD § 7; los números propios de la app los fija el Arquitecto (DEP-DSK-6).

| NFR | Cómo aplica a la app |
|---|---|
| NFR-01 (cero pérdida de datos) | Toda escritura desde la app tiene punto previo recuperable (Q-DSK-2) |
| NFR-02 (seguridad) | La app no abre ninguna vía de autorización más débil que la TUI (Q-DSK-3); entra en la revisión de seguridad de cada release |
| NFR-03 (100 % local) | La app no envía código ni metadatos fuera de la máquina; sin cuenta |
| NFR-04 (frescura) | La app refleja los cambios del motor con la misma frescura percibida que la TUI (< 500 ms); ⚠️ **ASSUMPTION**: el Arquitecto confirma el número |
| NFR-05 (escala) | 10 copias de trabajo activas y repos de 100K commits sin degradarse (R-DSK-7) |
| NFR-09 (accesibilidad) | Navegable por teclado y sin depender solo del color |
| NFR-10 (i18n) | Inglés y español |

## 15. Restricciones Técnicas

Las define el Arquitecto; el PO solo recoge las heredadas que acotan el alcance:

- Tauri + React para la app (ADR-GRP-001), sobre el design system de ADR-GRP-003 y la guía de estado de ADR-GRP-004.
- Cliente del canal local del daemon, como la TUI y la CLI (ADR-GRP-005, ADR-GRP-013).
- Escrituras solo con el catálogo y el ejecutor del daemon (ADR-CKP-002) y la operación protegida de la Time Machine (ADR-TMC-004).
- Git del sistema y la configuración, los hooks y las credenciales del usuario (NFR-07).

## 16. Integraciones Externas

- **UI-1**: ninguna. Todo es local (NFR-03).
- **UI-2**: el remoto Git del usuario, con sus credenciales, para push, pull y fetch como acción explícita del humano (Q-DSK-14, decisión de Rene del 2026-11-08); nunca por agentes ni por el MCP.
- **UI-3**: GitHub y Azure DevOps para crear PRs (BR-19, Fase 2).

## 17. Estándares Aplicables

- Revisión de seguridad OWASP y MCP Top 10 antes de cada release (NFR-02), ahora también sobre la app.
- Licencia FSL-1.1-ALv2 del núcleo (D4); UI-1 va en el núcleo gratuito y lo de equipo es de pago (Q-DSK-11).
- Clean-room respecto a los competidores (NFR-11): GitKraken es la referencia de experiencia, no una fuente de código, iconos ni textos.
- Accesibilidad e i18n de NFR-09 y NFR-10.

---

## Acciones pendientes de Rene

| # | Acción | Cuándo | Por qué |
|---|---|---|---|
| 1 | **Comprar la cuenta de Apple Developer** (99 USD al año) para firmar y notarizar la app y el CLI | Para v0.1.0 (Q-DSK-16); hasta entonces las alphas salen sin firmar | Sin firma Developer ID verificada no hay presencia de sesión (ADR-GRP-017, revisión de seguridad A-1). **Pendiente** |
| 2 | **Reservar tiempo para ejecutar SPIKE-GRD-002 en su Mac** | Ya, dentro de UI-1 (Q-DSK-17) | Es lo que hace "sin terminal" verdad (Q-DSK-15) |

## Decisiones tomadas (2026-11-08)

Rene escribió "acepto las recomendaciones" el 2026-11-08: las 14 decisiones de alcance quedan resueltas con la opción recomendada, y la 14 con la propuesta B1 del mock. Cada una está en § 11 con su efecto.

| # | Decisión | Opción elegida | Efecto |
|---|---|---|---|
| 1 (Q-DSK-7) | ¿UI-1 dentro de M2 o hito propio? | (b) hito propio paralelo **M2-UI**, con las 8 historias de UI-1 en macOS y la demo del novato | No ata el cierre de M2 a una feature nueva y deja medir la app aparte |
| 2 (Q-DSK-8) | ¿Qué Shoulds de M2 se recortan? | Tanda 1; la tanda 2 no se aplica; US-TMC-010 se queda (implementada, #319) | M2 pasa de 112 a 102 fichas; criterio de salida 1 enmendado ("lo sacó de M2") |
| 3 (Q-DSK-9) | Excepción consciente en la app en UI-1 | (b) la app explica el bloqueo y muestra el comando `raptor guard exec` para copiarlo; dentro de la app cuando exista el factor del SO | Cumple NFR-02 sin trabajo de backend |
| 4 (Q-DSK-15) | Añadir y proteger un repo sin terminal | Esperar al factor del SO, porque SPIKE-GRD-002 se ejecuta ya (Q-DSK-17); mientras tanto, la versión con el comando para copiar | La promesa principal de UI-1 depende del spike |
| 5 (Q-DSK-10) | ¿Qué de UI-2 entra antes de v0.1.0? | (b) diff, commit, ramas, integrar y push y pull | El novato no vuelve a GitKraken para lo diario (R-DSK-4) |
| 6 (Q-DSK-14) | ¿Push, pull y fetch desde la app? | (b) solo como acción explícita del humano, decidida por Guardrails, nunca por agentes ni por el MCP | BR-CKP-CONS-002 se enmienda (Arquitecto); US-DSK-015 sin condición |
| 7 (Q-DSK-11) | Edición de la app | (c) UI-1 en el núcleo gratuito; lo de equipo, de pago | Cierra en parte la pregunta abierta 5 del BRD |
| 8 (Q-DSK-12) | Plataformas de UI-1 | (a) macOS; Windows en M3 | Windows no confirma trabajo ajeno (R-DSK-8) |
| 9 (Q-DSK-13) | Repo sin agentes | (b) se soporta, sin promocionarlo hasta UI-2 | La flota vacía es un estado válido |
| 10 (Q-DSK-16) | Cuenta de Apple Developer | (b) en v0.1.0; las alphas sin firmar no tienen presencia de sesión | **La compra la hace Rene (pendiente)** |
| 11 (Q-DSK-17) | SPIKE-GRD-002 (macOS) | (a) se ejecuta ya, dentro de UI-1 | Va con Q-DSK-15 |
| 12 (Q-DSK-18) | Updater de Tauri | (b) solo a petición | Cualquier updater abre red desde la app (NFR-03) |
| 13 (Q-DSK-19) | Canal de distribución | (b) `.dmg` más cask de Homebrew en v0.1.0; alphas: `.dmg` aarch64 sin firmar | Coherente con D5 (tap propio) |
| 14 (Q-DSK-20) | Layout, tema y demás | Propuesta B1 del mock, "grafito glass" | Resumen en § 11; pantallas en el design flow |
