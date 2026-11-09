---
id: TS-CKP-006
title: "Consultas del diff de un commit, entre dos commits y del índice de un worktree en el canal"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
blocked_by: [US-CKP-012]
related:
  adrs: [ADR-GRP-009, ADR-GRP-005, ADR-GRP-016, ADR-GRP-017, ADR-MCP-001, ADR-CKP-003]
  stories: [US-DSK-009, US-DSK-017, US-DSK-005, US-DSK-010, US-DSK-011, US-DSK-014, TS-CKP-008, US-CKP-012, INF-GRP-001, INF-GRP-002]
  specs: []
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
  - Parámetros: el repo observado, oids completos de commit y, en `diff.between`, una ruta relativa literal opcional; en `diff.index`, un worktree observado.
  - Se descubren en `hello.methods`; ninguno se ofrece a `raptor-mcp` (allowlist de BR-14, NFR-02).
- **Calcular** el diff en el daemon con la lectura sin filtros de ADR-GRP-009 § 1, en el pool de consultas bajo demanda que crea US-CKP-012.
  - Commit raíz: contra el árbol vacío. Commit de merge: contra el primer padre, con la marca "merge" y el número de padres en la respuesta.
  - Sin filtros `clean`, `textconv`, diff externo ni `gpg`; ningún proceso hijo.
- **Calcular** en `diff.index` lo preparado (HEAD contra el índice) y lo sin preparar (índice contra el working tree, con los archivos sin seguimiento), con el contenido de cada lado identificado para que TS-CKP-008 detecte un archivo que cambió (US-DSK-010, US-DSK-011); los archivos en conflicto salen marcados como tales (US-DSK-014).
- **Validar** que cada oid nombra un commit **alcanzable desde una ref** del repo observado; un objeto suelto o de otro repo se rechaza sin leer su contenido.
- **Acotar** la respuesta con los topes de `diff.worktree`: por archivo, en total y de tiempo.
  - Al superarlos, "truncado" con el motivo; los binarios y los archivos sobre el tope, sin contenido y con el motivo (US-DSK-009, escenario 3).
  - Cada archivo lleva sus fragmentos con su identificador estable, el mismo formato que usa TS-CKP-008 para preparar por fragmento.
- **Reutilizar** la cancelación de consultas (`query.cancel`) y el rechazo por ocupado de US-CKP-012; una consulta nueva de la misma conexión no espera a la anterior.
- **Responder** solo a la conexión que pidió el diff, nunca al stream de difusión, sin persistirlo ni registrarlo (ADR-GRP-009, Validación 8, condiciones 1 a 3).
- **Aplicar** la regla de perfil de `diff.worktree`: perfil completo de lectura para la app con identidad (ADR-GRP-017 § 3) y para la TUI; la misma respuesta que `diff.worktree` para una conexión de perfil `mcp`.
- **Fuera de alcance**: la vista del diff en la app (US-DSK-009); el diff del worktree y de lo que entraría al merge (US-CKP-012); preparar por fragmento (TS-CKP-008); la lista de commits y ramas (TS-CKP-007); el diff entre ramas que aún no tienen commit común (se rechaza con motivo).

### Plan de Verificación

#### Pruebas Automatizadas

- **Contrato**: `diff.commit`, `diff.between` y `diff.index` aparecen en `hello.methods` de una conexión TUI o app y no en `raptor-mcp`; un campo de más o un oid abreviado se rechazan.
- **Forma**: en un repo temporal con un commit raíz, un commit normal, un merge de dos padres y un renombrado, cada diff trae los archivos y las líneas esperadas; `diff.between` con ruta trae solo ese archivo.
- **Índice**: con un archivo preparado, otro a medio preparar y uno sin seguimiento, `diff.index` separa lo preparado de lo sin preparar; un archivo en conflicto sale marcado.
- **Alcanzabilidad**: un blob suelto, un commit huérfano sin ref y un oid de otro repo se rechazan sin leer contenido.
- **Topes**: un archivo de 40 MB y un `.png` salen sin contenido y con motivo; un commit con más archivos que el tope total sale "truncado"; una consulta que supera el tiempo se corta con su motivo, nunca con un resultado parcial presentado como completo.
- **Cancelación**: `query.cancel` sobre un diff en vuelo responde y el trabajo se detiene entre archivos.
- **Frontera (ADR-GRP-009, Validación 7)**: el repo canario con `filter.*.clean`, `diff.*.textconv` y `gpg.program` apuntando a un marcador: el marcador no aparece tras pedir el diff de un commit que toca esos archivos; la auditoría de `exec` no ve procesos hijo.
- **Secretos (Validación 8)**: con secretos plantados y el diff pedido varias veces, perfil y logs con 0 hallazgos; el stream de difusión no lleva el contenido.
- **Rendimiento**: en el repo de 100K commits de INF-GRP-002, el diff de un commit típico dentro del presupuesto de consulta bajo demanda que fije el plan, medido en el banco.

#### Verificación Manual / Sandbox

- Con la app en un repo de demo, abrir el diff de un commit desde el grafo y desde un resultado de búsqueda y comparar con `git show --stat`.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
