---
id: DS-US-GRP-012
title: "Dev Spec — El desarrollador ve el ahead/behind de cada worktree contra la rama base del repo"
type: dev-spec
status: implemented
feature: motor-local
domain: GRP
created: 2026-10-05
updated: 2026-10-08
related:
  stories: [US-GRP-012, US-GRP-001, US-GRP-002, US-GRP-016]
  enablers: [TS-GRP-002, TS-GRD-001, INF-GRP-001]
  adrs: [ADR-GRP-007, ADR-GRD-004, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011]
  rules: [BR-CONS-006, BR-CONS-001]
  nfrs: [NFR-01, NFR-10, SEC-11, SEC-12]
tags: [motor-local, rama-base, ahead-behind, cli, i18n, repo-intacto]
---

# Dev Spec — US-GRP-012: ahead/behind contra la rama base

Plano compacto (AADD ligero) de [US-GRP-012](../user-stories/US-GRP-012-rama-base-main.md). Parte de lo que ya está en `main`: la lectura de Git con la primitiva `ahead_behind` (TS-GRP-002), la rama base confirmada en el almacén por repo (TS-GRD-001, `RepoStore::confirmed_team_baseline`) y el estado de los worktrees de US-GRP-001 ([Dev Spec](./US-GRP-001-dev-spec.md)). El contrato resultante está en [api-contract-ipc.md](../../../../architecture/design/api-contract-ipc.md).

**Qué entrega**: el estado del motor dice cuál es la rama base de cada repo y si está confirmada. Para cada worktree legible dice cuántos commits lleva por delante y por detrás de ella, o por qué no se puede calcular. `raptor status [--json]` lo muestra. Nada de esto escribe en el repo ni consulta el remoto.

## 1. Decisiones

Todas son **Decisión del orquestador (2026-10-05), validada por el Arquitecto** (técnica) **y el PO** (alcance). La columna de la derecha recoge los ajustes que pidieron y que ya están incorporados.

| # | Decisión | Ajuste incorporado |
|---|---|---|
| D1 | **Qué rama base**: la **confirmada** del almacén por repo, si la hay (ADR-GRD-004 § 3.5: es el único valor de rama base del repo). Si no la hay, `main`, marcada como **no confirmada** (`base-unconfirmed`). Esta historia **no lee la configuración del equipo** (el suelo): eso es de US-GRP-016, que cambia la rama propuesta sin confirmar de `main` a la que resuelve `TeamLoader` (TS-GRD-001) en un solo punto, `observe::base_branch`. Nada de esta historia confirma una rama base (añadir el repo no confirma, D9) | Arquitecto: `observe::base_branch` devuelve ya `gitraptor_policy::team::BaseBranch`, así US-GRP-016 solo cambia de dónde sale el valor. Un valor ilegible del almacén cuenta como no confirmado. PO: la marca "no confirmada" va ya en esta historia (BR-CONS-006 no depende de que haya configuración del equipo). Usar la confirmada va más allá de "hoy `main` en todos los repos"; se acepta porque mantiene la misma rama base que Guardrails, y el PR lo dice. El escenario 1 se prueba sin base confirmada |
| D2 | **Qué ref es la rama base**: `refs/heads/<base>`, la rama local. Si no existe, el ahead/behind de cada worktree es `base-missing` y no se usa ninguna otra ref: ni `refs/remotes/<remoto>/<base>` ni una etiqueta con el mismo nombre (Q42) | Arquitecto y PO: OK. La copia remota solo sirve para leer el suelo (ADR-GRD-004 § 3.3), no como rama base. Los tests incluyen una `origin/main` conocida y una etiqueta `main` para fijarlo |
| D3 | **Contrato**: `RepoView.base = {name, status}`, con `status` = `confirmed`, `unconfirmed` o `invalid` (como `BaseStatus`; `invalid` solo llega con US-GRP-016) y `name` como `Untrusted`. Cada worktree `ready` lleva `divergence`: `counted {ahead, behind}`, cada lado `{count, exact}`; `base-missing`; `no-base` (estado `invalid`); `no-commits` (HEAD sin nacer); o `unreadable`. Un worktree `unavailable` no lleva nada | Arquitecto: dentro de `WorktreeStatus::Ready`, porque depende del `HEAD`. El estado de la base es un enum con la forma que usará US-GRP-016 |
| D4 | **Cálculo**: `RepoReader::ahead_behind_commits` (gix, en proceso, ADR-GRP-010 § 4), variante por id de commit de la primitiva de TS-GRP-002 que hace falta para un `HEAD` separado. Cuenta entre el `HEAD` del worktree y la punta de la rama base, con un recorrido acotado a 10 000 commits por lado (`MAX_DIVERGENCE_WALK`; `exact: false` al llegar al tope). Solo lectura: sin `fetch` y sin escribir el `commit-graph` (ADR-GRP-009) | — |
| D5 | **Al día sin watcher**: se calcula en la reconciliación (al añadir y al arrancar, como el resto del estado) y **otra vez en cada `engine.snapshot`** de una conexión completa, en el hilo de la conexión, con la punta de la base leída en ese momento. Es una proyección de solo lectura: no publica eventos ni persiste nada. Una caché por par de commits (`(head, base) → counts`, 256 entradas, con mutex en `ServerCtx`) evita repetir recorridos. Un fallo deja `unreadable` en ese worktree sin romper el snapshot. El perfil MCP no recalcula | Arquitecto (a): la fila no mezcla dos lecturas. La reconciliación guarda en memoria, fuera del contrato, qué era el `HEAD` (`HeadRef`). Un worktree en una rama se recalcula desde **esa rama**, la que nombra su fila, así el worktree principal en `main` no aparece "1 por detrás de `main`". Uno separado se recalcula desde el commit de la reconciliación. Arquitecto (b) y PO: solo `engine.snapshot` (y `raptor status`) está al día. Los eventos `worktree.state` llevan el valor calculado al publicarse, igual que el resto de su estado; ver Pendientes |
| D6 | **Versión**: `PROTOCOL_VERSION` 2 → 3 y `API_VERSION` 3.0.0. Los tipos rechazan campos desconocidos, así que es un cambio incompatible (como D5 de US-GRP-001). Si US-GRP-002 sube la versión antes, esta rama rebasa y toma la siguiente; una sola subida por PR | — |
| D7 | **Persistencia**: ninguna nueva. El ahead/behind es derivado y se recalcula. `KnownState.refs` ya guarda las puntas de las ramas locales, y US-GRP-002 las compara para saber que la base avanzó | — |
| D8 | **CLI**: en texto, debajo del repo, `base branch: main (unconfirmed)` / `rama base: main (no confirmada)`. En cada worktree, `3 ahead and 1 behind main` / `3 por delante y 1 por detrás de main` ("at least N" / "al menos N" si no es exacto), o `no ahead/behind: base branch "main" does not exist in the repo` / `sin ahead/behind: la rama base "main" no existe en el repo`, y lo mismo para `no-commits`, `no-base` y `unreadable`. El nombre de la rama se sanea y se sustituye el último, así un nombre como `{ahead}` no rellena otro marcador. En JSON: `base_branch` y `base_confirmed` en el repo, y `ahead_behind {state, ahead?, behind?, exact?}` en cada worktree legible. Catálogos `en`/`es` de US-GRP-001 (D11) con claves nuevas | PO: redacción de los textos. El texto de `base-missing` no nombra ninguna otra rama, para que no se lea como una sugerencia |
| D9 | **MCP**: sin cambios. `raptor-mcp` no recibe el estado de los worktrees (allowlist de US-GRP-001, D9) hasta F-001-05 | — |

## 2. Estructura

```
crates/api      messages.rs  BaseBranchView, BaseStatusView, DivergenceView, CommitCountView,
                             MAX_DIVERGENCE_WALK; RepoView.base; WorktreeStatus::Ready.divergence
                lib.rs       PROTOCOL_VERSION 3, API_VERSION 3.0.0
crates/git      reader.rs    ahead_behind_commits(a, b, limit) (por id de commit)
crates/core     observe.rs   base_branch(confirmed), base_view(); reconcile(common_dir, base);
                             RepoRead::set_base / divergence_inputs; HeadRef; DivergenceCache;
                             refresh_divergence(repos, inputs, cache)
                daemon/      repo_base(): la rama base de cada repo sale de su almacén al añadir
                             y al arrancar; EngineShared.divergence se actualiza con repos
                channel/     conn.rs: snapshot con el ahead/behind recalculado; caché en ServerCtx
apps/cli        status.rs (texto/JSON), i18n/{en,es}.txt
```

## 3. Plan de pruebas (escenario → test)

Escenarios de punta a punta con el binario `raptor` real como daemon y como cliente, en repos temporales del arnés (`apps/cli/tests/base_branch.rs`, macOS, como `repo_state.rs`).

| Escenario Gherkin | Test |
|---|---|
| Sin configuración del equipo, la rama base es main | `without_team_settings_the_base_branch_is_main`: "feat-login" con 3 commits propios y `main` con 1 que no tiene. El estado dice base `main`, `base_confirmed: false`, el principal 0/0 y `feat-login` 3 por delante y 1 por detrás, en JSON y en texto (en y es) |
| El ahead/behind se recalcula cuando avanza la rama base | `ahead_behind_follows_the_base_branch`: con el motor observando, un commit nuevo en `main` deja `feat-login` en 3 y 2 (y el principal en 0/0) sin volver a añadir el repo |
| El motor no trae novedades del remoto | `repo_intact_the_engine_does_not_fetch`: un remoto con 2 commits en `main` que el repo no conoce. El ahead/behind sale de lo conocido (3/1) y no cambia cuando el remoto avanza otra vez. `refs/remotes/origin/main` no se mueve y la huella del testkit no tiene diferencias fuera del perfil (el remoto y el otro clon los mueve el test) |
| Si la rama base no existe, el motor lo indica y no elige otra | `a_missing_base_branch_is_reported_and_no_other_is_used`: "otro" sin `main`, con `trunk`, un worktree enlazado, una `origin/main` conocida y una etiqueta `main`. Cada worktree dice `base-missing`, el texto lo dice para los dos (en y es) y no aparece ninguna cifra |

Además, `crates/core/tests/base_branch.rs` corre en todos los SO del CI: base `main` no confirmada con huella estricta; una base confirmada manda sobre `main` y `set_base` recalcula; `base-missing` con `origin/main` y una etiqueta `main` (huella estricta); HEAD separado y rama sin commits (`no-commits`); el tope del recorrido con 10 001 commits (`exact: false`); y el snapshot que sigue a la base y a la rama de cada fila, con la caché (huella estricta). En `crates/git/tests/boundary.rs` se prueba `ahead_behind_commits`. Las pruebas unitarias de la CLI cubren el texto, la forma del JSON y que un nombre de rama no rellene otros marcadores.

## 4. Pendientes

- **Rama base del equipo, `base-change-pending` e `invalid`**: US-GRP-016 (D1, D3). **Confirmarla**: US-GRD-001 / US-GRD-014.
- **Ahead/behind en vivo para los suscriptores**: US-GRP-002 (D5). Hasta entonces, `engine.snapshot` (y `raptor status`) está al día, pero un evento `worktree.state` lleva el valor de su reconciliación, que puede quedarse viejo. Cuando la segunda fase de US-GRP-002 (ADR-GRP-010 § 4, ADR-GRP-011) publique el ahead/behind al cambiar las refs, US-GRP-002 **quita el recálculo del snapshot** para no tener dos fuentes de verdad. El snapshot tampoco coincide exactamente con el flujo hasta `seq` (DEP-CKP-6): se tolera porque `worktree.state` lleva el estado completo del repo y es idempotente.
- **Linux y Windows**: los escenarios de proceso usan `script` y el canal Unix, así que solo corren en macOS. `crates/core/tests/base_branch.rs` corre en todos los SO del CI. Pendiente: etapa de validación multiplataforma.
- **Coste del recorrido con gix sin `commit-graph`** (supuesto de ADR-GRP-010 § 4): sin medir en esta historia; el tope de 10 000 y la caché lo acotan.

## Estado de la implementación (2026-10-08)

Implementado en: PR #55.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- Rama base del equipo (`base-change-pending`, `invalid`): US-GRP-016; confirmarla: US-GRD-001/014.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
