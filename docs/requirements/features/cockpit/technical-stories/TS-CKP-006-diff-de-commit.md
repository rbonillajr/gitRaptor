---
id: TS-CKP-006
title: "Consultas del diff de un commit, entre dos commits y del índice de un worktree en el canal"
type: ts
status: implemented
feature: cockpit
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-10
blocked_by: [US-CKP-012]
related:
  adrs: [ADR-GRP-009, ADR-GRP-005, ADR-GRP-016, ADR-GRP-017, ADR-MCP-001, ADR-CKP-003]
  stories: [US-DSK-009, US-DSK-017, US-DSK-005, US-DSK-010, US-DSK-011, US-DSK-014, TS-CKP-008, US-CKP-012, INF-GRP-001, INF-GRP-002]
  specs: [DS-TS-CKP-006]
ado:
  id: null
  url: null
tags: [cockpit, desktop, dep-dsk-3, consulta-bajo-demanda, diff, solo-lectura, nfr-05, sec-09, ui-2]
---

## TS-CKP-006: Consultas del diff de un commit, entre dos commits y del índice de un worktree en el canal

**Valor**: la app de escritorio enseña qué cambió en un commit del grafo o de la búsqueda con la misma consulta acotada, sin filtros y de solo lectura que ya sirve el diff de un worktree, en lugar de leer Git por su cuenta.

> Enabler de DEP-DSK-3 ([contexto de desktop](../../desktop/context.md) § 8). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: es trabajo del canal del motor (consulta nueva del módulo `diff` que crea US-CKP-012), por eso lleva el slug CKP y no DSK; tiene varias historias dueñas (US-DSK-009 y US-DSK-017; `diff.index`, US-DSK-010, 011 y 014), así que pasa el Enabler Decision Gate como TS.

### Descripción

**Como** Arquitecto
**Quiero** tres consultas bajo demanda en el canal del daemon: el diff de un commit frente a su primer padre (`diff.commit`), el diff entre dos commits, opcionalmente limitado a una ruta (`diff.between`), y el diff de un worktree partido en "preparado" y "sin preparar" (`diff.index`)
**Para** que el diff de UI-2 salga del motor por la frontera de solo lectura de ADR-GRP-009 (cálculo sin filtros, sin `git` hijo) con los mismos topes, cancelación y reparto a una sola conexión que `diff.worktree` (DS-US-CKP-012), y que la app siga sin leer Git (Q-DSK-1, ADR-GRP-017 § 1)

### Alcance Técnico

- **Declarar** en el módulo `diff` del contrato del canal los métodos `diff.commit`, `diff.between` y `diff.index`, en el archivo propio del módulo (ADR-GRP-016, [extender sin archivos compartidos](../../../../architecture/extender-sin-archivos-compartidos.md)).
  - Parámetros: el repo observado (`repo_id`), oids completos de commit y, en `diff.between`, una ruta relativa literal opcional; en `diff.index`, un worktree observado. Sin campo de más (`deny_unknown_fields`).
  - **Oid completo**: 40 o 64 hex en minúscula, convertido con `ObjectId::from_hex`. Se rechaza todo valor que empiece por `-` y se prohíbe pasarlo por `rev_parse` o cualquier revspec (`HEAD~`, `@{-1}`, `:/regex`), de modo que una abreviatura o un nombre de ref nunca se resuelve.
  - Se descubren en `hello.methods` de la TUI y de la app; son `mcp: false` y no reservadas: el método no se ofrece ni se atiende a una conexión `raptor-mcp` (allowlist de BR-14, NFR-02), y la vista restringida de la app no lo ve.
- **Calcular** el diff en el daemon con la lectura sin filtros de ADR-GRP-009 § 1, en el pool de consultas bajo demanda que crea US-CKP-012.
  - Misma forma de sección que `diff.worktree` (`DiffSection`, `FileDiff`, `FileBody`, `Hunk`), para que la app no tenga un segundo ingest ni una segunda vista, más `parents` (número de padres) y la marca de merge.
  - Commit raíz: contra el árbol vacío. Commit de merge: contra el primer padre.
  - Sin renombrados en el MVP (D16 de DS-US-CKP-012): un `git mv` sale como un borrado y un añadido, cada uno con su ruta, en los tres métodos.
  - Lectura con `RepoReader::open` (`crates/git/src/reader.rs`, que aplica `neutralize_programs`) y sin Git CLI. `neutralize_programs` solo borra secciones con subsección y no borra `[diff] external`: el guard de abajo lo prueba y, si falla, esta ficha exige cerrarlo.
  - Sin filtros `clean`, `textconv`, diff externo ni `gpg`; ningún proceso hijo.
- **Calcular** en `diff.index` lo preparado (HEAD contra el índice) y lo sin preparar (índice contra el working tree, con los archivos sin seguimiento), con el contenido de cada lado identificado para que TS-CKP-008 detecte un archivo que cambió (US-DSK-010, US-DSK-011); los archivos en conflicto salen marcados como tales (US-DSK-014).
- **Validar** que cada oid nombra un commit **alcanzable desde una ref** del repo observado; un blob, un commit huérfano o un oid de otro repo se rechaza sin leer su contenido.
  - El código del rechazo es el mismo que el de "no existe": no distingue "no existe" de "no alcanzable".
  - La búsqueda de alcanzabilidad se acota con el plazo de 2 s y el token de cancelación; si se corta, responde el mismo código que "no existe".
- **Acotar** la respuesta con los mismos topes, plazo, cancelación y cola que `diff.worktree` (D4 de DS-US-CKP-012): una consulta en vuelo por conexión, pool de 1 hilo con cola global de 4 (más, `query-busy`), por archivo, en total y de tiempo.
  - Mejora del motor sin bloqueo (I-01 de la revisión de seguridad 2026-11-10): repartir la cola global por solicitante para que un cliente no agote las 4 plazas de los demás.
  - Al superarlos, "truncado" con el motivo; los binarios y los archivos sobre el tope, sin contenido y con el motivo (US-DSK-009, escenario 3).
  - Cada archivo lleva sus fragmentos con su identificador estable, el mismo formato que usa TS-CKP-008 para preparar por fragmento.
- **Reutilizar** la cancelación de consultas (`query.cancel`) y el rechazo por ocupado de US-CKP-012; una consulta nueva de la misma conexión no espera a la anterior.
- **Responder** solo a la conexión que pidió el diff, nunca al stream de difusión, sin persistirlo ni registrarlo (ADR-GRP-009, Validación 8, condiciones 1 a 3).
- **Bloquea** (DEP-DSK-3): el escenario 2 de US-DSK-009 (T007 de su Dev Spec, el diff de un commit) y las partes de US-DSK-012 y US-DSK-017 que muestran el diff de un commit. Las demás tareas de esas historias no esperan a esta ficha.
- **Decisión de P29 (2026-11-10)**: el coordinador aceptó que `diff.commit` vaya en una sola historia de motor con la tarea T007 de US-DSK-009 bloqueada hasta que esta ficha llegue a `main`. `blocked_by: [US-CKP-012]` se mantiene: la rama `feat/US-CKP-012-agent-diff` implementa el módulo `diff`, el pool y `query.cancel`, y aún no está en `main`.
- **Aplicar** la regla de perfil: perfil completo de lectura para la app con identidad (ADR-GRP-017 § 3) y para la TUI; para una conexión de perfil `mcp` el método no se ofrece ni se atiende (`mcp: false`).
- **Fuera de alcance**: la vista del diff en la app (US-DSK-009); el diff del worktree y de lo que entraría al merge (US-CKP-012); preparar por fragmento (TS-CKP-008); la lista de commits y ramas (TS-CKP-007); el cálculo de la base común en `diff.between` (compara a→b de árbol a árbol, sin merge-base; M-02 de la revisión de seguridad).

### Plan de Verificación

#### Pruebas Automatizadas

- **Contrato**: `diff.commit`, `diff.between` y `diff.index` aparecen en `hello.methods` de una conexión TUI o app y no en `raptor-mcp`, y una conexión `mcp` que los invoca recibe el rechazo de método no ofrecido; un campo de más se rechaza.
- **Oid**: se rechazan un oid abreviado, con mayúsculas, de longitud distinta de 40 o 64, que empiece por `-`, y los revspec `HEAD~`, `@{-1}` y `:/regex`, sin llamar a `rev_parse`.
- **Forma**: en un repo temporal con un commit raíz (contra el árbol vacío), un commit normal, un merge de dos padres (contra el primer padre, con `parents` = 2 y la marca de merge) y un `git mv`, cada diff trae los archivos y las líneas esperadas en la misma forma de sección que `diff.worktree`; el `git mv` sale como un borrado y un añadido (D16); `diff.between` con ruta trae solo ese archivo.
- **Índice**: con un archivo preparado, otro a medio preparar y uno sin seguimiento, `diff.index` separa lo preparado de lo sin preparar; un archivo en conflicto sale marcado.
- **Alcanzabilidad**: un blob suelto, un commit huérfano sin ref y un oid de otro repo se rechazan sin leer contenido y con el mismo código que un oid inexistente; si el plazo corta la búsqueda de alcanzabilidad, responde ese mismo código.
- **Topes**: un archivo de 40 MB y un `.png` salen sin contenido y con motivo; un commit con más archivos que el tope total sale "truncado"; una consulta que supera el tiempo se corta con su motivo, nunca con un resultado parcial presentado como completo.
- **Cancelación y cola**: `query.cancel` sobre un diff en vuelo responde y el trabajo se detiene entre archivos; una segunda consulta de la misma conexión no espera a la anterior; con la cola global llena, la consulta nueva sale `query-busy`.
- **Frontera (ADR-GRP-009, Validación 7)**: guard obligatorio en un repo con `diff.external` (en `[diff]` sin subsección), `diff.x.textconv`, `filter.x.clean`, `gpg.program` y un `.gitattributes` que apuntan a un script que crea un marcador: tras pedir el diff de un commit que toca esos archivos (con `diff.commit`, `diff.between` y `diff.index`) el marcador no existe y la auditoría de `exec` no ve procesos hijo. El mismo caso, con `diff.external` sin subsección, se añade al canario de `diff.worktree` (`crates/core/tests/diff_git.rs`).
- **Secretos (Validación 8)**: con secretos plantados y el diff pedido varias veces, perfil y logs con 0 hallazgos; el stream de difusión no lleva el contenido y otra conexión abierta al mismo daemon no recibe la respuesta.
- **Rendimiento**: en el repo de 100K commits de INF-GRP-002, el diff de un commit típico dentro del presupuesto de consulta bajo demanda que fije el plan, medido en el banco.

#### Verificación Manual / Sandbox

- Con la app en un repo de demo, abrir el diff de un commit desde el grafo y desde un resultado de búsqueda y comparar con `git show --stat`.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

> Dev Spec: [`dev-specs/TS-CKP-006-diff-de-commit.md`](../dev-specs/TS-CKP-006-diff-de-commit.md) — `DS-TS-CKP-006` (status: spec-ready; revisión de seguridad 2026-11-10 incorporada; decisiones D1 a D15 pendientes de revisión del coordinador).

Implementado en: PR #PRN. La corrida de `/implement` se abortó en P0 por el modo rápido y la historia queda **sin certificar**; el banco de rendimiento (CKP006-012) se mide a mano en release.
