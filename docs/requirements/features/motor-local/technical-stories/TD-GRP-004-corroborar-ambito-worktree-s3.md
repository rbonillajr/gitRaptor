---
id: TD-GRP-004
title: "Corroborar con el reflog de HEAD el ámbito worktree de S3 y blindar la lectura de la redirección"
type: td
status: draft
feature: motor-local
domain: GRP
priority: medium
complexity: medium
created: 2026-10-09
updated: 2026-10-09
related:
  adrs: [ADR-GRP-012, ADR-TMC-005]
  stories: [TS-GRP-008, US-GRP-007]
  specs: [DS-TS-GRP-008]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, deteccion, atribucion, s3, seguridad, reflog, br-edge-004]
---

## TD-GRP-004: Corroborar con el reflog de HEAD el ámbito worktree de S3 y blindar la lectura de la redirección

**Valor**: ningún commit humano puede atribuirse a una sesión de agente solo porque un `git` de otro worktree escribió en el reflog de la rama un mensaje que parece un commit.

### Descripción

**Como** desarrollador que trabaja con agentes en varios worktrees del mismo repo
**Quiero** que el motor atribuya un commit al agente de un worktree solo con pruebas de que el `git` de ese worktree lo escribió
**Para** que el agente nunca pueda deshacer mi trabajo (ADR-TMC-005) porque se le atribuyó

> Dev Spec: Pendiente

**Origen: revisión de seguridad de TS-GRP-008 (2026-10-09).** Los hallazgos son M-01, L-03 y L-05 del `security-expert`. M-01 se declaró como riesgo en la Enmienda (2026-10-09) de ADR-GRP-012, en vez de arreglarlo en el PR de TS-GRP-008. **Decisión del orquestador (2026-10-09), validada por la revisión de seguridad**, que ofrecía esa vía: es Medium, no bloquea, y el arreglo choca con el contrato de TS-GRP-008 (ver el BLOQUEO).

- **M-01**: el ámbito worktree de S3 se aplica a `Commit`, `Merge` y `Rebase` con worktree no inferido. Pero `kind_of` (`watch/repo.rs`) saca el tipo del **texto libre** del reflog de la rama, y `place()` también da "no inferido" en el paso heurístico `names_branch`. Ejemplo: `git update-ref -m "commit: x" refs/heads/<rama de A>`, lanzado desde el worktree B, produce un `Commit` situado en A. Si a la vez hay un `git` del agente en A, el evento se atribuye a la sesión.
- **L-03**: entre el listado de procesos y la lectura del argv o del entorno, un pid reutilizado haría leer los de otro proceso. La ventana es de microsegundos a milisegundos. El cwd ya tenía el mismo patrón.
- **L-05**: un `git` ajeno con el cwd fuera del repo se ignora antes de mirar si redirige su destino (`--git-dir` de plumbing desde `/tmp`).

**El BLOQUEO de TS-GRP-008.** Corroborar exige un dato nuevo en el evento. Si va en `GitEventDetails` (api, `deny_unknown_fields`) con valor por defecto "corroborado", falla abierto, que es justo lo contrario de la regla. Si va en `RawEvent`, hay que cambiar un test del contrato de TS-GRP-008. Además, en un `rebase` la transición old → new de la rama no aparece en una sola línea de `logs/HEAD`. Por eso esta historia necesita su propio diseño.

### Alcance Técnico

- **Corroboración**: el watcher marca un `Commit`, `Merge` o `Rebase` como corroborado cuando el `logs/HEAD` del worktree registró ese movimiento. En un `rebase`, una entrada `rebase*` con el mismo `new`. El campo es interno del core y falla cerrado: por defecto, no corroborado. `s3_scope` exige la corroboración para el ámbito worktree, y el paso `names_branch` cuenta como no corroborado.
- **Identidad del proceso al leer la redirección** (L-03):
  - Linux: `openat` sobre `/proc/<pid>` y comparación del `starttime` de `stat`.
  - macOS: volver a leer `pbi_start_tv*` después de `KERN_PROCARGS2`.
  - Si no coinciden, `None`.
- **L-05**: decidir con el Arquitecto si se amplía la lectura a los `git` con cwd fuera del repo (más lectura bajo SEC-04) o se mantiene como hueco declarado.
- **Fuera de alcance**: el origen `active-session` (diferido en TS-GRP-008) y separar `BranchUpdate` por su mensaje.

### Plan de Verificación

- **Test con un repo temporal**: un `update-ref -m "commit: x"` sobre la rama de A, lanzado desde B, da ámbito repo. Un `git commit`, un `merge` y un `rebase` en A dan ámbito worktree.
- **Test del detector**: un pid reutilizado entre el listado y la lectura da `None`, y el `git` cuenta en todo el repo.
- **Medición**: se repite la medición de TS-GRP-008 (`measure_quick_commits_with_a_foreign_git_in_another_worktree`) y la proporción de commits atribuidos no baja.
