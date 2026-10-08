---
id: SPIKE-GRD-001
title: "Interceptabilidad, coexistencia y coste de la capa de hooks en los tres SO"
type: spike
status: partially-implemented
feature: guardrails
domain: GRP
priority: critical
complexity: medium
created: 2026-10-04
updated: 2026-10-08
related:
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-005, ADR-GRD-007]
  stories: [US-GRD-001, US-GRD-002, US-GRD-003, US-GRD-004, US-GRD-007]
  specs: []
ado:
  id: null
  url: null
tags: [guardrails, spike, hooks-git, interceptabilidad, reference-transaction, husky, lefthook, pre-commit, worktreeconfig, latencia, windows]
---

## SPIKE-GRD-001: Interceptabilidad, coexistencia y coste de la capa de hooks en los tres SO

> **Estado (2026-10-04)**: **Done para macOS** (Git 2.38.5, 2.50.1 y 2.56.0, esta también con reftable). Research Brief: [`research/SPIKE-GRD-001-resultados.md`](../research/SPIKE-GRD-001-resultados.md). Las 17 enmiendas recomendadas se aplicaron en ADR-GRD-001 y ADR-GRD-002, con efectos en ADR-GRD-003 y ADR-GRD-005, y las garantías al usuario quedaron en Q-GRD-28 a Q-GRD-31. **Pendientes**: Linux, Windows (incluido el coste, con el dispatcher nativo) y los casos sin verificar del § 11 de los resultados. Siguen bloqueando el merge de US-GRD-001 y el cierre de las Dev Specs de US-GRD-002 y US-GRD-004.

**Valor**: confirmar con mediciones reales la matriz de ADR-GRD-002 y el diseño de instalación de ADR-GRD-001, antes de que la lista publicada de operaciones no impedibles y el encadenado con gestores se conviertan en contrato.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-GRD-001-interceptabilidad-hooks.md`. Es un prototipo aislado: dispatchers de prueba y repos temporales, sin código del daemon. **Depende de**: nada; arranca el día uno, en paralelo con el motor. **Valida**: ADR-GRD-001, ADR-GRD-002, la parte de detección de ADR-GRD-005 y la normalización y el criterio de Windows de ADR-GRD-007. **Bloquea**: el merge de US-GRD-001 (la parte de force-push y borrado de la rama base, y el coste en Windows) y el cierre de las Dev Specs de US-GRD-002 y US-GRD-004.

### Pregunta

¿Una carpeta de dispatchers activada con la clave local de hooks puede hacer tres cosas?

- Interceptar **antes de cualquier efecto** las operaciones que ADR-GRD-002 declara impedibles.
- Encadenar sin alterarlos los hooks propios y los de husky, lefthook y pre-commit.
- Cubrir todos los worktrees y cumplir el presupuesto de < 100 ms por evaluación en Windows, macOS y Linux, con Git 2.38 y con la última versión estable.

### Hipótesis

- **Momento A** (antes de cualquier efecto): force-push y borrado remoto con `pre-push`, borrado local con `reference-transaction` en `prepared`, commit con `pre-commit` y rebase con `pre-rebase`.
- **Momento B o C** (después de efectos parciales, o sin hook): `reset --hard`, merge y borrar worktree.
- **Crear worktree**: solo es impedible cuando crea una rama nueva.
- **`--no-verify`** no salta `reference-transaction`. `-c` con la clave de hooks y las variables de configuración por entorno sí saltan todos los hooks.
- **Gestores**:
  - husky reescribe la clave al reinstalarse.
  - lefthook escribe en el directorio de hooks efectivo, es decir, en la carpeta de Guardrails.
  - pre-commit se niega a instalar si la clave está definida.
- **Coste**: la vía rápida de `reference-transaction` cabe en < 30 ms p95 en macOS y Linux; en Windows, el arranque del `sh` de Git for Windows es el coste dominante.
- **Configuración de Git**: la escritura y el borrado de la clave local dejan el archivo de configuración idéntico byte a byte.

### Experimento

- **Matriz**: por cada operación de BR-VAL-002, ejecutarla con Git crudo, registrar qué hooks corren y en qué orden, y tomar la huella del working tree, el índice y las refs en el momento de cada hook. Incluye `pull --rebase`, `send-pack`, `commit -a` rechazado, merge fast-forward y crear worktree con y sin rama nueva.
- **Saltos**: `--no-verify` en cada operación; `-c` con la clave de hooks; variables de configuración por entorno; plumbing (`update-ref`, `commit-tree`).
- **Force-push**: con objeto remoto ancestro, no ancestro y ausente en el repo; con `--force-with-lease`; borrado remoto.
- **Coexistencia**: instalar y desinstalar sobre hooks propios, husky, lefthook y pre-commit; reinstalar cada gestor después de instalar; comprobar contenido y efecto de sus hooks y el estado resultante.
- **Worktrees**: un worktree creado después de instalar; configuración por worktree con la clave de hooks; `include` e `includeIf` (también `onbranch:`).
- **Conjunto de dispatchers**: confirmar qué hooks cambian el comportamiento de Git por el mero hecho de existir y cuáles tienen un coste por invocación que no se justifica.
- **Coste**: p95 de la evaluación gobernada y de la vía rápida en los tres SO, con un `fetch` de 1.000 refs. Comparar en Windows el dispatcher `sh` con un ejecutable nativo, si Git for Windows lo admite.
- **Configuración de Git**: comparar byte a byte el archivo de configuración antes de instalar y después de desinstalar, con y sin un valor previo de la clave.
- **Pérdida externa**: medir en cuánto tiempo detecta un observador de prueba el cambio de la clave, la edición de un dispatcher y la creación de una configuración por worktree.
- **Alias de refs** (H-06): `Main` frente a `main` en sistemas de archivos sin distinción de mayúsculas y variantes Unicode no NFC: qué ref crea Git y qué recibe cada hook.
- **Objetos de reemplazo** (H-05): un `git replace --graft` que hace que un push que reescribe `main` parezca fast-forward; qué ve `pre-push` y qué envía Git.
- **Refs simbólicas y pseudo-refs** (M-01): `HEAD`, `main-worktree/HEAD`, `worktrees/<id>/HEAD`, refs simbólicas y pseudo-refs en la entrada de `reference-transaction`, y su frecuencia durante un rebase.
- **Repos cruzados** (M-02): `GIT_DIR` y `GIT_WORK_TREE` apuntando a otro repo; qué dispatcher corre y con qué cwd.
- **Normalización del argv del token** (J9): push sin refspec con cada valor de `push.default`, `--all`, `--mirror`, `-C`, refspecs con comodines.
- **Actualización del binario** (J4): `brew upgrade` (nuevo destino del enlace estable), reinstalación por `winget`, `npm` y script; firma y huella resultantes.
- **Dispatcher con constantes** (M-04): ruta del binario con espacios, no ASCII y comillas, con escapes octales en los tres SO; coste del paso de evaluación con entorno por allowlist más el paso de encadenado.
- **Windows** (M-07): viabilidad del criterio de humano y agente (handle, hora de creación, consola dueña) y verificación de la imagen del servidor del named pipe.

### Criterios de Éxito

- **Matriz**: confirmada o corregida fila a fila en los tres SO y las dos versiones de Git, lista para convertirse en el dato versionado de ADR-GRD-002 § 2.
- **Coexistencia**: los cuatro gestores conservan contenido y efecto, o el caso queda clasificado como "no se instala".
- **Coste**: p95 < 100 ms por evaluación gobernada en los tres SO. Objetivo de Windows fijado para la vía rápida.
- **Huella**: idéntica tras desinstalar, o la diferencia queda documentada para decidir el criterio semántico de NFR-GRD-01.
- **Vía de fracaso**:
  - Si `pre-push` o `reference-transaction` no cumplen el momento A, se replantea ADR-GRD-002 y el alcance de US-GRD-001.
  - Si el coste no cabe en un SO, se replantea ADR-GRD-001: dispatcher nativo, o sin `reference-transaction` en ese SO, con el borrado local de ramas declarado como no impedible allí.
  - Si un gestor no se puede encadenar, se amplía la lista de casos "no se instala" de ADR-GRD-001 § 6.
  - Si la normalización del argv no es fiable para un caso, ese caso se rechaza antes de emitir el token (ADR-GRD-007 § 3).
  - Si el criterio de humano y agente no es viable en Windows, se replantea ADR-GRD-007 § 2 para ese SO.

### Time-box

⚠️ **ASSUMPTION**: 2 semanas, repartidas entre los tres SO. Se amplía desde semana y media por los casos que añadió la revisión de seguridad (2026-10-04).

## Estado de la implementación (2026-10-08)

Implementado en: PR #22, #25, #121.

Estado: implementación parcial. Pendiente:
- Matriz en Linux y Windows (incluido el coste con el dispatcher nativo) y los casos sin verificar del § 11 de los resultados.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
