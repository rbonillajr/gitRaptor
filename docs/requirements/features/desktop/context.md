---
id: CTX-DSK-001
title: "Contexto — App de escritorio para novatos"
type: context
status: draft
created: 2026-10-28
updated: 2026-10-28
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
  - propuesta
---

# Contexto del Feature: App de escritorio para novatos

> **Origen**: decisión **D7** de Rene Bonilla (2026-10-28) y capacidad **BR-27** del [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.9, § 6.1 y § 12.1). La app de escritorio (Tauri + React) sale de BR-22 y entra en el MVP, en paralelo; la extensión de VS Code/Cursor sigue en la Fase 3 (BR-22). Épica **E-001**.
>
> **Estado**: propuesta del PO. **Rene revisa el alcance**: lo marcado "Pendiente de Rene" no está cerrado. Las historias de [user-stories.md](./user-stories.md) son contratos propuestos (`source: inline`), no aprobados.
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

El alcance va en niveles. **UI-1** es la primera versión útil para un novato (unas 3 semanas, ⚠️ **ASSUMPTION** S-DSK-1; la estima el Scrum Master). **UI-2** es lo que hace falta para que la app se sienta como GitKraken. **UI-3** son los opcionales, sin historia todavía. Cada historia está en [user-stories.md](./user-stories.md) con lo que reutiliza del backend y lo que falta.

### UI-1: primera versión útil para un novato (Must)

| Capacidad | Historias | Reutiliza |
|---|---|---|
| Añadir un repo sin terminal (incluidos los descubiertos) | US-DSK-001 | US-GRP-001, US-GRP-015, US-GRP-020, US-GRP-022; `repo.add`, `repo.locate` |
| Proteger un repo con consentimiento y poder quitarlo | US-DSK-002 | US-GRD-001, US-GRD-003, US-GRD-004, US-GRD-020; `guard.plan`, `guard.status`, `guard.install`, `guard.decline` |
| Estado del motor, de la conexión y estados vacíos | US-DSK-003 | US-CKP-003, US-GRP-004; `engine.snapshot`, `scope.snapshot`, `daemon.replace` |
| Flota de agentes en vivo | US-DSK-004 | US-CKP-001, US-CKP-002, US-CKP-026, US-GRP-007, US-GRP-012; `scope.subscribe`, `events.subscribe` |
| Grafo con un carril por agente | US-DSK-005 | US-CKP-022 (DS-US-CKP-022), `graph.lanes` (DEP-CKP-2) |
| Historial con "Deshacer" y "Rehacer" de un botón | US-DSK-006 | US-CKP-021, US-TMC-002, US-TMC-003, US-TMC-006, US-TMC-012, US-TMC-024; `timemachine.timeline`, `timemachine.undo`, `timemachine.redo`, `timemachine.notices` |
| Bloqueo de Guardrails explicado, con su excepción consciente | US-DSK-007 | US-CKP-019 (DS-US-CKP-019), US-GRD-005, US-GRD-006; `guard.log`, `audit.list`, `reserved.audit` |
| Conflictos previstos entre agentes | US-DSK-008 | US-CKP-006 a US-CKP-009, TS-CKP-001; `prediction.list`, `prediction.pairs`, `prediction.changed` |

### UI-2: sentirse como GitKraken

| Capacidad | Historias | MoSCoW | Imprescindible u opcional |
|---|---|---|---|
| Diff visual de una copia de trabajo y de un commit | US-DSK-009 | Must | Imprescindible |
| Commit desde la app (archivos completos) | US-DSK-010 | Must | Imprescindible |
| Staging por fragmento y por línea | US-DSK-011 | Should | Imprescindible |
| Ramas: crear, cambiar y borrar | US-DSK-012 | Must | Imprescindible |
| Integrar (merge) y poner al día (rebase) la rama de un agente | US-DSK-013 | Must | Imprescindible |
| Resolver un conflicto de merge dentro de la app | US-DSK-014 | Should | Imprescindible |
| Push, pull y fetch | US-DSK-015 | Must, **condicionado a Q-DSK-14** | Imprescindible |
| Preparar una copia de trabajo para un agente y descartar la de uno que terminó | US-DSK-016 | Should | Imprescindible |
| Búsqueda de commits, ramas y archivos | US-DSK-017 | Should | Imprescindible |
| Atajos de teclado y paleta de comandos | US-DSK-018 | Should | Imprescindible |

### UI-3: opcionales (sin historia todavía)

| Capacidad | MoSCoW | Motivo |
|---|---|---|
| Crear PRs en GitHub y Azure DevOps (enlaza BR-19, Fase 2) | Could | BR-19 es Fase 2; GitKraken ya lo hace bien (no rehacerlo, RES-GRP-COMP-2026-10 § 5.2, punto 8) |
| Cola de confirmación en la app (US-GRD-015) | Could | Depende de US-GRD-015 y del factor del SO; propongo sacarla de M2 (§ 10) |
| Rebase interactivo (reordenar, unir, editar commits) | Won't-now | Alto riesgo para un novato; exige operación nueva en el catálogo |
| Stash | Could | La Time Machine ya guarda lo sin commitear; el valor para el novato es bajo |
| Perfiles e identidades de Git (varias cuentas) | Could | Lo resuelve la configuración de Git del usuario (NFR-07) |
| Blame e historial de un archivo | Could | Útil para el revisor, no para el novato |
| Editar las políticas del repo desde la app (US-GRD-013) | Could | US-GRD-013 está pendiente y relajar exige el factor del SO (ADR-GRD-008 § 5) |
| Varias ventanas o pestañas multi-repo | Won't-now | Q-CKP-1: una vista por repo con selector |
| Notificaciones del sistema operativo | Could | Q-CKP-7 las dejó en Won't para la TUI; en una app de escritorio son naturales, pero no son de UI-1 |
| App en Windows y Linux | Pendiente de Rene (Q-DSK-12) | macOS primero |

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
| R-DSK-1 | La app compite con M2 por la revisión de Rene y el CI de macOS, que ya son el cuello de botella (release-plan, Ruta crítica); recortar Shoulds no libera revisión | Alta | Alto | Recorte de § 10; hito propio (Q-DSK-7) para que UI-1 no bloquee el cierre de M2; historias pequeñas |
| R-DSK-2 | **La app es un lanzador con ascendencia limpia**: un agente puede abrirla (`open -a`) y conseguir lo que la terminal le niega (riesgo A-2 de ADR-GRP-005, Validación 9) | Media | Crítico | Q-DSK-3: la app no rebaja ningún control; DEP-DSK-1 la resuelve el Arquitecto; revisión de seguridad de NFR-02 sobre la app |
| R-DSK-3 | Sin DEP-DSK-1, "añadir y proteger sin terminal" no se cumple: el onboarding cae a "copia este comando", que contradice la promesa al novato | Alta | Alto | Q-DSK-15: adoptar el factor del SO de ADR-GRD-008 si SPIKE-GRD-002 lo confirma en macOS; si no, la versión degradada y medirla |
| R-DSK-4 | El novato usa la app como cliente Git general, echa en falta commit y push y vuelve a GitKraken | Media | Alto | Q-DSK-10 y Q-DSK-13: decidir qué de UI-2 entra antes de v0.1.0 |
| R-DSK-5 | Instalar hooks sin explicarlo repite el incidente de GitKraken ([benchmark](./research/benchmark-2026-10.md) § 3, P10) | Media | Alto | Q-DSK-6: consentimiento explícito, qué se instala y "Desproteger" visible |
| R-DSK-6 | Expectativa de paridad con GitKraken (años de producto) y crecimiento del alcance | Alta | Medio | Niveles UI-1, UI-2 y UI-3 con MoSCoW; Rene cierra el alcance |
| R-DSK-7 | La app se queda lenta con 10 copias de trabajo o un grafo grande (NFR-04 y NFR-05 hablan de la TUI) | Media | Medio | El Arquitecto fija el NFR de la app (DEP-DSK-6) |
| R-DSK-8 | Windows no puede confirmar trabajo ajeno (BR-TMC-AUTH-001, ADR-TMC-005): el "Deshacer" de lo de un agente no funciona igual en los tres SO | Alta (en Windows) | Medio | Q-DSK-12: macOS primero; Windows declarado como diferencia |

## 8. Dependencias

| Id | Qué falta | Dueño | Historias afectadas |
|---|---|---|---|
| DEP-DSK-1 | **Prueba de presencia humana desde la app.** Añadir un repo, instalar la protección, la excepción consciente y confirmar trabajo ajeno exigen hoy terminal de control y líder de sesión limpio (ADR-GRP-005 § 6, controles 1 a 3; ADR-TMC-005 § 3). Una app gráfica no tiene terminal de control. Candidato a revisar: el factor del SO de ADR-GRD-008 (Touch ID o contraseña de sesión en macOS, SPIKE-GRD-002 en `ready`). transversal (lo define el Arquitecto) | Arquitecto; Rene decide si UI-1 lo espera (Q-DSK-15) | US-DSK-001, 002, 006, 007, 013, 016 |
| DEP-DSK-2 | Cómo se identifica la app ante el daemon (solicitante, capa y superficie) para que reciba lo mismo que la TUI y nunca más. transversal (lo define el Arquitecto) | Arquitecto | Todas |
| DEP-DSK-3 | Lecturas que la TUI no necesitaba: el diff de un commit cualquiera, los commits de una rama que no es de un agente y la búsqueda | Arquitecto | US-DSK-009, 012, 017 |
| DEP-DSK-4 | Operaciones que el catálogo no tiene: preparar y quitar del índice, cambiar de rama, crear y borrar una rama suelta, continuar un merge tras resolver, commit desde la capa del humano (hoy "No en el MVP" para el Cockpit, ADR-CKP-002 § 1) y la red (push, pull, fetch; ver Q-DSK-14). Cada una con punto previo (NFR-01) y decisión de Guardrails | Arquitecto (ADR-CKP-002) | US-DSK-010 a 016 |
| DEP-DSK-5 | Arrancar el motor desde la app y distribuir la app con el binario del motor (autoarranque de US-GRP-004 y DEP-CKP-12; ADR-GRP-014) | Arquitecto | US-DSK-003 |
| DEP-DSK-6 | Design system para la web (tokens de `packages/design-tokens`, ADR-GRP-003), estado del frontend (ADR-GRP-004) y NFR de rendimiento de la app. AGENTS.md y ADR-GRP-001 a 004 decían "no construir todavía": el BRD ya refleja D7; AGENTS.md y las ADRs los enmienda el orquestador o el Arquitecto | Arquitecto y orquestador | Todas |
| DEP-DSK-7 | i18n en/es en la app (NFR-10) y accesibilidad sin depender solo del color (NFR-09) | Diseño y Arquitecto | Todas |

## 9. Supuestos

| Id | Supuesto |
|---|---|
| S-DSK-1 | ⚠️ **ASSUMPTION**: UI-1 cabe en unas 3 semanas de la flota, porque casi todo el backend existe. Lo estima el Scrum Master cuando el Arquitecto cierre las enablers de la app |
| S-DSK-2 | ⚠️ **ASSUMPTION**: la app arranca en macOS (la plataforma del dogfooding y de M2) |
| S-DSK-3 | ⚠️ **ASSUMPTION**: el novato sabe qué es un commit y una rama, pero no usa la terminal a diario ni conoce worktrees, reflog ni rebase |
| S-DSK-4 | ⚠️ **ASSUMPTION**: la app reutiliza las reglas de negocio del Cockpit (BR-CKP-*), de la Time Machine y de Guardrails sin cambiarlas; no hay `business-rules.md` propio hasta que aparezca una regla que solo sea de la app |
| S-DSK-5 | ⚠️ **ASSUMPTION**: las metas de los KPIs de § 2 |

## 10. Recorte propuesto de M2

**Pendiente de Rene.** Objetivo: liberar unas 3 semanas de flota para UI-1 sin mover la fecha central de M2 (2027-01-14). Con la velocidad media de los últimos 14 días (0,86 fichas equivalentes por día, [release-plan](../../release-plan.md) § Ruta crítica), 3 semanas son unas **18 fichas equivalentes**. Fuente: la tabla de Composición de M2, fichas Should en estado Pend (y alguna P). No edito `release-plan.md` ni `backlog.md`: si Rene acepta, el orquestador mueve las fichas y enmienda el criterio de salida 1 de M2 ("o Rene lo sacó **de M2**", en vez de "del MVP").

**Tanda 1 (recomendada, unas 11 fichas más los 3 a 6 días que la cola de confirmación suma a la ruta)**

| # | Fichas | Qué | Mover a | Razón |
|---|---|---|---|---|
| 1 | US-GRD-015, US-CKP-023, US-MCP-014 | Cola de confirmación (Guardrails, TUI y MCP) | v0.1.0 o Fase 2 | Cadena bloqueada que el propio plan cifra en 3 a 6 días sobre la ruta; aprobar exige el factor del SO (ADR-GRD-008 § 5). En la app tendría mejor casa (UI-3). Hoy `ask` ya deniega a los agentes (Q-GRD-46), así que nadie queda desprotegido |
| 2 | US-GRD-013 | Configuración por comando | M3 o v0.1.0 | Espera a SPIKE-GRD-002 y a Q-GRD-32; editar la configuración a mano ya funciona y endurecer no pide factor |
| 3 | US-CKP-011 | Consulta de la flota y los conflictos por CLI | v0.1.0 | La app, la TUI y `check_conflicts` del MCP ya cubren esa consulta; es para scripts |
| 4 | US-TMC-008 | El timeline refleja las correcciones de atribución | v0.1.0 | Caso poco frecuente: depende de corregir atribuciones (US-GRP-010), que el novato no hará |
| 5 | US-TMC-010 | Deshacer los últimos N minutos | v0.1.0 | US-TMC-011 (`undo --agent --since`, Must y parte de la demo) y la restauración a un punto (US-TMC-009, implementada) cubren el caso |
| 6 | US-TMC-022 | Tope de disco fijado por el usuario | M3 | US-TMC-016 (Should, se queda) ya evita llenar el disco; el tope configurable es un ajuste fino |
| 7 | US-GRP-006 | Historial de un repo retirado y vuelto a añadir | v0.1.0 | Caso raro en el dogfooding; no afecta a la demo ni a los gates |
| 8 | US-GRP-013 | Umbral de inactividad por repo | v0.1.0 | El umbral global ya existe; ajuste fino |
| 9 | US-GRP-016 | La rama base la define la configuración del equipo | M3 | US-GRD-014 (implementada) ya fija y protege la base del equipo con `raptor guard confirm`; queda el resto del motor, que el Arquitecto debe confirmar que no hace falta para M2 |

**Tanda 2 (si la tanda 1 no llega a 3 semanas, unas 3,5 fichas)**

| # | Fichas | Qué | Mover a | Razón |
|---|---|---|---|---|
| 10 | US-CKP-004 y US-CKP-013 | Recordar la vista y abrir en el editor | v0.1.0 | Tienen Dev Spec desde el 2026-10-28 y son baratas, así que recortarlas ahorra poco; van juntas porque US-CKP-013 guarda su aprobación en las preferencias de US-CKP-004. La app las reimplementa a su manera |
| 11 | US-TMC-014 | Aviso de que lo deshecho sigue en el remoto | v0.1.0 | Útil, pero mientras GitRaptor no haga push (BR-CKP-CONS-002) es informativo |
| 12 | US-GRD-004 (lo pendiente) | Aviso de protección caída: escenario 3 y vigilante de `config` | M3 | Lo principal está hecho (PR #201); lo que falta es de repos retirados y de Linux y Windows |

**No recortar** (razón): US-TMC-003 (rehacer: lo pide el "Deshacer" de la app), US-TMC-007 (lo espera US-MCP-017), US-TMC-015 y US-TMC-016 (seguridad de datos, NFR-01), US-TMC-020 (criterio de salida 4 de M2), TD-TMC-001 (lo exige US-GRD-017), US-MCP-015 (criterio de salida 5 de M2), US-GRP-011 (la demo tiene 4 sesiones de Claude Code y puede haber worktrees compartidos), US-GRD-018 y US-CKP-025 (ya en parte).

**Cautela**: el plan dice que el cuello de botella son la revisión y el CI de macOS, no solo la flota (R-DSK-1). Recortar fichas libera flota, no horas de revisión de Rene. Por eso recomiendo además que UI-1 sea un hito propio que no bloquee el cierre de M2 (Q-DSK-7).

## 11. Decisiones

**Decisión del orquestador (2026-10-28), validada por PO** salvo las marcadas "Pendiente de Rene". Ninguna cambia una decisión de otra feature; donde una lo haría (Q-DSK-14), queda pendiente.

| # | Decisión | Estado |
|---|---|---|
| Q-DSK-1 | La app muestra solo lo que publica el motor por el canal; no lee Git ni el perfil ni calcula estado propio (la misma restricción dura del Cockpit, ADR-GRP-005 y ADR-GRP-013) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-2 | Toda escritura desde la app es una operación del catálogo ejecutada por el daemon, con punto previo de la Time Machine (NFR-01) y decisión de Guardrails. Si una operación no está en el catálogo, la app no la ofrece hasta que exista (DEP-DSK-4) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-3 | La app no rebaja ninguna autorización: lo que en la TUI exige un comando reservado, una confirmación o el factor del SO, en la app exige lo mismo o algo más fuerte (NFR-02, BR-AUTH-*). Mientras no exista DEP-DSK-1, la app explica la acción, muestra el comando exacto para la terminal, deja copiarlo y refleja el resultado en vivo | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-4 | Lenguaje llano para el novato: "copia de trabajo" para worktree, "punto de recuperación" para snapshot y "protección" para Guardrails, con el término técnico en un glosario contextual (benchmark § 3, P3). El texto final lo afina el diseñador | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-5 | Persona primaria de UI-1: el dev junior o semi-senior con agentes (BRD § 5) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-6 | Proteger un repo exige el consentimiento explícito del usuario, explica qué se instala y deja "Desproteger" a la vista (benchmark § 3, P10). Rechazar la protección queda recordado (`guard.decline`) | Decisión del orquestador (2026-10-28), validada por PO |
| Q-DSK-7 | ¿UI-1 es Must de M2 o un hito propio paralelo ("M2-UI")? | **Pendiente de Rene** |
| Q-DSK-8 | ¿Qué Shoulds de M2 se recortan para liberar las 3 semanas? (§ 10) | **Pendiente de Rene** |
| Q-DSK-9 | ¿La excepción consciente se hace dentro de la app en UI-1 o la app solo muestra el bloqueo y el comando? | **Pendiente de Rene** |
| Q-DSK-10 | ¿Qué de UI-2 entra antes de v0.1.0? | **Pendiente de Rene** |
| Q-DSK-11 | ¿En qué edición va la app: núcleo gratuito o edición de equipo? (pregunta abierta 5 del BRD) | **Pendiente de Rene** |
| Q-DSK-12 | ¿Plataformas de UI-1? (propuesta: solo macOS) | **Pendiente de Rene** |
| Q-DSK-13 | ¿Se soporta y se promociona el uso en un repo sin agentes (cliente Git general para novatos)? | **Pendiente de Rene** |
| Q-DSK-14 | ¿Puede la app hacer push, pull y fetch? Contradice BR-CKP-CONS-002 ("nunca push ni fetch") y la Validación 16 de ADR-CKP-002 | **Pendiente de Rene** (y del Arquitecto) |
| Q-DSK-15 | ¿UI-1 espera a la prueba de presencia humana (DEP-DSK-1) para añadir y proteger sin terminal, o sale con la versión degradada? | **Pendiente de Rene** (y del Arquitecto) |

## 12. Fricción con ids

La herramienta `aadd-us-target.mjs --reserve` (nassa-aadd 0.13.81), lanzada con `--cwd docs --feature desktop --slug DSK`, devolvió **US-DSK-001** en las 18 llamadas seguidas: una reserva del mismo worktree se reutiliza mientras el archivo no exista en disco (no hay `--count`), y una de las llamadas devolvió `reserved: false`. Además, el nombre del archivo sale del `--title`, así que con el título completo da un slug truncado (`US-DSK-001-el-novato-anade-su-repo-a.md`). Solución aplicada: reservar, escribir el archivo y volver a reservar; de US-DSK-002 a US-DSK-018 la herramienta devolvió el id siguiente en cada llamada. Los slugs de los archivos son cortos y los elegí yo (el de US-DSK-001 es `anadir-repo-sin-terminal`, no el truncado que propuso la herramienta). La consulta final sin reservar devuelve US-DSK-019 ([user-stories.md](./user-stories.md), "Comprobación de ids"). Propuesta para el plugin: un `--count N` o que la reserva no se reutilice dentro de la misma ejecución.

## 13. Revisión de calidad del contexto

| Sección | Estado | Riesgo aguas abajo |
|---|---|---|
| Problema y objetivo | Claros, con persona y KPI supuestos | Los KPIs son supuestos (S-DSK-5) |
| Alcance | En niveles, con MoSCoW y reutilización | Rene puede mover el corte entre UI-1 y UI-2 |
| Autorización desde la app | ⚠️ Sin resolver (DEP-DSK-1) | Alto: sin ella, UI-1 no cumple "sin terminal" (R-DSK-3) |
| Red (push y pull) | ⚠️ Contradice una regla vigente (Q-DSK-14) | US-DSK-015 no puede pasar a Dev Spec hasta que Rene decida |
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
- **UI-2**: el remoto Git del usuario, con sus credenciales, solo si Rene aprueba Q-DSK-14 (push, pull y fetch).
- **UI-3**: GitHub y Azure DevOps para crear PRs (BR-19, Fase 2).

## 17. Estándares Aplicables

- Revisión de seguridad OWASP y MCP Top 10 antes de cada release (NFR-02), ahora también sobre la app.
- Licencia FSL-1.1-ALv2 del núcleo (D4), salvo que Q-DSK-11 decida otra edición.
- Clean-room respecto a los competidores (NFR-11): GitKraken es la referencia de experiencia, no una fuente de código, iconos ni textos.
- Accesibilidad e i18n de NFR-09 y NFR-10.

---

## Decisiones de alcance para Rene

| # | Decisión | Opciones | Recomendación del orquestador | Impacto |
|---|---|---|---|---|
| 1 (Q-DSK-7) | ¿UI-1 dentro de M2 o hito propio? | (a) Must de M2; (b) hito propio paralelo "M2-UI", con criterio de salida: las 8 historias de UI-1 en macOS y la demo del novato (añadir, proteger, ver un choque, deshacer lo de un agente); (c) después de M2 | **(b)**: no ata el cierre de M2 a una feature nueva y deja medir la app aparte | (a) arriesga la fecha de M2; (c) contradice D7 ("ya y en paralelo") |
| 2 (Q-DSK-8) | ¿Qué Shoulds de M2 se recortan? | Tanda 1 (9 grupos, unas 11 fichas más la cola); tanda 1 y 2 (unas 14,5); ninguna | **Tanda 1**, y la 2 solo si la velocidad no repunta en 2 semanas | Libera 2,5 a 3 semanas de flota; no libera revisión (R-DSK-1). Hay que enmendar el criterio de salida 1 de M2 |
| 3 (Q-DSK-9) | Excepción consciente en la app en UI-1 | (a) dentro de la app, con DEP-DSK-1; (b) la app explica el bloqueo, muestra y copia el comando `raptor guard exec` y refleja el resultado; (c) solo el bloqueo | **(b)** en UI-1; (a) cuando la excepción adopte el factor del SO (ADR-GRD-008 § 5 la pone la última, por fatiga) | (a) exige una prueba nueva para un comando que relaja (D5 y D10); (b) cumple NFR-02 sin trabajo de backend |
| 4 (Q-DSK-15) | Añadir y proteger un repo sin terminal | (a) UI-1 espera a DEP-DSK-1 (factor del SO en macOS, tras SPIKE-GRD-002); (b) UI-1 sale con "copiar el comando" y el factor llega después | **(a) si SPIKE-GRD-002 se ejecuta esta semana; si no, (b)** y medir cuántos novatos se atascan | Es la promesa principal de UI-1 (R-DSK-3). Instalar no relaja, pero la app no tiene terminal de control |
| 5 (Q-DSK-10) | ¿Qué de UI-2 entra antes de v0.1.0? | (a) nada; (b) lo mínimo de un cliente diario: diff (US-DSK-009), commit (010), ramas (012), integrar (013) y push y pull (015, si Q-DSK-14 lo permite); (c) todo UI-2 | **(b)** | Sin (b), el novato vuelve a GitKraken para lo diario (R-DSK-4); (c) mueve v0.1.0 |
| 6 (Q-DSK-14) | ¿Push, pull y fetch desde la app? | (a) no, nunca (regla actual); (b) sí, solo como acción explícita del humano en la app, decidida por Guardrails, con punto previo para pull, y nunca por el MCP; (c) sí también para los agentes | **(b)** | Cambia BR-CKP-CONS-002 para la app y la Validación 16 de ADR-CKP-002 (Arquitecto). Sin push la app no es cliente diario |
| 7 (Q-DSK-11) | Edición de la app | (a) núcleo gratuito (FSL-1.1-ALv2); (b) edición de equipo; (c) UI-1 en el núcleo y lo de equipo (PRs avanzados de Azure DevOps, políticas centralizadas) de pago | **(c)**: el diferencial frente a GitKraken es "local, sin cuenta y gratis" (BRD § 3.4) | Cierra en parte la pregunta abierta 5 del BRD (BR-22 y la nueva BR-27) |
| 8 (Q-DSK-12) | Plataformas de UI-1 | (a) macOS; (b) macOS y Windows; (c) las tres | **(a)**, con Windows en M3 junto al resto | Windows no confirma trabajo ajeno (R-DSK-8) y no hay firma todavía (M4) |
| 9 (Q-DSK-13) | Repo sin agentes | (a) se soporta y se promociona como cliente Git general; (b) se soporta (la flota vacía es un estado válido; historial, deshacer y protección funcionan) sin promocionarlo hasta UI-2; (c) solo repos con agentes | **(b)** | (a) sin commit ni push decepciona; (c) deja fuera al novato antes de que use agentes |
