---
id: TS-CKP-007
title: "Consulta de todas las ramas del repo y de los commits de cualquier rama, paginada y filtrable"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
blocked_by: [US-CKP-012, US-CKP-022]
related:
  adrs: [ADR-GRP-009, ADR-GRP-005, ADR-GRP-011, ADR-GRP-016, ADR-GRP-017, ADR-MCP-001, ADR-CKP-003]
  stories: [US-DSK-012, US-DSK-017, US-DSK-005, US-DSK-015, US-CKP-022, US-CKP-012, US-GRP-012, US-GRD-019, INF-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, desktop, dep-dsk-3, consulta-bajo-demanda, ramas, historial, busqueda, paginacion, nfr-05, ui-2]
---

## TS-CKP-007: Consulta de todas las ramas del repo y de los commits de cualquier rama, paginada y filtrable

**Valor**: la app de escritorio ve todas las ramas del repo, no solo las de los agentes, recorre los commits de cualquiera de ellas por páginas y busca commits por texto o autor en un repo de 100K commits, siempre con lo que publica el motor.

> Enabler de DEP-DSK-3 ([contexto de desktop](../../desktop/context.md) § 8). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: es trabajo del canal del motor, por eso lleva el slug CKP; sus historias dueñas son US-DSK-012 y US-DSK-017 (y US-DSK-005 cuando el grafo de UI-2 muestra todas las ramas). La búsqueda de commits es un filtro de esta misma consulta y la de ramas un filtro de la lista; la búsqueda de **rutas** tiene una sola dueña y va en los Requisitos Técnicos de US-DSK-017.

### Descripción

**Como** Arquitecto
**Quiero** dos consultas bajo demanda en el canal: la lista de ramas del repo (`refs.list`) y una página de commits de una rama o de un rango (`log.page`), con cursor y con filtro opcional por texto del mensaje y por autor o agente
**Para** que UI-2 tenga ramas, historial y búsqueda por la frontera de solo lectura de ADR-GRP-009, con un tope por página que mantiene el repo de 100K commits dentro de NFR-05 y del presupuesto de ADR-GRP-011, sin que la app lea Git (Q-DSK-1)

### Alcance Técnico

- **Declarar** los métodos `refs.list` y `log.page` en sus módulos propios del contrato (ADR-GRP-016); ninguno se ofrece a `raptor-mcp` (BR-14, NFR-02).
- **Exponer** en `refs.list` todas las ramas locales y de seguimiento remoto del repo observado.
  - Por rama: nombre completo, oid de la punta, rama de seguimiento si la tiene, ahead/behind frente a ella (lo que ya calcula US-GRP-012), el worktree que la tiene sacada si hay uno y el agente de ese worktree si lo hay.
  - Filtro opcional por subcadena del nombre, aplicado en el daemon; tope de ramas por respuesta con "hay más".
- **Exponer** en `log.page` los commits alcanzables desde una rama o un oid, opcionalmente excluyendo lo alcanzable desde otro (rango `base..rama`), en orden topológico y por fecha.
  - Por commit: oid, padres, asunto, autor, fecha y la atribución de agente que ya publica el motor (D6, US-GRD-019); nunca el cuerpo completo del mensaje sin tope.
  - Página con tope de commits y **cursor opaco** ligado a la conexión y al oid de partida; un cursor de otra conexión, caducado o de una punta que se movió se rechaza con "vuelve a empezar".
- **Filtrar** en `log.page` por texto del asunto y por autor o agente, sin distinguir mayúsculas y tratado como literal (nunca patrón).
  - El recorrido se corta por tope de commits examinados y de tiempo; la respuesta dice cuántos se examinaron y si quedan, para que la app diga "en qué se buscó" (US-DSK-017, escenario 3).
- **Calcular** las dos consultas con la lectura sin filtros de ADR-GRP-009 § 1, sin proceso hijo, en el pool de consultas de US-CKP-012, con `query.cancel`.
- **Reutilizar** el lector de carriles de US-CKP-022 para el rango `base..rama`; `graph.lanes` sigue igual para la TUI.
- **Aplicar** la regla de perfil de `graph.lanes` (DS-US-CKP-022, G3): perfil completo para la app con identidad y la TUI; una conexión de perfil `mcp` recibe el mismo trato que en `graph.lanes`.
- **Fuera de alcance**: la búsqueda de rutas de archivo (Requisitos Técnicos de US-DSK-017); el diff de un commit (TS-CKP-006); el dibujo del grafo y la ventana de 50 commits de la TUI (US-CKP-022); crear, cambiar o borrar ramas (US-DSK-012); el ahead/behind contra el remoto tras un fetch (US-DSK-015); un índice persistente de búsqueda, que solo se crearía si el banco demuestra que el recorrido acotado no cumple NFR-05.

### Plan de Verificación

#### Pruebas Automatizadas

- **Contrato**: `refs.list` y `log.page` en `hello.methods` de la TUI y de la app, no en `raptor-mcp`; un campo de más o un cursor manipulado se rechazan.
- **Ramas**: en un repo temporal con ramas de agentes, una rama suelta, una rama de seguimiento remoto y una rama sacada en otro worktree, `refs.list` las trae todas con su worktree y su agente; el filtro "pagos" trae `feat-pagos` y `feat-pagos-v2` (US-DSK-017, escenario 2).
- **Páginas**: recorrer una rama de 1.000 commits por páginas da cada commit una vez y en orden; mover la punta entre páginas invalida el cursor con su motivo.
- **Filtro**: "rounding" encuentra `fix: rounding in invoices` con su autor y su agente (US-DSK-017, escenario 1); un texto con caracteres de patrón se busca literal; sin resultados, la respuesta dice cuántos commits se examinaron.
- **Frontera y secretos**: el repo canario de ADR-GRP-009 (Validación 7) sin marcador y sin procesos hijo; perfil y logs sin asuntos ni autores guardados.
- **Rendimiento**: en el repo de 100K commits de INF-GRP-002, primera página de `log.page` y `refs.list` con 500 ramas dentro de NFR-05; una búsqueda sin resultados termina por tope de tiempo y lo dice.

#### Verificación Manual / Sandbox

- Con la app en un repo de demo con varias ramas sin agente, recorrer el historial de una de ellas y buscar por mensaje y por autor; comparar con `git log --oneline`.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
