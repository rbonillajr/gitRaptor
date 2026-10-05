---
id: CTX-CKP-001
title: "Contexto — Cockpit"
type: context
status: draft
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: cockpit
scope: feature
stakeholders:
  - rene-bonilla
related:
  rules:
    - BR-CKP-001
  context:
    - CTX-GRP-001
    - CTX-TMC-001
    - CTX-GRD-001
  adrs:
    - ADR-GRP-004
    - ADR-GRP-005
    - ADR-GRP-009
    - ADR-GRP-011
    - ADR-GRP-013
    - ADR-TMC-004
    - ADR-TMC-005
    - ADR-GRD-007
tags:
  - cockpit
  - tui
  - worktrees-en-vivo
  - grafo
  - prediccion-conflictos
  - acciones-por-agente
  - operacion-protegida
  - mvp
---

# Contexto del Feature: Cockpit

> **Fundamento**: enfoque híbrido BRD-PRD para arquitectura pre-desarrollo (framework AADD). Combina objetivos de negocio con requisitos de producto para informar las decisiones del Arquitecto.
>
> **Origen**: [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5) § 6.1, capacidades **BR-04, BR-05, BR-06 y BR-07** (todas Must). Épica **E-001**, feature **F-001-02** del [backlog](../../backlog.md). Reglas de negocio en [business-rules.md](./business-rules.md) (BR-CKP-001).
>
> **Decisiones heredadas**: el Cockpit parte de decisiones ya tomadas en el [Motor local](../motor-local/context.md), la [Time Machine](../time-machine/context.md) y [Guardrails](../guardrails/context.md), y de varios ADRs. Ninguna decisión de este documento cambia una decisión de otra feature. Ver [Decisiones heredadas](#decisiones-heredadas).
>
> **Convención de IDs**: lo propio lleva el prefijo CKP: preguntas **P-CKP-n**, decisiones **Q-CKP-n**, supuestos **S-CKP-n**, riesgos **R-CKP-n**, dependencias **DEP-CKP-n** y reglas **BR-CKP-<CAT>-NNN**. Lo ajeno se califica siempre: "Q24 de motor-local", "BR-WF-001 (Guardrails)", "BR-TMC-EDGE-004".
>
> **Restricción dura**: todo lo que el Cockpit muestra sale de lo que el motor publica (ADR-GRP-005, ADR-GRP-013). La TUI no lee Git, no abre el perfil y no calcula estado propio. Lo que falta en el motor se anota como dependencia (DEP-CKP-n), sin aplicarla aquí.

---

## 1. Visión General (BRD)

El Cockpit es la cara visible de GitRaptor en el MVP: la TUI `raptor`. Muestra en vivo qué agente trabaja en qué worktree, cómo crecen sus ramas sobre la rama base y qué pares van a chocar antes del merge. Desde la misma vista, el desarrollador actúa sobre el trabajo de cada agente: ver el diff, abrirlo en el editor, integrarlo, rebasarlo, descartarlo o crear un worktree para un agente nuevo.

El Cockpit no es dueño del estado. Presenta lo que publica el motor (F-001-01), escribe solo a través de la operación protegida de la Time Machine (F-001-03) y obedece la decisión de Guardrails (F-001-04). Su aporte propio es la presentación, el catálogo de acciones del usuario y la predicción de conflictos que el motor calcula y publica para todos los clientes.

### Problema de Negocio (BRD)

Un desarrollador que lanza de 3 a 10 agentes en paralelo no ve qué hace cada uno ni quién va a chocar con quién (BRD § 2, persona "Agent wrangler"). Hoy lo descubre al hacer merge, cuando el conflicto ya existe y el contexto de cada agente se perdió. Integrar el trabajo de un agente exige saltar entre terminales, `git worktree list`, `git log` y el editor.

Impacto de no resolverlo:
- Conflictos descubiertos tarde, con más coste de resolución y riesgo de perder trabajo.
- Tiempo de integración alto: el KPI del BRD pide −30 % frente a no usar GitRaptor (§ 9).
- La propuesta "ver todo de un vistazo, saber quién choca con quién, integrar rápido" (BRD § 5) queda sin cumplir.
- La demo del BRD § 13 ("el cockpit mostrando un conflicto antes de que ocurra") no se puede hacer.

### Valor Esperado (BRD)

- **ROI estimado**: no se cuantifica en dinero (herramienta interna, D1). El retorno es menos tiempo de integración y menos conflictos sorpresa.
- **KPIs de éxito**: ver § 3. Los principales: ≥ 70 % de los conflictos detectados antes y −30 % de tiempo de integración (BRD § 9).
- **Beneficiarios**: el desarrollador orquestador (directo); el Servidor MCP, que reutiliza la predicción publicada por el motor; la Time Machine y Guardrails, que ganan una superficie donde se ven sus avisos.

---

## 2. Dominio Específico (PRD)

- **Tipo de funcionalidad**: interfaz de terminal en vivo (lectura de lo publicado por el motor) con un catálogo de acciones de escritura gobernadas y protegidas.
- **Usuarios principales**: desarrollador orquestador (humano). Los agentes no usan el Cockpit; si una TUI se lanza desde el terminal de un agente, actúa como ese agente (Q-CKP-16).
- **Casos de uso principales**:
  - **Ver la flota de un vistazo**: worktrees del repo con su agente, estado de sesión, rama, archivos modificados, ahead/behind y última actividad (BR-04; BR-CKP-WF-001, BR-CKP-CALC-001).
  - **Ver crecer las ramas**: un carril por rama de worktree sobre la rama base confirmada (BR-05; BR-CKP-CALC-004).
  - **Saber quién va a chocar**: solapes y conflictos previstos por par, con archivos y hunks, antes de cualquier merge (BR-06; BR-CKP-CALC-002, BR-CKP-CALC-003, BR-CKP-WF-007).
  - **Revisar el trabajo de un agente**: ver el diff que entraría al merge y abrir el worktree en el editor (BR-07; BR-CKP-CALC-005, BR-CKP-ELIG-006).
  - **Integrar o actualizar la rama de un agente**: merge local a la base confirmada o rebase sobre ella, con snapshot previo y Deshacer (BR-07; BR-CKP-ELIG-002, BR-CKP-ELIG-003, BR-CKP-WF-002).
  - **Descartar el trabajo de un agente**: borrar worktree y rama, recuperable salvo lo que el snapshot no guarda (BR-07; BR-CKP-ELIG-004, BR-CKP-EDGE-008).
  - **Preparar un worktree para un agente nuevo**: crear rama y worktree desde la base confirmada; el Cockpit no lanza al agente (BR-07; BR-CKP-ELIG-005).
  - **Entender por qué una acción no se puede hacer**: motivo de la precondición o de la regla de Guardrails, con la acción que lo resuelve (BR-CKP-ELIG-001, BR-CKP-AUTH-001).
- **Alcance**:
  - **IN scope**:
    - Lista en vivo de worktrees y sesiones de un repo, con selector entre los repos observados (Q-CKP-1, Q-CKP-2, Q-CKP-3, Q-CKP-28).
    - Grafo en vivo acotado sobre la rama base confirmada (Q-CKP-4).
    - Presentación de la predicción de conflictos en dos niveles, con su antigüedad y sus límites declarados; cálculo y publicación por el motor (Q-CKP-5, Q-CKP-6, DEP-CKP-1).
    - Alertas dentro de la TUI (Q-CKP-7).
    - Acciones de BR-07 en la TUI: ver diff, abrir en el editor (Should), merge, rebase, descartar y crear worktree (Q-CKP-8 a Q-CKP-13).
    - Catálogo de operaciones del usuario y su ejecutor en el daemon, compartido con F-001-05 (ADR-TMC-002 § 5, ADR-TMC-007 § 2; DEP-CKP-7).
    - Presentación de estados del motor, de la conexión, de la base y del estado de protección de Guardrails (Q-CKP-22, Q-CKP-27; Q-GRD-25).
    - Presentación del timeline, del aviso de purga y del Deshacer de la Time Machine (BR-TMC-TIME-001, D-TMC-13).
    - Registro de predicciones y conflictos para el KPI de detección (Q-CKP-21).
    - CLI de solo lectura `raptor status` y `raptor conflicts` con `--json` (Should, Q-CKP-20).
    - Accesibilidad, i18n en/es y layout mínimo 80×24 (NFR-09, NFR-10, Q-CKP-18).
  - **OUT of scope**:

    | Exclusión | Dueño / motivo |
    |-----------|----------------|
    | App de escritorio (Tauri + React), extensión de VS Code/Cursor, UI kit React, Storybook, Motion | Fase 3 (BRD BR-22; AGENTS.md). Definiciones en ADRs solo como referencia |
    | Review queue (BR-17) | Fase 2 (BRD § 6.2) |
    | Creación de PRs (BR-19) | Fase 2 (BRD § 6.2) |
    | Push y cualquier operación contra el remoto | Nunca en el MVP del Cockpit (Q-CKP-10, ADR-TMC-004) |
    | Resolver conflictos dentro de la TUI | Fuera del MVP: se ofrece Abortar y Abrir en el editor (Q-CKP-11) |
    | Lanzar o detener agentes | Fuera de alcance del producto (BRD § 6.4); crear worktree muestra cómo lanzarlo (Q-CKP-13) |
    | Notificaciones del sistema operativo y campana | Won't en el MVP (Q-CKP-7) |
    | Vista agregada multi-repo | Fuera del MVP (Q-CKP-1) |
    | Acciones de BR-07 desde la CLI | Fuera del MVP; solo TUI (Q-CKP-20) |
    | Atribución por archivo en worktree compartido | Fase posterior (Q7 de motor-local) |
    | Aprobar en la cola de confirmación | Bloqueado hasta US-GRD-015 con el factor de ADR-GRD-008 (Q-CKP-14, DEP-CKP-8) |
    | Cálculo del estado, detección de sesiones, atribución | Motor local (F-001-01) |
    | Snapshots, undo y timeline como capacidad | Time Machine (F-001-03); el Cockpit los presenta |
    | Decisión de políticas y la cola como regla | Guardrails (F-001-04); el Cockpit las presenta |
    | Herramientas MCP (incluida `check_conflicts`) | Servidor MCP (F-001-05); reutiliza la predicción publicada |
    | Soporte completo de agentes distintos de Claude Code | Después del MVP (D2, Q32 de motor-local) |

### Dependencias con otras features

| Feature | Relación con el Cockpit | Dirección |
|---------|-------------------------|-----------|
| F-001-01 Motor local | Publica worktrees, sesiones, actor, ahead/behind, rama base confirmada y estados del motor; el Cockpit solo presenta. Faltan consultas y campos: DEP-CKP-2 a DEP-CKP-6, DEP-CKP-11, DEP-CKP-14. Presupuesto de frescura compartido (ADR-GRP-011). | Cockpit depende del motor |
| F-001-03 Time Machine | Toda escritura del Cockpit pasa por la operación protegida (D-TMC-10, ADR-TMC-004). El Cockpit presenta timeline, aviso de purga y Deshacer. El ejecutor de operaciones de usuario y el catálogo son del Cockpit y F-001-05 (ADR-TMC-002 § 5, ADR-TMC-007 § 2; DEP-CKP-7). | Bidireccional |
| F-001-04 Guardrails | Decide si cada acción del Cockpit procede (merge, rebase, crear y borrar worktree son operaciones gobernadas, BR-VAL-002 (Guardrails)). El Cockpit presenta estado de protección, denegaciones, excepción consciente y la cola. Falta la capa `cockpit` (DEP-CKP-10) y la cola publicada (DEP-CKP-8). | Cockpit depende de Guardrails |
| F-001-05 Servidor MCP | Comparte el catálogo de operaciones y el ejecutor (DEP-CKP-7). Su `check_conflicts` reutiliza la predicción que publica el motor (DEP-CKP-1). La CLI de solo lectura del Cockpit sigue sus restricciones: sin mensajes de commit ni contenido (Q-CKP-20). | Bidireccional |

---

## 3. Objetivos de Negocio (BRD)

| Objetivo | Métrica de Éxito | Prioridad |
|----------|------------------|-----------|
| Flota de un vistazo | Un cambio en un worktree se ve en la TUI en < 500 ms p95 de extremo a extremo con 10 worktrees y 100K commits (NFR-04, NFR-05); el Cockpit gasta ≤ 100 ms p95 de ese presupuesto (ADR-GRP-011) | Alta |
| Detectar los conflictos antes | ≥ 70 % de los conflictos reales tuvieron antes un ⚡ conflicto previsto del mismo par y archivo (BRD § 9; criterio en BR-CKP-CONS-005) | Alta |
| Integrar más rápido | −30 % en el tiempo entre el último commit de la rama y su merge. Línea base pendiente (Q-CKP-30, S-CKP-5) | Alta |
| Cero pérdida de datos | 0 incidentes de pérdida causados por el Cockpit; 100 % de sus escrituras pasan por la operación protegida (NFR-01, BR-CKP-CONS-002) | Alta |
| Demostrar el valor | La demo del BRD § 13 es reproducible como prueba de aceptación (Q-CKP-25) | Alta |
| Dogfooding | El Cockpit se usa en el 100 % de las sesiones con agentes al construir GitRaptor (BRD § 9) | Media |

---

## 4. Stakeholders y Actores (BRD + PRD)

### Stakeholders de Negocio (BRD)

| Stakeholder | Interés | Expectativa |
|-------------|---------|-------------|
| Rene Bonilla (producto, revisión e integración) | Único humano del proyecto (D3) y primer usuario. Integra el trabajo de varios agentes cada día. | Ver la flota y los choques sin salir de la terminal; integrar con una tecla y poder deshacer. |
| Equipos internos piloto (2, BRD § 9) | Usarán el Cockpit en sus repos. | `[POR VERIFICAR]` quiénes son (heredado de Guardrails). |

### Actores del Sistema (PRD)

| Actor | Descripción | Permisos/Capacidades |
|-------|-------------|----------------------|
| **Desarrollador orquestador** | Persona que supervisa la flota desde la TUI. | Ve todo lo publicado; lanza las acciones de BR-07 sujetas a precondiciones y a Guardrails; usa la excepción consciente cuando no desciende de un agente (Q-CKP-15). Se le presenta como "Tú u otro (sin atribuir)". |
| **Agente Claude Code** | Agente con soporte completo en el MVP (Q32 de motor-local). | Aparece en la lista con su nombre, color y símbolo. No usa el Cockpit; si lanza una TUI, esta actúa como él (Q-CKP-16). |
| **Otro agente** | Codex, Cursor u otro registrado de forma explícita (Q32 de motor-local). | Igual que Claude Code en la presentación. |
| **"Sin atribuir"** | Lo que el motor no asigna a ningún agente: el humano o un agente sin registrar (Q35 de motor-local). | Se presenta como "Tú u otro (sin atribuir)", nunca como "humano" (D-TMC-12). |
| **TUI lanzada desde el terminal de un agente** | El daemon resuelve el solicitante por ascendencia (ADR-TMC-005 § 1). | Actúa como ese agente; la vista lo dice; no puede usar la excepción consciente (Q-CKP-15, Q-CKP-16). |
| **Daemon / motor** | Proceso en segundo plano (ADR-GRP-005). | Fuente única de lo que se muestra; resuelve el solicitante; ejecuta las operaciones del catálogo; escribe el perfil. |
| **Git del sistema** | Ejecuta las escrituras. | El ejecutor lo invoca; el Cockpit respeta la config y los hooks del usuario donde el entorno lo permite (NFR-07, ADR-CKP-002). |

---

## 5. Restricciones y Limitaciones (BRD + Arquitectura)

### Regulatorias (BRD + Arquitectura)
- Sin restricciones regulatorias: herramienta interna y 100 % local (NFR-03). `[POR VERIFICAR]` la política interna de ASSA (heredado).
- El diff, los mensajes de commit y las rutas son confidenciales: no salen de la máquina ni se exponen por el MCP (NFR-03, Q-CKP-8, Q-CKP-20).

### Técnicas (Arquitectura)
- **Fuente única**: todo lo que la TUI muestra sale de lo que el motor publica (ADR-GRP-005, ADR-GRP-013). La TUI no lee Git, no abre el perfil y no embebe el motor (Q-CKP-22). Lo que falta es una dependencia DEP-CKP-n.
- **Una sola vía de escritura**: toda escritura del Cockpit es una operación del catálogo ejecutada por el daemon como operación protegida (intención, snapshot previo, ejecución, registro). Sin snapshot no hay operación (D-TMC-10, ADR-TMC-004). Nunca push.
- **Nunca escribir en el repo para predecir**: la predicción no deja rastro en el repo del usuario (NFR-01, ADR-GRP-009).
- **Cero pérdida (NFR-01)**: cada acción de escritura es recuperable con Deshacer, salvo lo que el snapshot no guarda, que se nombra antes (Q-CKP-12).
- **Guardrails decide**: el Cockpit no autoriza; la confirmación en la TUI es UX, no control (Q-CKP-16, ADR-GRP-005 § 6).
- **Texto no confiable**: todo texto del repo o de un agente se sanea antes de pintarlo (SEC-12, DEP-CKP-9).
- **Dependencias del motor**: BR-05, BR-06 y parte de BR-07 esperan consultas y artefactos del Arquitecto (ver [Dependencias del motor](#dependencias-del-motor-y-enmiendas-propuestas-no-aplicadas)).

### De Negocio (BRD)
- Una persona orquestando agentes (D3): historias pequeñas y verificables.
- Soporte completo solo para Claude Code (D2).
- BR-04 a BR-07 son Must; "abrir en el editor" y la CLI de solo lectura son Should (Q-CKP-9, Q-CKP-20). Orden de entrega: Q-CKP-24.
- Sin fecha objetivo para el MVP (BRD § 12.2).

---

## 6. Requisitos No Funcionales Destacados (PRD + Arquitectura)

| RNF | Valor Objetivo | Crítico | Justificación |
|-----|----------------|---------|---------------|
| **Frescura de la vista** | ≤ 100 ms p95 desde que el cliente recibe el evento hasta que lo pinta, dentro de los < 500 ms de extremo a extremo de NFR-04 (ADR-GRP-011). Gate de CI con backend sin pantalla (INF-GRP-002). Arranque y reconciliación fuera del gate, pero visibles como estado | Sí | "En vivo" es la promesa de BR-04. |
| **Frescura de la predicción** | Resultado actualizado ≤ 5 s p95 tras un commit con 10 worktrees. Fuera del gate de 500 ms. Cálculo inicial y tras mover la base: "calculando". Siempre con antigüedad visible (Q-CKP-6). ⚠️ **ASSUMPTION** (S-CKP-1) hasta SPIKE-CKP-001 | Sí | Una predicción vieja presentada como actual engaña. |
| **Escala** | 10 worktrees (hasta 55 pares) y repos de 100K commits sin degradarse (NFR-05) | Sí | Persona "Agent wrangler" (3-10 agentes). |
| **Feedback de la interfaz** | Respuesta visible a cada tecla en < 100 ms (ADR-GRP-004 § 3) | No | Sensación de herramienta profesional. |
| **Accesibilidad** | Color nunca como única señal; símbolos (● ◐ ○ ⚡ ⚠ ⛔ ⟲) con fallback ASCII; `NO_COLOR`, `--ascii`, alto contraste, `--plain`; teclado primero con `?` de ayuda; mínimo 80×24 (NFR-09, Q-CKP-18) | Sí | NFR-09. |
| **i18n** | Toda la TUI y la CLI en inglés y español (NFR-10) | No | Convención del producto. |
| **Cero pérdida** | 100 % de las escrituras por operación protegida (NFR-01) | Sí | Riesgo crítico del BRD § 10. |
| **Privacidad** | Diff y mensajes nunca salen de la máquina ni por el MCP (NFR-03) | Sí | Datos confidenciales. |
| **Salida segura** | Texto no confiable saneado antes de pintarlo (SEC-12) | Sí | Un nombre de rama puede llevar secuencias de control. |

---

## 7. Integraciones Externas (PRD + Arquitectura)

| Sistema/API | Propósito | Tipo de Integración | Criticidad |
|-------------|-----------|---------------------|------------|
| Daemon / motor (F-001-01) | Estado, eventos, consultas bajo demanda, ejecución de operaciones | Canal local con instantánea y suscripción (ADR-GRP-005, ADR-GRP-013). Contrato: DEP-CKP-6 | Alta |
| Time Machine (F-001-03) | Operación protegida, timeline, Deshacer, aviso de purga | Interna, a través del daemon | Alta |
| Guardrails (F-001-04) | Decisión por acción, estado de protección, excepción consciente, cola | Interna, a través del daemon (DEP-CKP-10, DEP-CKP-8) | Alta |
| Servidor MCP (F-001-05) | Catálogo y ejecutor compartidos; predicción reutilizada | Interna | Media |
| Git del sistema (≥ 2.38, NFR-07) | Ejecutar merge, rebase, worktree add/remove, branch delete | Solo a través del ejecutor del daemon | Alta |
| Editor del usuario | Abrir el worktree o un archivo | Proceso lanzado sin shell (Q-CKP-9, DEP-CKP-12) | Media |
| Terminal del usuario | Pintar la TUI | Sin APIs privadas (NFR-08) | Alta |

---

## 8. Características Únicas del Feature (PRD)

- **El choque se ve antes del merge**: conflicto previsto por par, con archivos y hunks, mientras los agentes siguen trabajando.
- **Dos niveles honestos**: solape (incluye lo sin commitear) y conflicto previsto (solo lo commiteado), con sus límites declarados.
- **Integrar sin miedo**: cada merge, rebase o descarte tiene snapshot previo y Deshacer.
- **Agente primero**: lo primero que se lee en cada fila es quién trabaja ahí.
- **Nunca miente sobre la frescura**: antigüedad visible del ahead/behind y de la predicción.
- **Explica cada "no"**: toda acción desactivada dice por qué y qué la desbloquea.

---

## 9. Glosario del Dominio (PRD)

| Término | Definición | Notas |
|---------|------------|-------|
| **Worktree en vivo** | Fila de la lista: un worktree del repo con sus sesiones (0..n), rama, archivos modificados, ahead/behind y última actividad, según lo publica el motor. | Q-CKP-2. |
| **Worktree principal** | El worktree original del repo. Fijo arriba de la lista; no se descarta. | Q-CKP-2, Q-CKP-12. |
| **Sesión** | Presencia de un agente en un worktree: Activo, Inactivo o Terminado. Una sesión terminada no se reactiva. | BR-WF-001 (motor-local), Q41 de motor-local. |
| **Worktree compartido** | Worktree con más de una sesión. Se marca y se muestran todas. | Q7 de motor-local. |
| **Último agente** | "Último agente: X (terminó hace N)", visible mientras exista el worktree. | Q-CKP-3, DEP-CKP-4. |
| **Rama base confirmada** | La rama base que el humano confirmó en su máquina; único valor que usan motor, Guardrails y Cockpit. Puede estar "no confirmada" o "pendiente". | Q-GRD-21, Q-GRD-23. |
| **Carril** | Línea del grafo para la rama de un worktree, desde su merge-base con la base confirmada. | Q-CKP-4. |
| **Par** | Dos lados que se comparan: un worktree y la base confirmada, o dos worktrees con trabajo propio. | Q-CKP-5. |
| **Solape** (⚠) | Mismos archivos modificados por los dos lados de un par, incluido lo sin commitear. | Q-CKP-5. |
| **Merge en seco** | Simulación de un merge que no escribe en el repo del usuario. | Q-CKP-5, ADR-GRP-009. Mecanismo: ADR-CKP-001. |
| **Conflicto previsto** (⚡) | El merge en seco de lo commiteado de un par detecta conflicto; se acompaña de archivos y hunks. | Q-CKP-5. |
| **Antigüedad de la predicción** | Tiempo desde que se calculó el resultado mostrado. | Q-CKP-6. |
| **Detectado antes** | Había un ⚡ del mismo par y archivo antes de empezar la operación que chocó. | Q-CKP-21. |
| **Operación protegida** | Intención → snapshot previo obligatorio → ejecución → registro. Sin snapshot, no se ejecuta. | D-TMC-10, ADR-TMC-004. |
| **Catálogo de operaciones** | Lista cerrada de operaciones de usuario con sus parámetros, ámbito y si es destructiva (ante la duda, destructiva). | ADR-TMC-007 § 2; DEP-CKP-7. |
| **Ejecutor de operaciones** | Parte del daemon que ejecuta las operaciones del catálogo, serializa por repo y revalida precondiciones. | ADR-TMC-002 § 5; DEP-CKP-7. |
| **Precondición** | Condición que debe cumplirse para ofrecer una acción (base confirmada, working tree limpio, sin sesión presente…). | BR-CKP-ELIG-001. |
| **Sesión presente** | Sesión Activa o Inactiva: el proceso del agente sigue vivo en ese directorio. | Q-CKP-10, Q-CKP-12. |
| **Operación en curso** | Merge, rebase u otra operación de Git a medias en un worktree. | Q-CKP-11, BR-TMC-EDGE-004. |
| **Excepción consciente** | Vía del humano para saltarse una regla de Guardrails en una operación concreta: anuncio, ventana cancelable, auditoría. | Q-GRD-1, Q-GRD-24, ADR-GRD-007. |
| **Solicitante** | Actor al que el daemon atribuye una acción por la ascendencia del proceso. | ADR-TMC-005 § 1, Q-CKP-16. |
| **Hueco de observación** | Periodo en que el motor no observó; lo ocurrido es "sin atribuir" y se destaca. | BR-EDGE-005 (motor-local), SEC-13. |
| **Sin atribuir** | Valor del motor para lo no asignado a un agente. Se presenta "Tú u otro (sin atribuir)". | Q34, Q35 de motor-local; D-TMC-12. |

---

## 10. Estándares Aplicables (Arquitectura)

- **Design system**: [DSYS-GRP-001](../../../design-system/README.md) (TUI, CLI, tokens, contenido): paleta `agent.1..8`, símbolos con fallback ASCII, componentes PolicyBanner, ConflictAlert, ConfirmPrompt y toast.
- **UX**: ADR-GRP-004 **solo § 3** (principios de UX adaptados a la TUI): feedback < 100 ms; deshacer antes que confirmar; confirmar solo lo irreversible, con default No; teclado primero; errores accionables; vacíos útiles. El resto de ADR-GRP-004 es Fase 3.
- **Interoperabilidad**: Git nativo; sin APIs privadas (NFR-08).
- **Seguridad**: sin autenticación de usuarios; el solicitante lo resuelve el daemon (transversal, lo define el Arquitecto).

---

## 11. Referencias (BRD + PRD + Arquitectura)

- [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5): § 5, § 6.1 (BR-04 a BR-07), § 6.2 (BR-17, BR-19), § 6.4, § 7 (NFR-01 a NFR-12), § 9 (KPIs), § 13 (demo).
- [Contexto del Motor local](../motor-local/context.md) (CTX-GRP-001) y [sus reglas](../motor-local/business-rules.md) (BR-GRP-001).
- [Contexto de la Time Machine](../time-machine/context.md) (CTX-TMC-001) y [sus reglas](../time-machine/business-rules.md) (BR-TMC-001).
- [Contexto de Guardrails](../guardrails/context.md) (CTX-GRD-001) y [sus reglas](../guardrails/business-rules.md) (BR-GRD-001).
- [ADRs](../../../architecture/decisions/): ADR-GRP-004, 005, 006, 009, 010, 011, 013; ADR-TMC-002 a 005, 007; ADR-GRD-003, 006, 007.
- [Requisitos no funcionales](../../../architecture/non-functional.md): SEC-02, SEC-08, SEC-12, SEC-13.
- [Design system](../../../design-system/README.md) (DSYS-GRP-001).
- [Reglas de negocio de esta feature](./business-rules.md) (BR-CKP-001).
- [Backlog](../../backlog.md): E-001 / F-001-02.

---

## Cobertura de BR-04…BR-07

Cada capacidad del BRD queda cubierta por decisiones, reglas y, donde el motor aún no lo publica, por dependencias.

| Capacidad (BRD) | Decisiones | Reglas | Dependencias | Cubierta |
|-----------------|------------|--------|--------------|----------|
| **BR-04** Lista en vivo: rama, estado, archivos, ahead/behind, última actividad | Q-CKP-1, 2, 3, 17, 18, 19, 22, 23, 28, 29 | CALC-001, WF-001, WF-004, WF-005, CONS-001, CONS-003, CONS-004, CONS-006, TIME-001, TIME-002, EDGE-001, EDGE-003, EDGE-004, EDGE-005, EDGE-006, VAL-002 | DEP-CKP-4, 6, 9, 11 | Sí |
| **BR-05** Grafo en vivo sobre la rama base | Q-CKP-4, 18, 27 | CALC-004, EDGE-001 | DEP-CKP-2 | Sí, condicionada a DEP-CKP-2 |
| **BR-06** Predicción de conflictos (archivos y hunks, entre agentes y con la base) y alerta | Q-CKP-5, 6, 7, 21, 25, 27 | CALC-002, CALC-003, WF-007, CONS-005 | DEP-CKP-1, 14 | Sí, condicionada a DEP-CKP-1 |
| **BR-07** Ver diff, abrir en editor, merge/rebase, descartar, crear worktree | Q-CKP-8 a 16, 19, 20, 26, 27 | VAL-001, VAL-003, CALC-005, ELIG-001 a 006, WF-002, WF-003, WF-006, AUTH-001 a 004, CONS-002, TIME-003, TIME-004, EDGE-002, EDGE-007, EDGE-008, EDGE-009 | DEP-CKP-3, 7, 8, 10, 12, 13 | Sí; cola bloqueada por DEP-CKP-8 |

Los IDs de reglas omiten el prefijo `BR-CKP-`.

---

## Dependencias del motor y enmiendas propuestas (no aplicadas)

Estas dependencias se anotan; **no se aplican** en este documento. Las resuelve el Arquitecto.

| DEP | Hueco | Artefacto a crear o enmendar |
|---|---|---|
| DEP-CKP-1 | Predicción de conflictos y solape publicados por el motor | SPIKE-CKP-001 (gix en memoria vs almacén alternativo en el perfil) → ADR-CKP-001. Si gana el almacén: enmiendas a ADR-GRP-009 (§ 2, § 3, Validación 5) y ADR-GRP-006 (almacén, cuota, limpieza). Necesita el conjunto completo de rutas sin commitear por worktree. |
| DEP-CKP-2 | Grafo: consulta de commits base..rama | Enmiendas a ADR-GRP-005 § 5 (consultas gix bajo demanda, hoy "desde memoria") y TS-GRP-004. Opcional: relación commit→evento (ADR-GRP-013). |
| DEP-CKP-3 | Contenido de diff bajo demanda | Mismas enmiendas que DEP-CKP-2 + ADR-GRP-009 Validación 8 (alcance del escaneo de secretos). |
| DEP-CKP-4 | Última actividad y última sesión por worktree publicadas | Enmienda a ADR-GRP-013 § 1/§ 6. |
| DEP-CKP-5 | Timeline en vivo | Enmienda a ADR-TMC-003 (evento de stream). |
| DEP-CKP-6 | Arranque coherente (instantánea con secuencia N + suscripción desde N+1) | Enmienda a TS-GRP-004; crear api-contract-ipc.md (citado en el overview, no existe). |
| DEP-CKP-7 | Catálogo de operaciones y ejecutor del daemon (propiedad del Cockpit) | ADR-CKP-002: catálogo, entorno del ejecutor, serialización, permisos (extensión de ADR-TMC-005), cuarentena al descartar; sin otra vía de escritura (ADR-TMC-004); ejecutor padre directo de git (ADR-GRD-003 § 4). Enmienda a ADR-GRP-009 Validación 5. |
| DEP-CKP-8 | Cola de confirmación publicada | US-GRD-015 (cola en el daemon y la CLI, con el factor de ADR-GRD-008, aceptado el 2026-10-04; su Dev Spec espera a SPIKE-GRD-002). Sin artefacto nuevo. US-GRD-015 ya no depende del Cockpit: el Cockpit la consume. |
| DEP-CKP-9 | Saneado SEC-12 en la TUI | Enmienda a ADR-GRP-004 (overview § 10.3). |
| DEP-CKP-10 | Capa `cockpit` en la decisión de Guardrails, registrada una sola vez | Enmiendas a ADR-GRD-003 § 1 y ADR-GRD-006. |
| DEP-CKP-11 | Escrituras del Cockpit en el perfil (preferencias Q-CKP-17, registro KPI Q-CKP-21) vía daemon | Enmiendas a ADR-GRP-006 y TS-GRP-004. |
| DEP-CKP-12 | Lanzar procesos fuera de crates/git (editor, autoarranque) | Enmienda a ADR-GRP-009 Validación 5 (cierra overview § 10.5). |
| DEP-CKP-13 | Clave del editor solo en perfil y local personal | Enmiendas a ADR-GRP-007 y ADR-GRP-008. |
| DEP-CKP-14 | Estado en conflicto publicado (rutas sin fusionar, MERGE_HEAD/onto) | Enmiendas a ADR-GRP-010 § 4 y ADR-GRP-013 § 1. |

> **Nota sobre NFR-07**: el BRD justifica Git ≥ 2.38 por `merge-tree --write-tree`. Si SPIKE-CKP-001 elige el merge en memoria, esa justificación cambia; el mínimo de versión no cambia. Se anota para ADR-CKP-001.

---

## Decisiones heredadas

| Decisión | Qué fija para el Cockpit |
|----------|--------------------------|
| Q3 de motor-local | Umbral de inactividad de 5 minutos por defecto: separa Activo de Inactivo en la fila. |
| Q7 de motor-local | Worktree compartido: se marca y se muestran todas sus sesiones; sin atribución por archivo (también en el diff, Q-CKP-8). |
| Q12 de motor-local | El motor no consulta el remoto: el ahead/behind se presenta "según la copia local del remoto" con su antigüedad (Q-CKP-29). |
| Q24 de motor-local | Niveles admitidos por valor: el umbral de inactividad solo en perfil y local personal; la rama base solo en el equipo. La clave del editor sigue el mismo criterio (Q-CKP-9, DEP-CKP-13). |
| Q32 de motor-local | Soporte completo solo para Claude Code; el resto se presenta como "otro agente". |
| Q34, Q35 de motor-local | El actor es "agente X" o "sin atribuir"; nunca "humano". |
| Q41 de motor-local | Una sesión terminada no se reactiva; si el agente vuelve, es una sesión nueva en la fila. |
| Q42 de motor-local | Si la rama base no existe, ahead/behind "no calculable"; nunca se elige otra (BR-CKP-EDGE-001). |
| Q-GRD-1, Q-GRD-24 (Guardrails) | Una acción del humano en el Cockpit que choca con una regla es una excepción consciente con anuncio, ventana y auditoría. Base de Q-CKP-15 y Q-CKP-26. |
| Q-GRD-5 (Guardrails) | El conjunto mínimo solo deniega force-push y el borrado de la rama base: no protege el merge a la base (mitigación de R-CKP-3). |
| Q-GRD-13 (Guardrails) | Cómo se presenta la cola es del Cockpit y de la CLI; Guardrails exige que decida el humano. |
| Q-GRD-19 (Guardrails) | Aprobar en la cola exige el factor OS de ADR-GRD-008, que invoca el daemon y el Cockpit muestra como "esperando la autenticación del sistema"; sin él no se ofrece (Q-CKP-14). |
| Q-GRD-21, Q-GRD-23 (Guardrails) | Rama base confirmada, no confirmada o pendiente; la confirmación inicial nunca ocurre al añadir el repo (Q-CKP-27). |
| Q-GRD-25 (Guardrails) | El Cockpit presenta los 4 estados de protección y los 2 diagnósticos con su acción. |
| D-TMC-10 (Time Machine) | Toda operación lanzada por el Cockpit lleva snapshot previo garantizado. |
| D-TMC-12 (Time Machine) | "Sin atribuir" se presenta "Tú u otro (sin atribuir)". |
| D-TMC-13 (Time Machine) | El Deshacer se detiene ante solape con otro actor y lo muestra. |
| ADR-GRP-011 | Presupuesto de frescura: ≤ 100 ms p95 para el Cockpit dentro de los 500 ms de NFR-04, con gate de CI. |
| ADR-TMC-004 | Una sola vía de escritura: la operación protegida. |
| ADR-GRD-007 | Mecanismo de la excepción consciente: anuncio, ventana de 10 s (supuesto heredado, S-CKP-3), auditoría; rechazo por ascendencia de agente. |

---

## Supuestos

| # | Supuesto | Estado |
|---|----------|--------|
| S-CKP-1 | La predicción se actualiza ≤ 5 s p95 tras un commit con 10 worktrees, recalculando solo los pares del worktree que cambió (Q-CKP-6). | ⚠️ Sin validar hasta SPIKE-CKP-001 |
| S-CKP-2 | Una ventana de ≈ 50 commits por carril basta para el grafo; el resto se colapsa (Q-CKP-4). | ⚠️ La cifra la afina diseño |
| S-CKP-3 | La ventana cancelable de la excepción consciente es de 10 s. | Heredado de ADR-GRD-007; sin cifra propia |
| S-CKP-4 | El comportamiento en Linux y Windows (latencias, ejecutor, editor, ascendencia) es el mismo que en macOS. | ⚠️ Sin verificar; solo hay evidencia de macOS |
| S-CKP-5 | La línea base del −30 % se puede obtener de la historia previa de repos internos (Q-CKP-30). | ⚠️ Si no se obtiene, riesgo aceptado como en Guardrails |

## Riesgos

| # | Riesgo | Prob. | Impacto | Mitigación (de negocio) |
|---|--------|-------|---------|-------------------------|
| R-CKP-1 | Las dependencias del motor (DEP-CKP-1 a 14) bloquean BR-05, BR-06 y parte de BR-07. | Alta | Alto | Orden de entrega Q-CKP-24 (BR-04 primero, BR-05 al final); dependencias explícitas para el Arquitecto. |
| R-CKP-2 | Falsos positivos o negativos de la predicción (drivers de merge y `.gitattributes` del usuario no se aplican; lo sin commitear solo cuenta como solape). | Media | Medio | Límites declarados en la vista (BR-CKP-CALC-002); métrica de conflictos previstos que no ocurrieron (Q-CKP-21). |
| R-CKP-3 | Fricción del merge a una base protegida: cada merge pasa por excepción consciente y frena el −30 %. | Media | Medio | El conjunto mínimo no protege el merge a la base (Q-GRD-5); revisión con datos de dogfooding. **Aceptado por el orquestador, a confirmar por Rene Bonilla en el PR** (Q-CKP-26). |
| R-CKP-4 | El usuario no mira la TUI y no ve el ⚡ a tiempo (sin notificaciones del SO). | Media | Medio | Alertas en la TUI y toast (Q-CKP-7); CLI `raptor conflicts` (Q-CKP-20); revisar tras dogfooding. |
| R-CKP-5 | El ejecutor sin TTY rompe hooks, firma o editor de merge del usuario. | Media | Alto | Lo resuelve ADR-CKP-002 (entorno del ejecutor); el fallo se informa y la operación es recuperable. |
| R-CKP-6 | Descartar pierde lo que el snapshot no guarda (ignorados como `.env`, `node_modules`, credenciales, submódulos, archivos grandes). | Media | Alto | Confirmación obligatoria que nombra lo no recuperable (BR-CKP-EDGE-008); cuarentena evaluada en ADR-CKP-002. |
| R-CKP-7 | Una TUI lanzada por un agente actúa como agente y el humano no se da cuenta. | Media | Medio | La vista declara el solicitante; la excepción se rechaza por ascendencia (BR-CKP-AUTH-002). |
| R-CKP-8 | Texto no confiable (nombre de rama, mensaje de commit) inyecta secuencias en la terminal. | Baja | Alto | Saneado SEC-12 (BR-CKP-VAL-002, DEP-CKP-9). |
| R-CKP-9 | Rendimiento con 10 worktrees: 55 pares a recalcular. | Media | Medio | Recalcular solo los pares afectados; antigüedad visible; SPIKE-CKP-001. |

---

## Decisiones tomadas

**Decisión del orquestador (2026-10-04), validada por Arquitecto/PO.** Rene Bonilla delegó en el orquestador, el 2026-10-04, la autonomía para decidir las preguntas de esta feature y validarlas con los agentes PO y Arquitecto. Cada Q-CKP-n sale de P-CKP-n con el mismo número. Ninguna cambia una decisión de otra feature ni sale del MVP.

| # | Pregunta de origen | Decisión | Fecha | Decidido por | Validación | Reglas afectadas |
|---|--------------------|----------|-------|--------------|------------|------------------|
| Q-CKP-1 | P-CKP-1 | Una vista por repo con selector entre los repos observados; arranca en el repo del directorio actual o el último usado. Vista multi-repo fuera del MVP. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-001, CONS-006 |
| Q-CKP-2 | P-CKP-2 | La fila es el worktree con sus sesiones (0..n); el nombre del agente se lee primero. Se listan worktrees sin agente y el principal (fijo arriba). Compartido marcado con todas sus sesiones. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-001, EDGE-004 |
| Q-CKP-3 | P-CKP-3 | Terminado visible 24 h o hasta otra sesión en el worktree; luego oculto por defecto (filtro). "Último agente: X (terminó hace N)" mientras exista el worktree. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-001, TIME-002 |
| Q-CKP-4 | P-CKP-4 | Grafo: un carril por rama de worktree sobre la base confirmada, desde el merge-base, ≈ 50 commits por carril (el resto colapsado). Color por carril; actor por commit solo si el motor publica commit→evento, si no "sin atribuir". No es un cliente Git completo. Bajo demanda. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CALC-004 |
| Q-CKP-5 | P-CKP-5 | Dos niveles publicados por el motor: solape (⚠, incluye lo sin commitear) y conflicto previsto (⚡, merge en seco de lo commiteado, con archivos y hunks). Pares: cada worktree contra la base y cada par de worktrees con trabajo propio. Nunca escribe en el repo. Límites declarados. Mecanismo: SPIKE-CKP-001 → ADR-CKP-001. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CALC-002 |
| Q-CKP-6 | P-CKP-6 | Predicción fuera del gate de 500 ms; objetivo ≤ 5 s p95 (supuesto); "calculando" en el cálculo inicial y tras mover la base; antigüedad siempre visible; nunca un resultado viejo como actual. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CALC-003 |
| Q-CKP-7 | P-CKP-7 | Alertas solo en la TUI (ConflictAlert + toast al aparecer un ⚡ nuevo). Notificación del SO y campana: Won't. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-007 |
| Q-CKP-8 | P-CKP-8 | Ver diff: lo que entraría al merge y aparte lo sin commitear; bajo demanda, sin filtros, con topes y detección de binarios; nunca por el MCP; sin atribución por archivo en compartido. Choque con ADR-GRP-009 Validación 8 → DEP-CKP-3. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CALC-005, ELIG-006 |
| Q-CKP-9 | P-CKP-9 | Abrir en el editor (Should): configuración (perfil y local personal) o `$VISUAL`/`$EDITOR`, pedido al daemon; sin shell, metacaracteres rechazados; editor de terminal suspende la TUI; gráfico no bloquea; sin editor → error accionable. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | VAL-003, ELIG-006, EDGE-007 |
| Q-CKP-10 | P-CKP-10 | Merge local a la base confirmada en el worktree que la tiene sacada; nunca push. Precondiciones: base confirmada, destino limpio y sin sesión presente, sin operación en curso. Sesión Activa en el worktree del agente: aviso y confirmación "se integra hasta el commit X". Rebase en el worktree del agente: bloqueado con sesión presente; exige limpio. ⚡ → aviso y confirmación. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del PO: el merge no se bloquea con sesión Activa. Ajuste del Arquitecto: el rebase se bloquea con cualquier sesión presente | ELIG-001, ELIG-002, ELIG-003, CONS-002, EDGE-009 |
| Q-CKP-11 | P-CKP-11 | Merge/rebase que choca queda detenido como lo deja Git; se ofrece Abortar y Abrir en el editor; Deshacer solo tras abortar. No aborta solo ni resuelve en la TUI. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Arquitecto | WF-003, EDGE-002 |
| Q-CKP-12 | P-CKP-12 | Descartar: siempre operación protegida. Bloqueado con sesión presente, en el principal, en la rama base y en ramas protegidas. Sin trabajo sin integrar: sin confirmación, toast Deshacer. Con trabajo sin integrar: ConfirmPrompt (default No). Con lo no recuperable: confirmación obligatoria que lo nombra. HEAD separado: solo el worktree. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. El PO pedía confirmar con sesión Inactiva; se adopta el bloqueo del Arquitecto, que lo cubre | ELIG-004, EDGE-008 |
| Q-CKP-13 | P-CKP-13 | Crear worktree: rama nueva válida desde la base confirmada; ruta por defecto hermana del repo, configurable; ruta existente o no válida → error, nunca reutilizar. No lanza al agente. Bloqueado sin base confirmada. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | VAL-001, ELIG-005 |
| Q-CKP-14 | P-CKP-14 | Cola: pendientes con cuenta atrás, actor y regla. Aprobar solo con factor OS; rechazar es comando reservado sin ventana. Bloqueada hasta DEP-CKP-8. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-006, AUTH-004, TIME-003 |
| Q-CKP-15 | P-CKP-15 | Denegada por Guardrails: PolicyBanner ⛔ con regla y nivel; se ofrece la excepción consciente (ADR-GRD-007), rechazada si la TUI desciende de un agente. "Pedir confirmación" = denegar sin cola. Capa `cockpit` → DEP-CKP-10. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | AUTH-001, WF-002 |
| Q-CKP-16 | P-CKP-16 | Solicitante por ascendencia; TUI de un agente = ese agente, y la vista lo dice. La confirmación en la TUI es UX. Trabajo de otro actor: confirmación ligada al plan (ADR-CKP-002); en Windows, rechazo. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | AUTH-002, AUTH-003 |
| Q-CKP-17 | P-CKP-17 | Estado de la TUI por usuario en el perfil, guardado por el daemon → DEP-CKP-11. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Arquitecto | CONS-006 |
| Q-CKP-18 | P-CKP-18 | Mínimo 80×24; prioridad lista > alertas > grafo > detalle; el grafo colapsa primero; por debajo, mensaje de tamaño mínimo. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | EDGE-005 |
| Q-CKP-19 | P-CKP-19 | Varias TUIs permitidas; mismo estado; el ejecutor serializa por repo y revalida; una acción anunciada se ve en todas. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CONS-004 |
| Q-CKP-20 | P-CKP-20 | `raptor status` y `raptor conflicts` de solo lectura con `--json` (Should), sin mensajes de commit ni contenido. Acciones solo en la TUI. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CONS-007 |
| Q-CKP-21 | P-CKP-21 | KPI registrado por el daemon en el perfil, por repo, 90 días; por (par, archivo); "detectado antes" = ⚡ previo; huecos y merges remotos aparte; métrica complementaria; Git crudo → DEP-CKP-14. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CONS-005 |
| Q-CKP-22 | P-CKP-22 | Sin daemon: la TUI lo arranca o explica cómo; nunca embebe el motor. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-004 |
| Q-CKP-23 | P-CKP-23 | > 8 agentes: colores reutilizados; nombre y símbolo distinguen. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | EDGE-006 |
| Q-CKP-24 | P-CKP-24 | Orden de entrega: 1) BR-04 con estados del motor; 2) BR-06; 3) BR-07; 4) BR-05. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | Matriz de priorización |
| Q-CKP-25 | P-CKP-25 | La demo del BRD § 13 es prueba de aceptación: 4 sesiones, dos tocan el mismo archivo; ⚡ con archivo y hunk antes del merge; el merge real produce el conflicto previsto. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | CALC-002, CONS-005 |
| Q-CKP-26 | P-CKP-26 | Merge a base protegida: se mantienen Q-GRD-1/Q-GRD-24 (excepción consciente con ventana). No se refina Guardrails. Riesgo R-CKP-3 aceptado. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO. El PO sugería preguntar a Rene Bonilla; el orquestador decide no cambiar una decisión de Guardrails y lo anota en el PR | AUTH-001, ELIG-002 |
| Q-CKP-27 | P-CKP-27 | Base no confirmada o pendiente: predicción contra la base "pendiente"; merge, rebase y crear desactivados con motivo y acción; solapes entre worktrees siguen. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-005, ELIG-001 |
| Q-CKP-28 | P-CKP-28 | Orden: principal arriba; luego lo que pide atención (⚡, ⛔, hueco), Activo, Inactivo, Terminado, sin agente. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | WF-001 |
| Q-CKP-29 | P-CKP-29 | Ahead/behind "según la copia local del remoto" con su antigüedad. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | CALC-001 |
| Q-CKP-30 | P-CKP-30 | Línea base del −30 %: tiempo entre el último commit de la rama y su merge, desde la historia previa de repos internos; si no se obtiene, riesgo aceptado. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | — (KPI, § 3) |

## Preguntas abiertas

Todas las preguntas de esta feature están resueltas. La tabla se conserva como historia. Quedan **pendientes técnicos del Arquitecto**, que no son preguntas de negocio: SPIKE-CKP-001 → ADR-CKP-001 (mecanismo de predicción), ADR-CKP-002 (catálogo, ejecutor, merge sin worktree con la base sacada, cuarentena) y las enmiendas DEP-CKP-n. Queda también la **confirmación de Rene Bonilla en el PR** del riesgo R-CKP-3 (Q-CKP-26).

| # | Pregunta | Recomendación original del orquestador | Estado |
|---|----------|----------------------------------------|--------|
| P-CKP-1 | ¿Un repo o varios a la vez? | Una vista por repo con selector; multi-repo fuera del MVP. | Resuelta (Q-CKP-1) |
| P-CKP-2 | ¿Unidad de la lista? | El worktree con sus sesiones; también sin agente y el principal. | Resuelta (Q-CKP-2) |
| P-CKP-3 | ¿Cuánto sigue visible una sesión Terminado? | Hasta otra sesión o 24 h; luego oculta por defecto. | Resuelta (Q-CKP-3) |
| P-CKP-4 | ¿Qué muestra el grafo? | Carril por rama sobre la base, ventana acotada (≈ 50), no cliente Git completo. | Resuelta (Q-CKP-4) |
| P-CKP-5 | ¿Niveles y entradas de la predicción? | Solape y conflicto previsto; pares worktree-base y worktree-worktree; nunca escribe. | Resuelta (Q-CKP-5) |
| P-CKP-6 | ¿Frescura de la predicción? | Fuera del gate; ≤ 5 s p95; antigüedad visible. | Resuelta (Q-CKP-6) |
| P-CKP-7 | ¿Alertas? | Dentro de la TUI; sin SO ni campana. | Resuelta (Q-CKP-7) |
| P-CKP-8 | ¿Qué diff? | Lo que entraría al merge y aparte lo sin commitear; bajo demanda. | Resuelta (Q-CKP-8) |
| P-CKP-9 | ¿Abrir en el editor? | Editor del perfil o `$VISUAL`/`$EDITOR`, sin shell; error accionable. | Resuelta (Q-CKP-9) |
| P-CKP-10 | ¿Merge y rebase? | Local, nunca push; bloquear con sesión Activa; limpio; base calculable. | Resuelta (Q-CKP-10), con ajustes |
| P-CKP-11 | ¿Merge/rebase real que choca? | Detenido; Abortar, editor y Deshacer. | Resuelta (Q-CKP-11), Deshacer tras abortar |
| P-CKP-12 | ¿Descartar? | Protegido, toast Deshacer, confirmar con trabajo sin integrar; nunca principal, base ni sesión Activa. | Resuelta (Q-CKP-12), con ajustes |
| P-CKP-13 | ¿Crear worktree? | Rama nueva válida desde la base; ruta configurable; no lanza al agente. | Resuelta (Q-CKP-13) |
| P-CKP-14 | ¿Cola de confirmación? | Presentarla; aprobar con factor OS; rechazar sin factor. | Resuelta (Q-CKP-14) |
| P-CKP-15 | ¿Acción denegada por Guardrails? | PolicyBanner y excepción consciente. | Resuelta (Q-CKP-15) |
| P-CKP-16 | ¿Quién es el solicitante? | El que resuelva el daemon por ascendencia. | Resuelta (Q-CKP-16) |
| P-CKP-17 | ¿Persistencia del estado de la TUI? | En el perfil, por usuario. | Resuelta (Q-CKP-17) |
| P-CKP-18 | ¿Layout mínimo? | 80×24 con prioridades; grafo colapsa primero. | Resuelta (Q-CKP-18) |
| P-CKP-19 | ¿Varias TUIs? | Permitidas; acciones serializadas. | Resuelta (Q-CKP-19) |
| P-CKP-20 | ¿Equivalentes CLI? | `status` y `conflicts` de solo lectura con `--json`. | Resuelta (Q-CKP-20) |
| P-CKP-21 | ¿Cómo se mide el KPI del 70 %? | Registro de predicciones y conflictos en el perfil, 90 días. | Resuelta (Q-CKP-21), con criterio "solo ⚡" |
| P-CKP-22 | ¿Sin daemon? | Arrancarlo o explicar cómo; nunca embeber. | Resuelta (Q-CKP-22) |
| P-CKP-23 | ¿Más de 8 agentes? | Reutilizar colores; nombre y símbolo distinguen. | Resuelta (Q-CKP-23) |
| P-CKP-24 | ¿Orden de entrega? | BR-04, BR-06, BR-07, BR-05. | Resuelta (Q-CKP-24) |
| P-CKP-25 | ¿La demo es criterio de aceptación? | Sí. | Resuelta (Q-CKP-25) |
| P-CKP-26 | ¿Merge a base protegida sin excepción? | Mantener Guardrails; aceptar el riesgo. | Resuelta (Q-CKP-26) |
| P-CKP-27 | ¿Base no confirmada o pendiente? | Predicción "pendiente"; escrituras desactivadas con acción. | Resuelta (Q-CKP-27) |
| P-CKP-28 | ¿Orden de la lista? | Principal, atención, Activo, Inactivo, Terminado, sin agente. | Resuelta (Q-CKP-28) |
| P-CKP-29 | ¿Cómo se presenta ahead/behind? | "Según la copia local del remoto" con antigüedad. | Resuelta (Q-CKP-29) |
| P-CKP-30 | ¿Línea base del −30 %? | Historia previa de repos internos. | Resuelta (Q-CKP-30) |

> P-CKP-1 a P-CKP-23 llevan la recomendación del brief del orquestador; donde la validación la ajustó, la columna Estado lo indica. P-CKP-24 a P-CKP-30 no figuran en ese brief: se registran desde las decisiones finales y su recomendación coincide con la decisión.

---

## ✅ Quality Review (Auto-evaluación del Contexto)

> Revisión ejecutada el 2026-10-04 al terminar el documento (`methodology.md` § 7). Resultado: **9 ✅ · 5 ⚠️ · 0 🔴**. Las ⚠️ dependen de artefactos del Arquitecto (DEP-CKP-n, SPIKE-CKP-001, ADR-CKP-002) y de cifras sin validar; ninguna impide que el Arquitecto empiece.

| Sección | Resultado | Nota |
|---------|-----------|------|
| Problema de Negocio | ✅ | Específico (persona "Agent wrangler", conflictos tarde, demo § 13). |
| Valor Esperado / ROI | ⚠️ | ROI cualitativo; −30 % sin línea base (S-CKP-5). |
| KPIs de Éxito | ⚠️ | 70 % con criterio fijo (Q-CKP-21); −30 % sin línea base; frescura cuantificada. |
| Usuarios/Actores | ✅ | Incluye la TUI lanzada por un agente y la presentación de "sin atribuir". |
| Alcance OUT of scope | ✅ | 16 exclusiones con dueño o fase. |
| Restricciones | ✅ | Fuente única, una vía de escritura y saneado explícitos. |
| RNFs | ⚠️ | 100 ms y 500 ms heredados y firmes; 5 s de predicción es supuesto (S-CKP-1); Linux/Windows sin verificar (S-CKP-4). |
| Integraciones | ⚠️ | Claras, pero 14 dependencias del motor sin resolver (R-CKP-1). |
| Glosario | ✅ | 23 términos, incluidos solape, conflicto previsto, par, operación protegida y sesión presente. |
| Stakeholders | ✅ | Equipos piloto `[POR VERIFICAR]`, heredado. |
| Cobertura BR-04…07 | ✅ | Cada capacidad con decisiones, reglas y dependencias. |
| Predicción (BR-06) | ⚠️ | Niveles, pares y límites fijados; mecanismo y frescura esperan SPIKE-CKP-001. |
| Acciones (BR-07) | ✅ | Matriz de precondiciones completa (BR-CKP-ELIG-001); cola bloqueada y declarada. |
| Decisiones y trazabilidad | ✅ | 30 decisiones con validación; sin preguntas de negocio abiertas. |

## ⚠️ Known Risks (from Quality Review)

| # | Sección | Riesgo | Impacto | Aceptado por |
|---|---------|--------|---------|--------------|
| 1 | Valor Esperado / KPIs | Sin línea base del −30 %. | No se podrá demostrar la mejora si la historia previa no da datos. | Orquestador (Q-CKP-30); pendiente de Rene Bonilla |
| 2 | RNFs | 5 s p95 de predicción sin evidencia. | Las historias de BR-06 tendrán una cifra provisional hasta el spike. | Orquestador (Q-CKP-6) |
| 3 | Integraciones | DEP-CKP-1 a 14 sin resolver. | BR-05, BR-06 y parte de BR-07 no se cierran sin los artefactos del Arquitecto. | Orquestador (Q-CKP-24 ordena la entrega) |
| 4 | Predicción | Falsos positivos o negativos declarados. | El KPI del 70 % puede quedar por debajo por límites del merge en seco. | Orquestador (Q-CKP-5) |
| 5 | Acciones | Fricción del merge a base protegida (R-CKP-3). | Puede frenar el −30 %. | Orquestador; **a confirmar por Rene Bonilla en el PR** |
