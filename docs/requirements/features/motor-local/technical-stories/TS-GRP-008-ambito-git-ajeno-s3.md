---
id: TS-GRP-008
title: "S3 cuenta los `git` ajenos en el ámbito del evento, no en todo el repo"
type: ts
status: implemented
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-09
updated: 2026-10-09
related:
  adrs: [ADR-GRP-012, ADR-GRP-013, ADR-TMC-005]
  stories: [US-GRP-007, US-GRP-008, US-GRP-009, US-GRD-019, SPIKE-GRP-001]
  rules: [BR-EDGE-004, BR-CONS-001, BR-AUTH-005, BR-26]
ado:
  id: null
  url: null
tags: [motor-local, deteccion, atribucion, s3, claude-code, worktrees, dogfooding, carrera-s3, sec-04]
---

## TS-GRP-008: S3 cuenta los `git` ajenos en el ámbito del evento, no en todo el repo

**Valor**: en un repo con muchos worktrees vivos a la vez (un agente por worktree), los commits de Claude Code salen atribuidos a su sesión con origen "detectado" aunque haya actividad de Git en **otros** worktrees. Hoy casi la mitad salen "(no agent)".

### Descripción

**Origen (dogfooding de Rene, 2026-10-09).** En `raptor timeline`, los commits que hace Claude Code en el worktree `infmcp` (Orca) salen "(no agent)", aunque la sesión estuvo activa todo el rato. Se atribuía a la carrera S3 de ADR-GRP-012 (el `git commit` corto termina antes de la muestra).

**Lo que dicen los datos** (perfil real, en solo lectura; la línea `s3_evidence` del log del daemon, desde el 2026-10-06):

| Evento | `attributed` | `ambiguous` | `no-sighting` |
|---|---|---|---|
| commit | 299 | 273 | **0** |
| push | 90 | 21 | 129 |
| rebase | 26 | 19 | 0 |

Ningún commit salió `no-sighting`: **no es la carrera de tiempo**. Los 273 son `ambiguous`, y en el almacén ninguno lleva la pista `single-session`. La regla de S3 (Enmienda 2026-10-05 de ADR-GRP-012) declara ambiguo el evento si en la ventana del lote hay **un `git` ajeno con cwd en cualquier worktree del repo**. Con unos 20 worktrees vivos, casi siempre hay uno: el sondeo de `git status` de Orca, el daemon de nx (desacoplado de su sesión), los scripts del coordinador o los `git` del propio daemon.

**Decisión del orquestador (2026-10-09), validada por Arquitecto/PO.** El encargo proponía tres opciones: (a) atribuir a la única sesión activa con un origen nuevo (`active-session`), (b) reutilizar la resolución del solicitante de Guardrails, o (c) las dos.

- **(b) ya existe**: es S4 (Enmienda 2026-10-07, PR #155). Solo actúa con los hooks de Guardrails, y el repo del dogfooding no los tiene.
- **(a) se difiere**: la "evidencia de proceso" posible (un descendiente vivo de la sesión en el worktree) se cumple casi siempre, porque los servidores MCP y las shells de la herramienta viven en el worktree, así que es co-ubicación. El trailer `Co-Authored-By` lo escribe quien hace el commit, y un commit humano con el mensaje que sugirió el agente lo lleva, que es el caso que protege BR-EDGE-004. Además, atribuir tiene efectos de permisos en la Time Machine (ADR-TMC-005). Cambiarlo exige modificar BR-EDGE-004, BR-AUTH-005 y la regla 3 de ADR-GRP-012, y eso lo ratifica Rene. Se reabre solo si, después de esta historia, el diagnóstico muestra un número relevante de commits `no-sighting`.
- **Se hace**: acotar el `git` ajeno al **ámbito del evento**. El commit del agente sale con origen "detectado", con evidencia real del proceso, que es lo que ya promete US-GRP-007.
- **Criterio del encargo reformulado (PO)**: el encargo pedía "un origen que se distinga de detected". Ahora el commit sale atribuido con origen "detectado" aunque haya actividad de Git en otros worktrees.

> Dev Spec: [DS-TS-GRP-008](../dev-specs/TS-GRP-008-dev-spec.md). Los escenarios observables están en [US-GRP-007](../user-stories/US-GRP-007-sesiones-claude-code.md) (añadidos el 2026-10-09).

### Alcance Técnico

1. **Ámbito del `git` ajeno por tipo de evento** (`Detector::evidence`, `crates/core/src/detect/mod.rs`):
   - **Ámbito worktree**, para eventos que solo produce un `git` que trabaja en ese worktree:
     - `Reset`: sale del reflog de HEAD del worktree.
     - `BranchSwitch`: sale del archivo HEAD del worktree.
     - `Commit`, `Merge` y `Rebase` con `worktree_inferred == false`: salen del reflog de la rama activa en ese worktree, y Git no deja tener la misma rama activa en dos worktrees.
   - **Ámbito repo**, el de hoy, para todos los demás tipos: `BranchUpdate` (puede venir de `branch -f`, `update-ref` o `fetch X:X` desde cualquier worktree), `BranchCreate`, `BranchDelete`, `Push`, `WorktreeCreate`, `WorktreeDelete`, y `Commit`, `Merge` o `Rebase` con worktree inferido. `Reconciled` nunca se atribuye.
   - **Siempre cuentan en todo el repo**, sea cual sea el ámbito:
     - los `git` del daemon con el cwd en el repo o ilegible: el escritor de la Time Machine corre con el cwd en `<común>/.git/worktrees/<w>`, que por ruta cae en el worktree principal, y el restore nunca se atribuye a un agente;
     - un `git` ajeno situado por el cwd de su ancestro (`launched_from`): su cwd real es desconocido, y `git -C A` lanzado desde una shell en B escribe en A;
     - un `git` ajeno con el cwd dentro del directorio Git común.
     - un `git` ajeno que redirige su destino (`-C`, `--git-dir` o `--work-tree` entre las opciones globales; `GIT_DIR`, `GIT_WORK_TREE` o `GIT_COMMON_DIR` en el entorno) o cuyo argv o entorno no se pudo leer (ajuste del coordinador, 2026-10-09; solo un booleano, nunca valores; en Windows no se lee y cuenta en todo el repo);
   - **El worktree de un cwd** se resuelve con `worktree_of` (la raíz más larga), también para los `git` de la sesión. Hoy, un `git` de una sesión en un worktree anidado (`.claude/worktrees/x`) cuenta como evidencia para el worktree principal.
2. **Diagnóstico** (SPIKE-GRP-001): la línea `s3_evidence` añade solo contadores enteros:
   - `scope` (`worktree` o `repo`);
   - `sessions_wt`;
   - `foreign_wt`;
   - `foreign_other_wt`: los que el ámbito worktree ignora;
   - `foreign_daemon`;
   - `foreign_by_ancestor`;
   - `foreign_gitdir`;
   - `foreign_redirected`;
   - `gits_after_notice`.

   Nunca rutas, nombres de worktree o de rama, pids ni argv (SEC-04).
3. **`raptor timeline` muestra la pista `inferred`** igual que `raptor events` (US-GRD-019): "no agent; inferred: Claude Code" / "sin agente; inferido: Claude Code", y "(confirmado por el trailer)" cuando aplica. Se contrasta con el trailer, nunca cambia el actor, no se muestra con `human-author` y no aparece si está `contradicted`. La salida JSON la lleva en un campo opcional. Textos en en/es según la guía de contenido de `docs/design-system`.
4. **Sin cambios**: la pista `single-session` (sigue saliendo con `NoSighting`), S4, el registro (regla 3), los valores del actor (Q34, Q35), ADR-GRP-013 y el contrato del actor.
- **Fuera de alcance**:
  - el origen `active-session` (opción a, diferida);
  - separar `BranchUpdate` por su mensaje de reflog;
  - cualquier cambio de BR-EDGE-004, BR-AUTH-005 o BR-26, que ratifica Rene.

**Riesgos declarados** (enmienda de ADR-GRP-012):

- Un `git` que redirige su destino por una vía distinta de esas opciones y variables, como `core.worktree` en la configuración.
- Una rama activa en dos worktrees (`git worktree add --ignore-other-worktrees`, `checkout --ignore-other-worktrees`).

En los dos casos, un `git` ajeno de otro worktree podría hacer un commit en la rama de A sin contar como ajeno en A.

### Plan de Verificación

Los escenarios observables están en US-GRP-007: varios worktrees vivos, el humano en el worktree del agente, dos sesiones y la carrera verdadera.

- **Tests unitarios deterministas** en `crates/core/src/detect/tests.rs`, con el `ProcLister` falso y `sample_now`, sin `sleep`:
  - el caso del dogfooding (`git` de Orca en otro worktree);
  - el humano en el worktree del agente, con el cwd legible o situado por su ancestro;
  - la shell de otro worktree con un `git` que termina;
  - el daemon con el cwd en `.git/worktrees/A` y en otro worktree;
  - el worktree inferido;
  - el worktree anidado;
  - dos sesiones;
  - `NoSighting` con la pista;
  - los eventos de refs compartidas.
- **Diagnóstico**: un test del log comprueba los contadores y que no aparecen rutas ni canarios.
- **CLI**: tests de texto (en/es) y JSON de `raptor timeline` con la pista `unconfirmed`, `confirmed` y sin pista.
- **BR-CONS-001**: el arnés de repo intacto existente (INF-GRP-001) sigue en verde; los tests nuevos son de tabla de procesos y no tocan ningún repo.
- **Dogfooding** (fuera de CI): tras instalar el binario, la proporción `ambiguous`/`attributed` de los commits en `s3_evidence` baja, y los contadores nuevos dicen qué ajenos quedan. Va a SPIKE-GRP-001.
- Linux y Windows: la regla es independiente del SO, porque usa la misma tabla de procesos; *Pendiente: etapa de validación multiplataforma* para el dogfooding.

### Estado de la implementación (2026-10-09)

Implementado en: PR #228.

- Pendiente: medir de nuevo `s3_evidence` en el dogfooding real con el binario instalado (SPIKE-GRP-001).
- Deuda de la revisión de seguridad: [TD-GRP-004](./TD-GRP-004-corroborar-ambito-worktree-s3.md).
- Linux y Windows: *Pendiente: etapa de validación multiplataforma*.
