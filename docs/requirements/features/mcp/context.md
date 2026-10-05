---
id: CTX-MCP-001
title: "Contexto — Servidor MCP"
type: context
status: draft
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
scope: feature
stakeholders:
  - rene-bonilla
related:
  rules:
    - BR-MCP-001
  context:
    - CTX-GRP-001
    - CTX-CKP-001
    - CTX-TMC-001
    - CTX-GRD-001
  stories:
    - US-GRD-016
  adrs:
    - ADR-GRP-001
    - ADR-GRP-005
    - ADR-GRP-006
    - ADR-GRP-009
    - ADR-GRP-013
    - ADR-TMC-002
    - ADR-TMC-004
    - ADR-TMC-005
    - ADR-GRD-003
    - ADR-GRD-005
    - ADR-GRD-007
tags:
  - mcp
  - servidor-mcp
  - herramientas-seguras
  - allowlist
  - seguridad
  - operacion-protegida
  - claude-code
  - mvp
---

# Contexto del Feature: Servidor MCP

> **Fundamento**: enfoque híbrido BRD-PRD para arquitectura pre-desarrollo (framework AADD). Combina objetivos de negocio con requisitos de producto para informar las decisiones del Arquitecto.
>
> **Origen**: [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5) § 6.1, capacidades **BR-14, BR-15 y BR-16** (todas Must) y **NFR-02**. Épica **E-001**, feature **F-001-05** del [backlog](../../backlog.md). Reglas de negocio en [business-rules.md](./business-rules.md) (BR-MCP-001).
>
> **Decisiones heredadas**: el Servidor MCP parte de decisiones ya tomadas en el [Motor local](../motor-local/context.md), el [Cockpit](../cockpit/context.md), la [Time Machine](../time-machine/context.md) y [Guardrails](../guardrails/context.md), y de varios ADRs. Una sola decisión de este documento diverge de otra feature y lo declara: el rebase que choca se aborta solo (Q-MCP-6 frente a Q-CKP-11). Ver [Decisiones heredadas](#decisiones-heredadas).
>
> **Convención de IDs**: lo propio lleva el prefijo MCP: preguntas **P-MCP-n**, decisiones **Q-MCP-n**, supuestos **S-MCP-n**, riesgos **R-MCP-n**, dependencias **DEP-MCP-n** y reglas **BR-MCP-<CAT>-NNN**. Lo ajeno se califica siempre: "Q39 de motor-local", "BR-CONS-002 (Guardrails)", "Q-CKP-11", "D-TMC-23".
>
> **Restricción dura**: `raptor-mcp` es un cliente más del daemon (ADR-GRP-005). No lee Git, no abre el perfil y no embebe el motor. Escribe solo con operaciones del catálogo compartido con el Cockpit (DEP-CKP-7), que no inventa: lo que falta se anota como dependencia (DEP-MCP-n), sin aplicarla aquí.

---

## 1. Visión General (BRD)

El Servidor MCP es la cara de GitRaptor para los agentes. Les da herramientas de alto nivel y seguras para consultar el estado del repo, entender la historia, commitear, rebasar, crear un worktree, ver conflictos previstos, tomar un snapshot y deshacer lo propio. Cada herramienta pasa por las mismas reglas que protegen al repo cuando el agente usa Git crudo.

El MCP no es dueño del estado ni de la decisión. Presenta lo que publica el motor (F-001-01), escribe solo como operación protegida de la Time Machine (F-001-03) y obedece la decisión de Guardrails (F-001-04). Su aporte propio es el catálogo de herramientas, el ámbito del llamante, la allowlist de repos, las respuestas acotadas y la instalación en un paso para Claude Code.

### Problema de Negocio (BRD)

Los agentes operan Git con permisos plenos: pueden hacer `reset --hard`, force-push o borrar trabajo, y la *prompt injection* puede empujarlos a hacerlo (BRD § 2, dolor P3). Los servidores MCP de Git existentes exponen comandos crudos y han tenido vulnerabilidades graves (`mcp-server-git`, enero de 2026: path traversal, inyección de argumentos, repos fuera de alcance).

Impacto de no resolverlo:
- Las políticas de Guardrails solo se aplicarían en los hooks, después de que el agente decide la acción, y sin una vía segura que ofrecerle.
- El agente seguiría usando Git crudo para todo, sin snapshot garantizado en sus escrituras.
- El MCP de GitRaptor podría ser un vector de ataque (riesgo crítico del BRD § 10).
- La demo del BRD § 13 ("un agente que intenta force-push y queda bloqueado") y el prototipo (d) del mismo § 13 quedan sin cubrir.

### Valor Esperado (BRD)

- **ROI estimado**: no se cuantifica en dinero (herramienta interna, D1). El retorno es menos trabajo perdido por agentes y una superficie de ataque cerrada.
- **KPIs de éxito**: ver § 3. Los principales: 0 operaciones denegadas ejecutadas por la capa MCP, 0 escrituras sin snapshot y 100 % del corpus de seguridad rechazado (Q-MCP-18).
- **Beneficiarios**: el desarrollador orquestador (directo); los agentes, que reciben una vía segura y explicada; Guardrails, que gana su capa `mcp` (BR-CONS-002 (Guardrails), US-GRD-016).

---

## 2. Dominio Específico (PRD)

- **Tipo de funcionalidad**: servidor MCP local por stdio con un catálogo cerrado de herramientas de lectura y de escritura gobernadas y protegidas, más los comandos `raptor mcp install` y `raptor mcp uninstall`.
- **Usuarios principales**: los agentes de IA (Claude Code con soporte completo, D2) como llamantes; el desarrollador orquestador como quien instala el servidor y habilita repos.
- **Casos de uso principales**:
  - **Conectar Claude Code en un paso**: `raptor mcp install` deja el servidor disponible en todas las sesiones del usuario (BR-15; BR-MCP-WF-007).
  - **Habilitar un repo para los agentes**: el desarrollador añade el repo a la allowlist; observar no basta (BR-16; BR-MCP-WF-006).
  - **Que el agente sepa dónde está**: `status` del repo del llamante, con su solicitante resuelto y el estado de protección (BR-14; BR-MCP-CALC-003).
  - **Commitear sin saltarse nada**: `safe_commit` con mensaje validado, sin `--amend` ni `--no-verify`, con snapshot previo (BR-14; BR-MCP-ELIG-002).
  - **Ponerse al día con la base**: `safe_rebase` atómico: o termina, o el repo queda como antes con la lista de conflictos (BR-14; BR-MCP-WF-002).
  - **Saber con quién va a chocar**: `check_conflicts` con la predicción que publica el motor (BR-14; BR-MCP-CALC-004).
  - **Entender qué pasó**: `explain_history` con eventos y timeline acotados (BR-14; BR-MCP-CALC-005).
  - **Volver atrás lo propio**: `snapshot` manual y `undo` de su última operación (BR-14; BR-MCP-ELIG-005).
  - **Declararse**: `register_agent` y `unregister_agent` en su worktree (BR-02; BR-MCP-WF-005).
  - **Entender cada "no"**: todo rechazo dice motivo y acción (BR-MCP-CONS-005).
- **Alcance**:
  - **IN scope**:
    - Servidor `raptor-mcp` por stdio, solo con la capability `tools` (Q-MCP-13).
    - Diez herramientas: `status`, `explain_history`, `safe_commit`, `safe_rebase`, `create_worktree`, `check_conflicts`, `snapshot`, `undo`, `register_agent` y `unregister_agent` (Q-MCP-1).
    - Ámbito del llamante por el cwd del proceso, con `expect_worktree` opcional en las escrituras (Q-MCP-2, Q-MCP-25).
    - Allowlist de repos opt-in, gestionada con comandos reservados, ⊆ repos observados (Q-MCP-3, Q-MCP-20).
    - Correspondencia herramienta → operación normalizada de Guardrails → operación del catálogo compartido (Q-MCP-1, DEP-MCP-2, DEP-MCP-5).
    - Respuestas acotadas, sin diff, mensajes de commit ni contenido de archivos (Q-MCP-8 a Q-MCP-10, Q-MCP-16).
    - Errores con código estable, motivo y acción, en/es (Q-MCP-17).
    - `raptor mcp install` y `raptor mcp uninstall` para Claude Code (Q-MCP-14, Q-MCP-29).
    - Endurecimiento: entradas validadas, sin shell, catálogo fijo, confused deputy cerrado (Q-MCP-5, Q-MCP-15; DEP-MCP-3).
  - **OUT of scope**:

    | Exclusión | Dueño / motivo |
    |-----------|----------------|
    | App de escritorio, extensión de VS Code/Cursor, UI kit React, Storybook, Motion | Fase 3 (BRD BR-22; AGENTS.md) |
    | Instalación y soporte completo para Cursor, Codex y Copilot | Después del MVP, uno por uno (D2, Q32 de motor-local). En el MVP, "no soportado todavía" |
    | Push, merge, descartar, borrar rama o worktree, reset y cualquier operación contra el remoto | Cockpit (Q-CKP-10, Q-CKP-12) o nunca en el MVP |
    | Redo, restaurar a un punto y timeline completo | CLI/TUI de la Time Machine (Q-MCP-11) |
    | Editar la configuración, decidir en la cola, instalar hooks, excepción consciente | Comandos reservados del desarrollador (BR-AUTH-004 (Guardrails), ADR-GRD-007) |
    | Añadir o quitar repos de la allowlist o de la observación | Comandos reservados del desarrollador (Q-MCP-3, Q40 de motor-local) |
    | Transporte de red (HTTP, SSE) | Nunca: 100 % local (NFR-03) |
    | Ver el diff por MCP | Nunca (Q-CKP-8) |
    | Lanzar o detener agentes | Fuera de alcance del producto (BRD § 6.4) |
    | La regla "misma decisión en las dos capas" y US-GRD-016 | Guardrails (F-001-04); esta feature la desbloquea al definir herramientas y allowlist |
    | Cálculo del estado, detección de sesiones, predicción | Motor local (F-001-01) y Cockpit (DEP-CKP-1) |
    | Catálogo de operaciones y ejecutor | Cockpit (ADR-CKP-002, DEP-CKP-7), compartido |

### Dependencias con otras features

| Feature | Relación con el Servidor MCP | Dirección |
|---------|------------------------------|-----------|
| F-001-01 Motor local | Publica estado, sesiones, eventos y la rama base confirmada; resuelve el solicitante por ascendencia y lee el cwd del llamante. Espera el canal para el registro explícito de agentes (BR-02, Q39 de motor-local). | Bidireccional |
| F-001-02 Cockpit | Dueño del catálogo de operaciones y del ejecutor, compartidos (DEP-CKP-7 → ADR-CKP-002). `check_conflicts` reutiliza la predicción publicada (DEP-CKP-1). | MCP depende del Cockpit |
| F-001-03 Time Machine | Toda escritura del MCP es operación protegida (D-TMC-10). `snapshot` y `undo` son sus capacidades; el solicitante y el canal `mcp` vienen de ADR-TMC-005. | MCP depende de la Time Machine |
| F-001-04 Guardrails | Decide cada herramienta con capa `mcp` (ADR-GRD-003 § 4). Consulta la allowlist para el estado de protección y nunca la escribe (ADR-GRD-005 § 2). US-GRD-016 espera esta feature. | Bidireccional |

---

## 3. Objetivos de Negocio (BRD)

| Objetivo | Métrica de Éxito | Prioridad |
|----------|------------------|-----------|
| Ninguna acción denegada pasa por el MCP | 0 operaciones denegadas ejecutadas por la capa MCP (Q-MCP-18) | Alta |
| Cero pérdida de datos | 0 escrituras del MCP sin snapshot previo (NFR-01, D-TMC-10) | Alta |
| Superficie cerrada | 100 % del corpus de seguridad (traversal, refs maliciosas, UNC, inyección de argumentos, confused deputy) rechazado; 0 respuestas con datos de repos fuera de la allowlist; 0 escrituras fuera del worktree del llamante; 0 comandos reservados aceptados desde descendientes del ejecutor (Q-MCP-18) | Alta |
| Seguridad revisada | Revisión OWASP / MCP Top 10 aprobada antes de cada release (NFR-02) | Alta |
| Demostrar el valor | Demo del BRD § 13 reproducible: una herramienta MCP denegada por política y el force-push por Git crudo parado por el hook con la misma decisión (Q-MCP-18) | Alta |
| Acciones peligrosas bloqueadas | Se mide por capa (`mcp` y `hooks`), como pide el BRD § 9 | Media |
| Red de seguridad usada | Los undos por MCP cuentan en "undos por usuario activo y mes" (BRD § 9) | Media |
| Adopción | % de operaciones del agente hechas vía MCP: métrica sin meta | Baja |

---

## 4. Stakeholders y Actores (BRD + PRD)

### Stakeholders de Negocio (BRD)

| Stakeholder | Interés | Expectativa |
|-------------|---------|-------------|
| Rene Bonilla (producto, revisión e integración) | Único humano del proyecto (D3). Dogfooding con Claude Code. | Instalar en un paso y que los agentes no puedan saltarse las reglas ni tocar repos que no habilitó. |
| Equipos internos piloto (2, BRD § 9) | Usarán el MCP en sus repos. | `[POR VERIFICAR]` quiénes son (heredado de Guardrails). |

### Actores del Sistema (PRD)

| Actor | Descripción | Permisos/Capacidades |
|-------|-------------|----------------------|
| **Desarrollador orquestador** | Instala el servidor y decide qué repos habilita. | Ejecuta `raptor mcp install`/`uninstall`. Añade y quita repos de la allowlist con comandos reservados (Q-MCP-3). No usa el MCP para operar. |
| **Agente Claude Code** | Agente con soporte completo en el MVP (D2). Lanza `raptor-mcp`. | Usa las diez herramientas en el repo de su cwd, si está en la allowlist. Escribe solo en su worktree y deshace solo lo suyo (D-TMC-23). |
| **Otro agente** | Codex, Cursor u otro conectado a mano. | Sin soporte (Q-MCP-13): debe registrarse para escribir (Q-MCP-4). |
| **"Sin atribuir"** | Solicitante que el daemon no asigna a ningún agente. | Solo lecturas y `register_agent` (Q-MCP-4); `undo` siempre rechazado (TQ-7). |
| **Subagente de Claude Code** | Subagente aislado en otro worktree que comparte el servidor del padre. | Su ámbito es el del padre; `expect_worktree` evita que escriba en el worktree equivocado (Q-MCP-25). |
| **Proceso descendiente del ejecutor** | Hook del usuario u otro proceso lanzado durante una operación. | Se atribuye al solicitante de la operación en curso y no puede usar comandos reservados (Q-MCP-5, DEP-MCP-3). |
| **`raptor-mcp`** | Proceso servidor lanzado por el agente. | Cliente del daemon; lo arranca con la primera llamada (Q-MCP-16). Nunca cambia de directorio. |
| **Daemon / motor** | Proceso en segundo plano (ADR-GRP-005). | Resuelve solicitante y ámbito; consulta a Guardrails; ejecuta las operaciones del catálogo. |
| **CLI de Claude Code (`claude`)** | Herramienta del agente que registra servidores MCP. | `raptor mcp install` la invoca con argv fijo (Q-MCP-14). |

---

## 5. Restricciones y Limitaciones (BRD + Arquitectura)

### Regulatorias (BRD + Arquitectura)
- Sin restricciones regulatorias: herramienta interna y 100 % local (NFR-03). `[POR VERIFICAR]` la política interna de ASSA (heredado).
- El diff, los mensajes de commit y el contenido de archivos son confidenciales: nunca salen por el MCP (Q-CKP-8). Las URLs de remotos van sin credenciales (SEC-05).

### Técnicas (Arquitectura)
- **Cliente del daemon**: `raptor-mcp` no lee Git, no abre el perfil y no embebe el motor (ADR-GRP-005). Lo que falta en el daemon es una dependencia DEP-MCP-n.
- **Una sola vía de escritura**: toda escritura es una operación del catálogo compartido ejecutada por el daemon como operación protegida. Sin snapshot no hay operación (D-TMC-10, ADR-TMC-004). Nunca push ni remoto.
- **Sin shell (NFR-02)**: argv fijo; el mensaje de commit no viaja por argv (Q-MCP-5).
- **Ámbito sin parámetro de repo**: el repo y el worktree salen del cwd del proceso, leído por el daemon (SEC-TMC-15).
- **Guardrails decide**: el MCP no autoriza; la decisión se toma con capa `mcp` antes de pedir la operación (ADR-GRD-003 § 4).
- **Texto no confiable**: todo texto del repo va marcado como dato no confiable (SEC-12). Las descripciones de herramientas son fijas.
- **Estándar**: protocolo MCP estándar y agnóstico (NFR-08), solo stdio y solo `tools` (Q-MCP-13).

### De Negocio (BRD)
- Una persona orquestando agentes (D3): historias pequeñas y verificables.
- Soporte completo solo para Claude Code (D2). BR-15 queda parcial y el BRD lo acepta (§ 11; D2 en § 12.1).
- BR-14 a BR-16 son Must. Orden de entrega: Q-MCP-19.
- Sin fecha objetivo para el MVP (BRD § 12.2).

---

## 6. Requisitos No Funcionales Destacados (PRD + Arquitectura)

| RNF | Valor Objetivo | Crítico | Justificación |
|-----|----------------|---------|---------------|
| **Seguridad (NFR-02)** | Sin shell, entradas validadas, solo repos de la allowlist; revisión OWASP / MCP Top 10 antes de cada release. Requisitos SEC-MCP-n → DEP-MCP-8 | Sí | Riesgo crítico del BRD § 10. |
| **Respuesta acotada** | Tamaño máximo de respuesta y por campo, paginación con tope (Q-MCP-16). Cifras: ⚠️ **ASSUMPTION** (S-MCP-1) | Sí | Contexto del agente y fuga de datos. |
| **Tiempo por llamada** | Tiempo máximo por llamada; una escritura que tarda por el snapshot declara su estado (Q-MCP-16). Cifra: S-MCP-1 | No | El agente no debe quedar colgado. |
| **Robustez** | Rate limit por conexión (SEC-08) y cuota propia de snapshots manuales (SEC-TMC-12) | Sí | Un agente en bucle no agota disco ni daemon. |
| **Cero pérdida** | 100 % de las escrituras por operación protegida (NFR-01) | Sí | Riesgo crítico del BRD § 10. |
| **Privacidad (NFR-03)** | Nada sale de la máquina; sin diff, mensajes ni contenido | Sí | Datos confidenciales. |
| **i18n (NFR-10)** | Mensajes de error en/es según el locale del usuario; nombres y descripciones de herramientas en inglés | No | Convención del producto. |
| **Interoperabilidad (NFR-08)** | MCP estándar; cualquier cliente puede conectarse, solo Claude Code se prueba | No | D2. |

---

## 7. Integraciones Externas (PRD + Arquitectura)

| Sistema/API | Propósito | Tipo de Integración | Criticidad |
|-------------|-----------|---------------------|------------|
| Daemon / motor (F-001-01) | Estado, eventos, ámbito, solicitante, ejecución | Canal local registrado como `mcp` (ADR-GRP-005, ADR-TMC-005 § 1). Contrato: DEP-MCP-1, DEP-MCP-6 | Alta |
| Time Machine (F-001-03) | Operación protegida, snapshot, undo, timeline | Interna, a través del daemon | Alta |
| Guardrails (F-001-04) | Decisión con capa `mcp`, estado de protección | Interna, a través del daemon (DEP-MCP-5) | Alta |
| Cockpit (F-001-02) | Catálogo de operaciones, ejecutor, predicción publicada | Interna (DEP-CKP-7, DEP-CKP-1) | Alta |
| Claude Code | Cliente MCP que lanza `raptor-mcp` por stdio | Protocolo MCP estándar (NFR-08) | Alta |
| CLI de Claude Code (`claude`) | Registrar y retirar el servidor | Proceso con argv fijo, por ruta absoluta (Q-MCP-14) | Media |
| Git del sistema (≥ 2.38) | Ejecutar commit, rebase, worktree add | Solo a través del ejecutor del daemon | Alta |

---

## 8. Características Únicas del Feature (PRD)

- **Herramientas, no comandos**: diez herramientas de alto nivel; ninguna ejecuta Git arbitrario.
- **El agente no elige el repo**: el ámbito sale de dónde arrancó, no de un parámetro.
- **Opt-in explícito**: un repo observado no queda expuesto hasta que el desarrollador lo habilita.
- **Rebase atómico**: o termina, o el repo queda como estaba.
- **Misma decisión que Git crudo**: Guardrails responde igual por MCP y por hooks.
- **Explica cada "no"**: código estable, motivo y acción.

---

## 9. Glosario del Dominio (PRD)

| Término | Definición | Notas |
|---------|------------|-------|
| **Herramienta** | Función MCP del catálogo fijo de `raptor-mcp`, con nombre y descripción fijos en el binario. | Q-MCP-1, Q-MCP-15. |
| **Allowlist** | Lista de repos habilitados para el MCP, por usuario, en el perfil; ⊆ repos observados. | Q-MCP-3, Q-MCP-20. |
| **Comando reservado** | Acción que solo puede hacer el desarrollador, con una confirmación que un agente no puede dar. Nunca está en el MCP. | ADR-GRP-005 § 6, SEC-03. |
| **Ámbito del llamante** | El repo del worktree que contiene el cwd del proceso `raptor-mcp`. Las lecturas operan sobre todo el repo. | Q-MCP-2. |
| **Worktree del llamante** | El worktree que contiene ese cwd. Las escrituras solo operan aquí. | Q-MCP-2. |
| **`expect_worktree`** | Parámetro opcional de las escrituras que solo estrecha el ámbito: si no coincide con el worktree del llamante, rechazo. | Q-MCP-25. |
| **Capa MCP** | Aplicación de las reglas de Guardrails a través de las herramientas. Activa cuando el repo está en la allowlist. | ADR-GRD-005 § 2, Q-MCP-26. |
| **Operación normalizada** | Forma común con la que Guardrails decide una operación, venga del MCP o de un hook. | BR-VAL-002 (Guardrails). |
| **Catálogo compartido** | Lista cerrada de operaciones de usuario del Cockpit y del MCP, con su ejecutor. | DEP-CKP-7, ADR-TMC-002 § 5. |
| **Ejecutor** | Parte del daemon que ejecuta las operaciones del catálogo como operación protegida. | ADR-CKP-002 (en curso). |
| **Operación protegida** | Intención → snapshot previo obligatorio → ejecución → registro. | D-TMC-10, ADR-TMC-004. |
| **Solicitante** | Actor al que el daemon atribuye la llamada por ascendencia del proceso: "agente X" o "sin atribuir". | ADR-TMC-005 § 1, D-TMC-23. |
| **Sin atribuir** | Solicitante no asignado a ningún agente. Por MCP solo lee y se registra. | Q-MCP-4, TQ-7. |
| **Confused deputy** | Proceso con poder ajeno que actúa por otro: un hook del usuario, lanzado por el ejecutor, que pide un comando reservado. | Q-MCP-5, DEP-MCP-3. |
| **Respuesta acotada** | Respuesta con campos de una allowlist, topes de tamaño y paginación. | Q-MCP-16, SEC-12. |
| **Texto no confiable** | Texto que sale del repo o de un agente (rutas, ramas, etiquetas). Se marca como dato, nunca como instrucción. | SEC-12. |
| **Rebase atómico** | Rebase que, si choca, se aborta solo y deja el repo como antes, con la lista de conflictos. | Q-MCP-6. |
| **Tool poisoning** | Instrucciones maliciosas escondidas en descripciones de herramientas o en texto que el agente lee. | MCP Top 10; Q-MCP-15. |
| **Rug pull** | Cambio en caliente de la lista o las descripciones de herramientas tras aprobarlas. | Q-MCP-15. |
| **Rechazo de dominio** | Resultado de herramienta con `isError`, código estable, motivo y acción. | Q-MCP-17. |
| **Sesión presente** | Sesión Activa o Inactiva en un worktree. | Q-CKP-10. |
| **Rama base confirmada** | La rama base que el humano confirmó; puede estar "no confirmada" o "pendiente". | Q-GRD-21, Q-GRD-23. |

---

## 10. Estándares Aplicables (Arquitectura)

- **Protocolo**: Model Context Protocol estándar, transporte stdio, capability `tools` (NFR-08). Implementación con `rmcp` (ADR-GRP-001).
- **Seguridad**: OWASP y MCP Top 10 antes de cada release (NFR-02); requisitos SEC-02, SEC-05, SEC-08, SEC-11, SEC-12, SEC-14, SEC-TMC-12 y SEC-TMC-15; checklist SEC-MCP-n → DEP-MCP-8.
- **Contenido**: [design system](../../../design-system/README.md) (DSYS-GRP-001), guía de contenido para los mensajes en/es.
- **Autenticación**: sin usuarios; el solicitante lo resuelve el daemon (transversal, lo define el Arquitecto).

---

## 11. Referencias (BRD + PRD + Arquitectura)

- [BRD-GRP-001](../../../business/gitraptor-documento-de-negocio.md) (v0.5): § 2 (P3), § 6.1 (BR-11, BR-14 a BR-16), § 7 (NFR-01 a NFR-03, NFR-08, NFR-10), § 9 (KPIs), § 10 (riesgos), § 11 (alcance de D2 sobre BR-15), § 12.1 (D2), § 13 (prototipo y demo).
- [Contexto del Motor local](../motor-local/context.md) (CTX-GRP-001).
- [Contexto del Cockpit](../cockpit/context.md) (CTX-CKP-001) y [sus reglas](../cockpit/business-rules.md) (BR-CKP-001).
- [Contexto de la Time Machine](../time-machine/context.md) (CTX-TMC-001).
- [Contexto de Guardrails](../guardrails/context.md) (CTX-GRD-001), [sus reglas](../guardrails/business-rules.md) (BR-GRD-001) y [US-GRD-016](../guardrails/user-stories/US-GRD-016-misma-decision-por-mcp.md).
- [ADRs](../../../architecture/decisions/): ADR-GRP-001, 005, 006, 009, 013; ADR-TMC-002, 004, 005; ADR-GRD-003, 005, 007.
- [Requisitos no funcionales](../../../architecture/non-functional.md): SEC-02, SEC-03, SEC-05, SEC-08, SEC-10, SEC-11, SEC-12, SEC-14; M8.
- [Reglas de negocio de esta feature](./business-rules.md) (BR-MCP-001).
- [Backlog](../../backlog.md): E-001 / F-001-05.

---

## Cobertura de BR-14…BR-16 y NFR-02

Cada capacidad del BRD queda cubierta por decisiones, reglas y, donde falta un artefacto del Arquitecto, por dependencias.

| Capacidad (BRD) | Decisiones | Reglas | Dependencias | Cubierta |
|-----------------|------------|--------|--------------|----------|
| **BR-14** Herramientas de alto nivel y seguras, sujetas a BR-11 | Q-MCP-1, 2, 4 a 12, 21 a 28 | CALC-001 a 005, ELIG-001 a 006, WF-001 a 005, AUTH-001 a 003, CONS-001, CONS-002, TIME-002, TIME-003, EDGE-001 a 003, EDGE-005 a 008, EDGE-010 | DEP-MCP-1, 2, 5, 6, 7 | Sí; `check_conflicts` y `explain_history` condicionadas a DEP-MCP-6 |
| **BR-15** Instalación en un paso | Q-MCP-14, 29 | WF-007, EDGE-009 | DEP-MCP-1 | Sí, parcial por D2: solo Claude Code |
| **BR-16** Endurecimiento: allowlist, path traversal, refs, argv fijo, límites de salida | Q-MCP-2, 3, 5, 15, 16, 17, 20, 25 | VAL-001 a 006, CALC-002, WF-006, AUTH-004, AUTH-005, CONS-003 a 005, TIME-001, EDGE-004 | DEP-MCP-1, 3, 4, 8 | Sí |
| **NFR-02** Sin shell, entradas validadas, solo allowlist, revisión por release | Q-MCP-3, 5, 13, 15, 18 | VAL-001 a 006, WF-006, AUTH-004, AUTH-005, CONS-003, CONS-006 | DEP-MCP-3, 8, 9 | Sí; Linux/Windows en la etapa de validación multiplataforma |

Los IDs de reglas omiten el prefijo `BR-MCP-`.

---

## Dependencias y enmiendas propuestas (no aplicadas)

Estas dependencias se anotan; **no se aplican** en este documento. Las resuelve el Arquitecto.

| DEP | Hueco | Artefacto a crear o enmendar |
|---|---|---|
| DEP-MCP-1 | Contrato del servidor MCP | ADR-MCP-001 (nuevo): ámbito por cwd y `expect_worktree`; herramienta → operación normalizada → operación del catálogo; allowlist de campos de respuesta (cierra M8 y SEC-12 del overview § 10.3); errores, límites, cancelación, transporte e instalación. |
| DEP-MCP-2 | Operaciones del catálogo que el MCP necesita | ADR-CKP-002 (en curso, DEP-CKP-7): commit, snapshot manual y crear worktree para el MCP; rebase atómico con el abort registrado; flags fijados (sin update-refs, autosquash, autostash ni exec); hooks sin stdin ni terminal y con tiempo máximo. |
| DEP-MCP-3 | Allowlist reservada y confused deputy | Enmienda a ADR-GRP-005 § 6 y SEC-03: añadir y quitar de la allowlist son comandos reservados; los descendientes del ejecutor se atribuyen al solicitante de la operación en curso y no pueden usar comandos reservados. **Estado (2026-10-04)**: la parte de confused deputy está implementada en TS-TMC-004 (marcas del ejecutor por proceso y grupo, hasta el cierre de la operación; Dev Spec DS-TS-TMC-004 § 7). Falta la allowlist reservada (con DEP-MCP-4) y la enmienda formal a ADR-GRP-005 § 6. |
| DEP-MCP-4 | Almacén de la allowlist | Enmienda a ADR-GRP-006: allowlist en el perfil, invariante ⊆ observados y retirada en cascada. |
| DEP-MCP-5 | Decisión con capa `mcp` | Enmienda a ADR-GRD-003 § 4 y § 5: transiciones de refs del abort no reevaluadas; tabla de correspondencias herramienta → operación normalizada. |
| DEP-MCP-6 | Datos para las lecturas | Predicción publicada (DEP-CKP-1); API de timeline y eventos (ADR-GRP-013, DEP-CKP-5) en `crates/api`. |
| DEP-MCP-7 | Respuesta "pendiente" de confirmación | DEP-CKP-8 / US-GRD-015. Sin artefacto nuevo. |
| DEP-MCP-8 | Requisitos de seguridad del MCP | SEC-MCP-n y checklist MCP Top 10 en `non-functional.md` (security-expert). |
| DEP-MCP-9 | Leer el cwd de otro proceso en Windows y Linux | Pendiente: etapa de validación multiplataforma. |

---

## Decisiones heredadas

| Decisión | Qué fija para el Servidor MCP |
|----------|-------------------------------|
| ADR-GRP-005 § 5 | `raptor-mcp` es cliente del daemon; el contrato de salida marca el texto no confiable y acota las respuestas. Lo arranca bajo demanda con entorno limpio (SEC-10). |
| ADR-GRP-005 § 6 | Los comandos reservados no se exponen por el MCP; el registro explícito valida nombres y prohíbe los reservados (§ 6.6). |
| ADR-TMC-005 § 1 | Solicitante por ascendencia; canal registrado `mcp`; repo y worktree del cwd del llamante, en la allowlist (SEC-TMC-15). |
| TQ-7 (Time Machine) | Un undo "sin atribuir" por MCP siempre se rechaza. Q-MCP-4 es más estricto y no lo contradice. |
| D-TMC-23 (Time Machine) | Un agente solo deshace lo suyo; la confirmación interactiva no la puede dar un agente (por eso nunca se usa la elicitation, Q-MCP-12). |
| D-TMC-10 (Time Machine) | Toda escritura lleva snapshot previo garantizado; sin snapshot no hay operación. |
| D-TMC-13 (Time Machine) | El undo se detiene ante solape con otro actor y lo informa. |
| ADR-GRD-003 § 4 | Decisión con capa `mcp`, antes de pedir la operación, registrada una sola vez. |
| ADR-GRD-005 § 2 | Capa MCP activa si el repo está en la allowlist; Guardrails la consulta y nunca la escribe. |
| ADR-GRD-007 § 1 | El canal rechaza los comandos reservados que llegan desde la conexión del MCP. |
| Q-GRD-15 (Guardrails) | La allowlist es un subconjunto de los repos observados. |
| BR-AUTH-004 (Guardrails) | Ninguna herramienta edita la configuración ni decide en la cola. |
| BR-CONS-002 (Guardrails) | La misma operación recibe la misma decisión por MCP y por hooks. |
| Q32 de motor-local | Soporte completo solo para Claude Code; el resto, "otro agente" registrado. |
| Q39 de motor-local | Registrarse donde ya se le detectaba confirma la misma sesión. |
| Q40 de motor-local | Un agente no puede añadir ni retirar repos de la observación. |
| Q-CKP-8 | El diff nunca se expone por el MCP. |
| Q-CKP-11 | En el Cockpit, el rebase que choca queda detenido. El MCP diverge de forma consciente (Q-MCP-6). |
| Q-CKP-13 | Reglas para crear un worktree, reutilizadas por `create_worktree`. |
| SEC-05 | Sin secretos en respuestas; URLs de remotos sin userinfo. |
| SEC-12 | Texto no confiable marcado en el contrato. |
| SEC-14 | Regla de "binario instalado": nunca desde una caché de npx o una carpeta temporal. |

---

## Supuestos

| # | Supuesto | Estado |
|---|----------|--------|
| S-MCP-1 | Topes numéricos (tamaño de respuesta y por campo, número de rutas, longitud del mensaje y de la etiqueta, tiempo por llamada, rate limit, cuota de snapshots manuales). | ⚠️ Las cifras se fijan en la Dev Spec (DEP-MCP-1) |
| S-MCP-2 | `explain_history` devuelve por defecto los últimos 50 eventos. | ⚠️ La cifra la afina la Dev Spec |
| S-MCP-3 | Claude Code lanza `raptor-mcp` con el cwd en el directorio del proyecto donde corre la sesión. | ⚠️ Verificado solo en macOS; se prueba en el spike del canal |
| S-MCP-4 | El comportamiento en Linux y Windows (lectura del cwd de otro proceso, ascendencia, CLI `claude`) es el mismo que en macOS. | Pendiente: etapa de validación multiplataforma (DEP-MCP-9) |
| S-MCP-5 | La CLI `claude mcp add --scope user` y `claude mcp get/remove` mantienen su forma actual. | ⚠️ Riesgo R-MCP-6 |

## Riesgos

| # | Riesgo | Prob. | Impacto | Mitigación (de negocio) |
|---|--------|-------|---------|-------------------------|
| R-MCP-1 | **Confused deputy**: un hook del usuario, lanzado por el ejecutor durante una operación del MCP, pide un comando reservado y se le atribuye al humano. | Media | Crítico | Descendientes del ejecutor atribuidos al solicitante y sin comandos reservados (Q-MCP-5, DEP-MCP-3). Requisito previo a toda escritura (Q-MCP-19). |
| R-MCP-2 | **Subagente en otro worktree**: comparte el servidor del padre y escribe en el worktree equivocado. | Media | Alto | `expect_worktree` opcional; cada respuesta nombra el worktree en el que actuó (Q-MCP-2, Q-MCP-25). |
| R-MCP-3 | **Prompt injection y tool poisoning**: texto del repo o una descripción alterada dirigen al agente. | Media | Alto | Descripciones fijas en el binario, lista sin cambios en caliente, texto del repo marcado como no confiable (Q-MCP-15, SEC-12). |
| R-MCP-4 | **Los agentes ignoran el MCP** y siguen con Git crudo. | Alta | Medio | Los hooks aplican la misma decisión (BR-CONS-002 (Guardrails)); adopción medida sin meta (Q-MCP-18). |
| R-MCP-5 | **El abort de un rebase falla** y el worktree queda a medias. | Baja | Alto | Queda detenido, se informa, el snapshot previo existe y las siguientes escrituras se rechazan por precondición (Q-MCP-6). |
| R-MCP-6 | **Cambia `~/.claude.json` o la CLI `claude`** y la instalación se rompe. | Media | Medio | Se usa la CLI, no el archivo; sin `claude`, se imprime el comando exacto (Q-MCP-14). |
| R-MCP-7 | **Allowlist mal entendida**: el desarrollador cree que observar ya protege por MCP. | Media | Medio | Estado de protección y diagnóstico "MCP no instalado" (Q-MCP-26); rechazo con acción en repos fuera (Q-MCP-3). |

---

## Decisiones tomadas

**Decisión del orquestador (2026-10-04), validada por Arquitecto/PO.** Rene Bonilla delegó en el orquestador, el 2026-10-04, la autonomía para decidir las preguntas de esta feature y validarlas con los agentes PO y Arquitecto. Cada Q-MCP-n sale de P-MCP-n con el mismo número. Solo Q-MCP-6 diverge de otra feature (Q-CKP-11) y lo declara; ninguna sale del MVP.

| # | Pregunta de origen | Decisión | Fecha | Decidido por | Validación | Reglas afectadas |
|---|--------------------|----------|-------|--------------|------------|------------------|
| Q-MCP-1 | P-MCP-1 | Catálogo: las 8 herramientas de BR-14 + `register_agent` y `unregister_agent` (solo el propio). Nada de push, merge, borrar rama o worktree, reset, restore, redo ni remoto. Cada herramienta → operación normalizada (BR-VAL-002 (Guardrails)) y, si escribe, → operación del catálogo (DEP-CKP-7). Lo que falta → DEP-MCP-2. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CONS-001, CONS-004, ELIG-001 |
| Q-MCP-2 | P-MCP-2 | Ámbito: el worktree que contiene el cwd del proceso, también desde una subcarpeta; el daemon lo lee en cada llamada; fuera de un worktree observado → rechazo. `raptor-mcp` nunca cambia de directorio. Sin parámetro de repo. Un `cd` del agente no cambia la sesión. Lecturas: todo el repo; escrituras: solo el worktree. Cada respuesta nombra el worktree. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del PO: subcarpeta y `cd` explícitos. Ajuste del Arquitecto: lectura por el daemon en cada llamada | CALC-001, VAL-005, EDGE-004 |
| Q-MCP-3 | P-MCP-3 | Allowlist por repo en el perfil, ⊆ observados. Añadir y quitar son comandos reservados (DEP-MCP-3). Opt-in: observar no habilita. Repo fuera: todas las herramientas, también lecturas, responden "repo no habilitado para el MCP" + acción, sin datos. Guardrails la consulta, nunca la escribe. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-006, CONS-003, AUTH-004, EDGE-004 |
| Q-MCP-4 | P-MCP-4 | "Sin atribuir": solo lecturas y `register_agent`; toda escritura rechazada con motivo y acción (registrarse). Más estricto que TQ-7. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | AUTH-002 |
| Q-MCP-5 | P-MCP-5 | `safe_commit`: rutas explícitas (pathspec literal) o "todo lo preparado"; en compartido solo rutas. Mensaje obligatorio con tope, validado por las convenciones configuradas (BR-11), sin pasar por argv. Nunca `--amend`, `--no-verify`, `--allow-empty`. Respeta hooks y config del usuario (ADR-CKP-002). Devuelve oid y rutas. Confused deputy cerrado antes de cualquier escritura (DEP-MCP-3). | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del PO: rutas explícitas en compartido. Hallazgo crítico del Arquitecto: confused deputy | VAL-001, VAL-003, ELIG-002, AUTH-005 |
| Q-MCP-6 | P-MCP-6 | `safe_rebase`: rama del worktree del llamante sobre la base confirmada; exige limpio, base confirmada, sin operación en curso y sin otra sesión presente. Si choca: abort automático, repo como antes, rutas en conflicto, evento "rebase abortado por conflicto". Diverge de Q-CKP-11 de forma consciente. Abort parte de la operación (variante atómica), transiciones registradas (DEP-MCP-5), flags fijados (DEP-MCP-2). Abort fallido: detenido, informado, escrituras rechazadas. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto, con condiciones del Arquitecto (abort en la operación, flags fijados, abort fallido). Ajuste del PO: bloqueo con otra sesión presente | ELIG-003, WF-002, WF-003 |
| Q-MCP-7 | P-MCP-7 | `create_worktree`: reglas de Q-CKP-13; ruta opcional sin traversal. No registra ni lanza agente; la respuesta aclara que la sesión sigue en el worktree original. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | VAL-001, VAL-002, ELIG-004 |
| Q-MCP-8 | P-MCP-8 | `check_conflicts`: lo publicado por el motor (pares, nivel, rutas y rangos de líneas sin contenido, antigüedad, "calculando"/"pendiente", límites). Por defecto los pares del worktree del llamante; opción de todo el repo. Nunca diff. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CALC-004 |
| Q-MCP-9 | P-MCP-9 | `explain_history`: eventos y timeline del repo (operación, actor, rama/worktree, oids, rutas con tope, cuándo, cobertura); sin mensajes ni contenido; últimos 50 por defecto; filtro y paginación con tope. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CALC-005 |
| Q-MCP-10 | P-MCP-10 | `status`: worktrees con sesiones y actor, rama, rutas modificadas con tope, ahead/behind con antigüedad, base confirmada, estado de protección y diagnósticos, estado del motor y huecos, solicitante resuelto. URLs sin userinfo. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del PO: solicitante y diagnósticos | CALC-003 |
| Q-MCP-11 | P-MCP-11 | `snapshot` manual con etiqueta corta no confiable, cuota y rate limit propios. `undo` de la última operación propia del worktree (pila por worktree); solape → se detiene. Redo, restaurar y timeline completo: solo CLI/TUI. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del Arquitecto: cuota y rate limit propios | VAL-004, ELIG-005, TIME-001 |
| Q-MCP-12 | P-MCP-12 | "Pedir confirmación": sin cola = denegar con "requiere confirmación del desarrollador" + acción (Cockpit). Con cola: "pendiente" con id, sin bloquear. Nunca elicitation como confirmación. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | WF-004, AUTH-004 |
| Q-MCP-13 | P-MCP-13 | Solo stdio y solo `tools` (sin sampling, prompts, resources, listChanged). Estándar y agnóstico; soporte probado solo Claude Code. Otros agentes: "otro agente", deben registrarse. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | CONS-004 |
| Q-MCP-14 | P-MCP-14 | `raptor mcp install`: solo Claude Code, ámbito de usuario, con la CLI `claude` por ruta absoluta y argv fijo (nunca editar `~/.claude.json`); idempotente; muestra qué cambia; no toca otros servidores; reversible con `uninstall`; binario instalado por ruta absoluta; sin `claude`, imprime el comando. Otros agentes: "no soportado todavía". Install no es reservado; añadir el repo a la allowlist sí. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Mecanismo del Arquitecto | WF-007, EDGE-009 |
| Q-MCP-15 | P-MCP-15 | Nombres y descripciones fijos en inglés, sin texto del repo; lista sin cambios en caliente; anotaciones orientativas, no control; cada descripción declara que el texto del repo es dato no confiable. Errores en/es. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto | VAL-006, CONS-004 |
| Q-MCP-16 | P-MCP-16 | Topes de respuesta, por campo, paginación, tiempo por llamada y rate limit por conexión (cifras: S-MCP-1). Cancelación o desconexión no interrumpe una escritura. Daemon arrancado con la primera llamada a herramienta. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del Arquitecto: cancelación y arranque | CALC-002, TIME-001 a 003 |
| Q-MCP-17 | P-MCP-17 | Rechazos de dominio con `isError`, código estable, plantilla fija y params no confiables; motivo y acción. JSON-RPC solo para llamadas mal formadas. Nunca trazas ni rutas fuera del repo. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del Arquitecto: `isError` frente a JSON-RPC | CONS-005 |
| Q-MCP-18 | P-MCP-18 | KPIs del § 3; ligados al BRD § 9 (bloqueos por capa, undos por MCP); demo § 13 en dos partes; adopción sin meta. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. Ajuste del PO: KPIs de allowlist y de demo sin herramienta de push | CONS-006 |
| Q-MCP-19 | P-MCP-19 | Orden: 1) canal + allowlist + `status` + install + register/unregister; 2) confused deputy; 3) snapshot/undo + `safe_commit`; 4) `check_conflicts` y `explain_history`; 5) `safe_rebase` y `create_worktree`. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO y Arquitecto. El Arquitecto aceptaba instalación manual para probar; se adopta install en la 1 por dogfooding | Matriz de priorización |
| Q-MCP-20 | P-MCP-20 | Repo retirado de la observación → sale de la allowlist, con aviso; su estado de protección cambia. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO; el Arquitecto coincide | WF-006, CONS-003 |
| Q-MCP-21 | P-MCP-21 | Worktree compartido: `safe_commit` solo con rutas; `safe_rebase` bloqueado con otra sesión presente; `undo` con solape se detiene. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | EDGE-003 |
| Q-MCP-22 | P-MCP-22 | Base no confirmada o pendiente: lecturas con el estado declarado; escrituras que la necesitan, rechazadas con acción; aplica el conjunto mínimo. Daemon caído: se arranca; si no, error con acción. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | EDGE-001, EDGE-002 |
| Q-MCP-23 | P-MCP-23 | `register_agent`: solo en el worktree del cwd; nombres validados; prohibidos los reservados y el de otro agente con sesión presente; origen "registrado" visible; si ya se le detectaba, misma sesión. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO; el Arquitecto coincide con ADR-GRP-005 § 6.6 | VAL-004, WF-005 |
| Q-MCP-24 | P-MCP-24 | Git crudo desde la misma sesión no es escritura del MCP: lo cubren hooks y observación; los KPIs miden solo las herramientas. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | PO | CONS-006 |
| Q-MCP-25 | P-MCP-25 | `expect_worktree` opcional en escrituras; solo estrecha; distinto → rechazo. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Arquitecto; el PO coincide | VAL-005, EDGE-006 |
| Q-MCP-26 | P-MCP-26 | Capa MCP activa = repo en la allowlist; los 4 estados de BR-WF-002 (Guardrails) no cambian; "MCP no instalado" es un diagnóstico. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Pregunta del Arquitecto; validada por el PO | CONS-003 |
| Q-MCP-27 | P-MCP-27 | `safe_commit` añade sin seguimiento solo si se nombran; "todo lo preparado" no los añade; ignorados nunca. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Pregunta del Arquitecto | EDGE-010 |
| Q-MCP-28 | P-MCP-28 | Rebase de una rama ya empujada: lo decide Guardrails; la respuesta avisa de upstream divergente; el force-push posterior lo deniega el mínimo seguro. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Pregunta del Arquitecto | EDGE-007 |
| Q-MCP-29 | P-MCP-29 | Nombre del servidor en Claude Code: "gitraptor". Si existe uno con ese nombre que no es nuestro, no se sobrescribe: error con acción. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Pregunta del Arquitecto | EDGE-009 |
| Q-MCP-30 | P-MCP-30 | HEAD separado: `safe_commit` y `safe_rebase` se rechazan con la acción "crea una rama o cámbiate a una"; `snapshot` y `undo` siguen permitidos. Son precondiciones del ejecutor del catálogo compartido y se revalidan antes de ejecutar; "operación en curso" se comprueba primero. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Arquitecto, con ajuste: la precondición va en el ejecutor (DEP-MCP-2), con su orden; el motivo es evitar un estado confuso, no una pérdida, porque el reflog y la Time Machine conservan el trabajo | EDGE-008 |
| Q-MCP-31 | P-MCP-31 | Repo de otro propietario o no disponible (SEC-11, ADR-GRP-009) → todas las herramientas responden "no disponible", sin datos, aunque esté en la allowlist. | 2026-10-04 | Orquestador (decisión del orquestador, 2026-10-04) | Arquitecto, con ajuste: se evalúa en cada llamada; incluye los criterios de worktree de SEC-11; la acción solo en texto, nunca escribe en la config global; precedencia "no habilitado" → "no disponible" | EDGE-005 |

## Preguntas abiertas

Todas las preguntas de esta feature están resueltas. La tabla se conserva como historia. Quedan **pendientes técnicos del Arquitecto**, que no son preguntas de negocio: ADR-MCP-001, las operaciones de ADR-CKP-002 y las enmiendas DEP-MCP-n. Queda también la **etapa de validación multiplataforma** (DEP-MCP-9, S-MCP-4).

| # | Pregunta | Recomendación original del orquestador | Estado |
|---|----------|----------------------------------------|--------|
| P-MCP-1 | ¿Catálogo de herramientas del MVP? | Las 8 de BR-14 + register/unregister; nada de push ni remoto; lo que falte, DEP-MCP-n. | Resuelta (Q-MCP-1) |
| P-MCP-2 | ¿Sobre qué worktree opera cada herramienta? | El del cwd del proceso; sin parámetro de repo; lecturas al repo, escrituras al worktree. | Resuelta (Q-MCP-2), con ajustes (subcarpeta, `cd`, lectura en cada llamada) |
| P-MCP-3 | ¿Qué es la allowlist y quién la cambia? | Por repo en el perfil, ⊆ observados, comandos reservados, opt-in; fuera → rechazo sin datos. | Resuelta (Q-MCP-3) |
| P-MCP-4 | ¿Qué puede hacer "sin atribuir"? | Solo lecturas y `register_agent`. | Resuelta (Q-MCP-4) |
| P-MCP-5 | ¿Cómo es `safe_commit`? | Rutas o "todo lo preparado"; mensaje validado; nunca amend ni no-verify; devuelve oid. | Resuelta (Q-MCP-5), con ajustes (compartido, stdin, confused deputy) |
| P-MCP-6 | ¿Rebase que choca: abortar o dejar detenido? | Abortar automáticamente; alternativa: detenido. | Resuelta (Q-MCP-6): abortar, con condiciones |
| P-MCP-7 | ¿`create_worktree`? | Reglas de Q-CKP-13; ruta sin traversal; no registra. | Resuelta (Q-MCP-7) |
| P-MCP-8 | ¿`check_conflicts`? | Lo publicado por el motor, sin contenido; pares del llamante por defecto. | Resuelta (Q-MCP-8) |
| P-MCP-9 | ¿`explain_history`? | Eventos y timeline sin mensajes; ventana acotada; paginación. | Resuelta (Q-MCP-9) |
| P-MCP-10 | ¿`status`? | Estado publicado del repo, sin contenido. | Resuelta (Q-MCP-10), con solicitante y diagnósticos |
| P-MCP-11 | ¿`snapshot` y `undo`? | Snapshot con etiqueta; undo de lo propio; redo y restaurar solo CLI/TUI. | Resuelta (Q-MCP-11), con cuota propia |
| P-MCP-12 | ¿"Pedir confirmación" por MCP? | Sin cola = denegar con acción; con cola, "pendiente". | Resuelta (Q-MCP-12), sin elicitation |
| P-MCP-13 | ¿Transporte y agentes? | Solo stdio; estándar; probado solo Claude Code. | Resuelta (Q-MCP-13), solo `tools` |
| P-MCP-14 | ¿`raptor mcp install`? | Solo Claude Code; ámbito de usuario; idempotente; reversible; no reservado. | Resuelta (Q-MCP-14), mecanismo por CLI `claude` |
| P-MCP-15 | ¿Descripciones de herramientas? | Fijas en el binario, en inglés; errores en/es. | Resuelta (Q-MCP-15), sin cambios en caliente |
| P-MCP-16 | ¿Límites y robustez? | Topes, paginación, tiempo, rate limit; cifras en la Dev Spec. | Resuelta (Q-MCP-16), con cancelación y arranque |
| P-MCP-17 | ¿Errores? | Códigos estables, plantilla fija, motivo y acción. | Resuelta (Q-MCP-17), con `isError` |
| P-MCP-18 | ¿KPIs? | 0 denegadas ejecutadas, 0 sin snapshot, corpus 100 %, revisión, demo. | Resuelta (Q-MCP-18), con KPIs de ámbito y demo en dos partes |
| P-MCP-19 | ¿Orden de entrega? | Lectura, escritura básica, registro, lecturas dependientes, rebase y worktree, install. | Resuelta (Q-MCP-19): install y registro en la 1; confused deputy como paso 2 |
| P-MCP-20 | ¿Repo retirado de la observación? | Sale de la allowlist con aviso. | Resuelta (Q-MCP-20) |
| P-MCP-21 | ¿Worktree compartido? | Rutas explícitas; rebase bloqueado con otra sesión; undo se detiene. | Resuelta (Q-MCP-21) |
| P-MCP-22 | ¿Base no confirmada o daemon caído? | Lecturas con estado; escrituras rechazadas con acción; daemon arrancado. | Resuelta (Q-MCP-22) |
| P-MCP-23 | ¿Límites de `register_agent`? | Solo en el cwd; nombres validados; sin suplantar. | Resuelta (Q-MCP-23) |
| P-MCP-24 | ¿Git crudo cuenta como escritura del MCP? | No; lo cubren hooks y observación. | Resuelta (Q-MCP-24) |
| P-MCP-25 | ¿Cómo evitar que un subagente escriba en otro worktree? | `expect_worktree` opcional que solo estrecha. | Resuelta (Q-MCP-25) |
| P-MCP-26 | ¿Cuándo está activa la capa MCP? | Repo en la allowlist; "MCP no instalado" como diagnóstico. | Resuelta (Q-MCP-26) |
| P-MCP-27 | ¿Archivos sin seguimiento en `safe_commit`? | Solo si se nombran; ignorados nunca. | Resuelta (Q-MCP-27) |
| P-MCP-28 | ¿Rebase de una rama ya empujada? | Lo decide Guardrails; aviso de divergencia. | Resuelta (Q-MCP-28) |
| P-MCP-29 | ¿Nombre del servidor en Claude Code? | "gitraptor"; nunca sobrescribir uno ajeno. | Resuelta (Q-MCP-29) |
| P-MCP-30 | ¿HEAD separado en el worktree del llamante? | Rechazar `safe_commit` y `safe_rebase` con acción, en vez de dejarlo a Guardrails. | Resuelta (Q-MCP-30), precondición en el ejecutor |
| P-MCP-31 | ¿Repo de otro propietario en la allowlist? | "No disponible" sin datos para todas las herramientas. | Resuelta (Q-MCP-31), evaluado en cada llamada |

> P-MCP-1 a P-MCP-19 llevan la recomendación del brief del orquestador; donde la validación la ajustó, la columna Estado lo indica. P-MCP-20 a P-MCP-24 salen de la validación del PO y P-MCP-25 a P-MCP-29 de la del Arquitecto: su recomendación coincide con la decisión. P-MCP-30 y P-MCP-31 salen de la revisión del orquestador sobre las reglas (HEAD separado y repo de otro propietario) y las validó el Arquitecto.

---

## ✅ Quality Review (Auto-evaluación del Contexto)

> Revisión ejecutada el 2026-10-04 al terminar el documento (`methodology.md` § 7). Resultado: **10 ✅ · 4 ⚠️ · 0 🔴**. Las ⚠️ dependen de artefactos del Arquitecto (ADR-MCP-001, ADR-CKP-002, DEP-MCP-n), de cifras sin fijar y de la etapa multiplataforma; ninguna impide que el Arquitecto empiece.

| Sección | Resultado | Nota |
|---------|-----------|------|
| Problema de Negocio | ✅ | Específico (dolor P3, CVEs de `mcp-server-git`, demo § 13). |
| Valor Esperado / ROI | ⚠️ | ROI cualitativo (D1); adopción sin meta. |
| KPIs de Éxito | ✅ | Cuantificados (0 / 100 %) y ligados al BRD § 9. |
| Usuarios/Actores | ✅ | Incluye subagente, descendiente del ejecutor y "sin atribuir". |
| Alcance OUT of scope | ✅ | 12 exclusiones con dueño o fase. |
| Restricciones | ✅ | Cliente del daemon, una vía de escritura, sin shell y ámbito sin parámetro. |
| RNFs | ⚠️ | Cifras de topes y tiempos en supuesto (S-MCP-1); Linux/Windows pendiente (S-MCP-4). |
| Integraciones | ⚠️ | Claras, pero 9 dependencias sin resolver; ADR-CKP-002 en curso. |
| Glosario | ✅ | 22 términos, incluidos ámbito del llamante, `expect_worktree`, confused deputy y rebase atómico. |
| Stakeholders | ✅ | Equipos piloto `[POR VERIFICAR]`, heredado. |
| Cobertura BR-14…16 y NFR-02 | ✅ | Cada capacidad con decisiones, reglas y dependencias; BR-15 parcial declarado. |
| Seguridad (BR-16, NFR-02) | ⚠️ | Decisiones firmes; checklist SEC-MCP-n pendiente (DEP-MCP-8). |
| Herramientas (BR-14) | ✅ | Matriz de precondiciones completa (BR-MCP-ELIG-001). |
| Decisiones y trazabilidad | ✅ | 31 decisiones con validación; sin preguntas de negocio abiertas. |

## ⚠️ Known Risks (from Quality Review)

| # | Sección | Riesgo | Impacto | Aceptado por |
|---|---------|--------|---------|--------------|
| 1 | RNFs | Topes numéricos y tiempos sin cifra (S-MCP-1, S-MCP-2). | Las historias llevarán cifras provisionales hasta la Dev Spec. | Orquestador (Q-MCP-16) |
| 2 | RNFs | Linux y Windows sin verificar (S-MCP-4, DEP-MCP-9). | El ámbito por cwd puede no funcionar igual fuera de macOS. | Orquestador; etapa de validación multiplataforma |
| 3 | Integraciones | DEP-MCP-1 a 9 sin resolver; ADR-CKP-002 en curso. | `safe_commit`, `snapshot`, `safe_rebase` y `create_worktree` no se cierran sin el catálogo. | Orquestador (Q-MCP-19 ordena la entrega) |
| 4 | Seguridad | Confused deputy abierto hasta DEP-MCP-3. | Ninguna herramienta de escritura puede entregarse antes. | Orquestador (Q-MCP-19, paso 2) |
