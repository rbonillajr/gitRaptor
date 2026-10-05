---
id: TS-CKP-002
title: "Catálogo de operaciones de usuario y ejecutor del daemon en dos fases"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: critical
complexity: high
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-002, ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007, ADR-GRP-005, ADR-GRP-009]
  stories: [TS-TMC-004, TS-TMC-003, TS-GRP-002, TS-GRP-003, TS-GRP-004, INF-GRP-001, INF-TMC-001, TS-CKP-003]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, catalogo-operaciones, ejecutor, operacion-protegida, capa, serializacion, nfr-01, nfr-02, br-07, dep-ckp-7, dep-mcp-2, dep-mcp-3, dep-mcp-5, f-001-05]
---

## TS-CKP-002: Catálogo de operaciones de usuario y ejecutor del daemon

**Valor**: la TUI y el MCP solo escriben en el repo de una manera: una operación del catálogo, preparada con su plan, revalidada bajo el cerrojo del repo y ejecutada como operación protegida, con la capa que fija el daemon y no el cliente.

### Descripción

**Como** Arquitecto
**Quiero** el catálogo cerrado y versionado de operaciones de usuario y el ejecutor del daemon que las prepara y las ejecuta en dos fases
**Para** que las acciones de BR-07 y las herramientas de escritura del Servidor MCP compartan validación, revalidación y snapshot previo, sin otra vía de escritura y sin que un agente rodee sus límites cambiando de cliente (ADR-CKP-002, ADR-TMC-004, NFR-01)

> Dev Spec: `dev-specs/TS-CKP-002-catalogo-ejecutor.md` | Pendiente
>
> **Depende de**: TS-TMC-004 (operación protegida, solicitante y reto ligado al plan), TS-GRP-004 (consultas para describir, preparar, ejecutar y cancelar; `planId`; eventos de operación; marca de la conexión de un descendiente del ejecutor: **pendiente, dueño: worker del canal (TS-GRP-004)**), TS-GRP-002 (lecturas para precondiciones) y TS-GRP-003. Comparte con TS-TMC-003 el cerrojo de escritura por repo: lo crea la primera de las dos que entre. Las operaciones gobernadas necesitan TS-CKP-003 antes de su historia. **ADRs**: ADR-CKP-002 § 1 a § 6, § 11 y § 12, que la Dev Spec sigue punto por punto; ADR-TMC-005 § 1 a § 3; ADR-GRP-009 § 3 y § 4. **Seguridad**: hallazgos H-01, H-02, M-01 a M-05, L-01, L-02, L-04, L-07 e I-02 de la revisión de seguridad de ADR-CKP-002; SEC-02, SEC-05, SEC-10, SEC-12, NFR-02. **Habilita**: BR-07 (merge, rebase, descartar, crear worktree y abortar; BR-CKP-ELIG-001 a 005, WF-003, WF-008, CONS-002, CONS-004, AUTH-002, AUTH-003) y las herramientas de escritura de F-001-05 (`safe_commit`, `safe_rebase`, `create_worktree`, `snapshot`; DEP-MCP-2, DEP-MCP-3, DEP-MCP-5).

### Alcance Técnico

- **Definir** en el contrato el catálogo versionado de ocho operaciones con parámetros tipados, clase, gobernanza y marcas por superficie (Cockpit, MCP), y la consulta que lo describe (ADR-CKP-002 § 1).
- **Resolver** en el daemon el solicitante y la capa, al preparar y otra vez al ejecutar; la capa decide qué operaciones se aceptan, el modo del rebase, el tiempo máximo, Cancelar y quién recibe la salida de Git (ADR-CKP-002 § 3 y § 4).
- **Validar** los parámetros en el daemon: rutas canonicalizadas, ruta nueva de un worktree sin enlaces ni padres escribibles por otros, ramas nuevas con las reglas de L-02 y oids tomados siempre del plan.
- **Implementar** la fase de preparar, sin efectos: precondiciones comunes en su orden, plan con valores esperados e identidad del repo, avisos, trabajo afectado, huella y vista previa de la decisión (ADR-CKP-002 § 2).
- **Emitir** un `planId` aleatorio ligado a la conexión y al solicitante, que caduca y que solo acepta la misma conexión.
- **Implementar** la fase de ejecutar: cerrojo, intención, solicitante re-resuelto, plan rehecho y comparado con la huella, decisión que cuenta, snapshot previo garantizado, ejecución y registro.
- **Revalidar** bajo el cerrojo la identidad del repo y del worktree antes de lanzar, y pasar siempre el repo y el working tree explícitos a Git (ADR-CKP-002 § 5 y § 6).
- **Comprobar** las precondiciones de Git que la TUI no ve: locks de Git, que nunca se borran; worktree bloqueado; rama sacada en otro worktree; repo con grafts.
- **Aplicar** la regla base de permisos con la confirmación ligada al plan de ADR-TMC-005 sobre el trabajo afectado de cada operación.
- **Crear** el cerrojo de escritura por repo en un módulo neutro del motor, compartido con el aplicador de la Time Machine, con una cola que ven todos los clientes.
- **Crear** el registro de hijos del ejecutor con su barrera de arranque: ningún `git` corre antes de quedar registrado con su identidad no reutilizable y las transiciones de su plan (I-02).
- **Resolver** todo proceso que desciende de un hijo registrado como el solicitante de ese plan, sin los privilegios de la capa `cockpit`, y rechazar sus peticiones de operación sin esperar al cerrojo (H-01).
- **Crear** en la capa de Git el módulo de invocación de operaciones de usuario: lista cerrada y tipada, padre directo, sin shell ni terminal de control y sin variantes que salten hooks o reescriban de más.
- **Construir** el entorno de cada `git` desde la allowlist más las variables de sesión validadas, con los ejecutables del usuario y el editor neutralizados, sin red, sin objetos de reemplazo y solo con los descriptores estándar (ADR-CKP-002 § 6).
- **Pasar** a Git los valores esperados del plan donde Git lo admite, para que un proceso externo no cuele un cambio entre la comprobación y la ejecución.
- **Implementar** Cancelar para la capa `cockpit` y el tiempo máximo con interrupción para la capa `mcp`.
- **Publicar** el inicio, la cola y el fin de cada operación en el stream, y devolver el resultado tipado; la salida de Git, saneada y truncada, solo a la conexión de capa `cockpit` que la pidió.
- **Registrar** en la comprobación estática de CI que el ejecutor no importa capas de escritura de la Time Machine ni de Guardrails, y que solo él usa el módulo de invocación.
- **Fuera de alcance**: la lógica propia de cada operación, que va en su historia dueña: merge con o sin la base sacada, descartar con lo no recuperable, crear worktree con su plantilla y abortar lo propio (historias de BR-07); `commit` con el mensaje por stdin, `snapshot` manual sobre la API de captura de la Time Machine y el modo atómico de `safe_rebase` con su abort (historias de F-001-05); el editor (historia de Q-CKP-9); la decisión de Guardrails y su registro (TS-CKP-003); la presentación; push y fetch, que no están en el catálogo.

### Plan de Verificación

#### Pruebas Automatizadas

- **Una vía**: test de contrato: ninguna operación escribe sin su snapshot previo garantizado; con el almacén lleno, `aborted` y el repo igual (huella de INF-GRP-001).
- **Frontera**: la comprobación estática falla si el ejecutor importa una capa de escritura o si otro módulo usa la invocación. La suite "ejecutor" de INF-GRP-001 comprueba con la auditoría de `exec` que todo `git` del ejecutor tiene al daemon como padre directo y lleva el repo y el working tree explícitos.
- **Capa (M-03)**: un agente que abre la TUI recibe capa `mcp`: merge, descartar y Cancelar se rechazan y no recibe la salida de Git. Un cliente JSON-RPC directo de un agente que pide un merge se rechaza.
- **Plan (M-04)**: un `planId` de otra conexión se rechaza; un plan ejecutado tras cambiar el solicitante resuelto da `rejected`.
- **Huella y valor esperado**: dos clientes descartan el mismo worktree y el segundo recibe "ya no existe"; un commit entre preparar y ejecutar un rebase da `rejected`; una base movida por un proceso externo hace fallar entera la actualización de la ref.
- **Repo revalidado (M-05)**: sustituir el `.git` de un worktree enlazado entre preparar y ejecutar da `rejected` sin efectos.
- **Barrera y descendientes (I-02, H-01)**: un hook que se conecta al arrancar encuentra al hijo registrado; con el registro fallido, el hijo muere sin ejecutar hooks. Un hook bajo el plan de un agente que pide un comando reservado, una confirmación o una operación se resuelve como ese agente y se rechaza sin esperar al cerrojo.
- **Entorno**: con un hook que lee la terminal y editores apuntando a un canario, nada se cuelga y el canario no se ejecuta. Variables de sesión inválidas se omiten con diagnóstico y un PATH con un `git` falso no cambia el `git` lanzado. Un `refs/replace` no cambia los objetos de la operación; un hook solo ve los descriptores 0, 1 y 2.
- **Ruta nueva y ramas (H-02, L-02)**: se rechazan un padre con un enlace simbólico, una ruta dentro de `.git` o de un worktree observado, una ruta que aparece entre preparar y ejecutar, y las ramas `HEAD`, `@`, hex de 40 caracteres y con caracteres de anchura cero.
- **Tiempo**: un hook de capa `mcp` que no termina se interrumpe con el motivo `time-limit`; uno igual de capa `cockpit` espera a Cancelar.
- **Nunca push ni descargas (M-02)**: captura de red durante todo el catálogo: 0 conexiones; en un *partial clone* con un objeto ausente, fallo sin red.
- **Interrupción (regresión de INF-TMC-001)**: un daemon que muere a mitad deja la operación `interrumpida` y Deshacer vuelve al previo.

#### Verificación Manual / Sandbox

- En un repo temporal con hooks reales (formateador con `node`, firma), lanzar un merge y un rebase desde un cliente de prueba de capa `cockpit` y revisar la salida de Git y el resultado.
- Windows (capa `cockpit`, barrera, consola) y Linux (muerte del hijo con el daemon, variables de sesión): **Pendiente: etapa de validación multiplataforma**.
