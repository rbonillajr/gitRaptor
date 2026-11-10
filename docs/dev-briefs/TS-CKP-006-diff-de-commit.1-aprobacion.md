---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/cockpit/dev-specs/TS-CKP-006-diff-de-commit.md"
---

Lee este fichero y luego `TS-CKP-006-diff-de-commit.md`; `TS-CKP-006-diff-de-commit.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas en la ronda 2 sin un plan aprobable; ver «Diferidos»
- Generada: `2026-11-10T19:37:49.962Z`
- Sha de aprobación: `4eb62557f9b85136ce39ad0094708ecb0c1c0a56f4617f47ff979e1c1728a73d:a8a75e3d55bce6d09c83efe283394839053b402befda5c4402c1d7ade92ef85d:9cb82f71ce903fecd65b70f2cefb62ec6a96ec4ee36270a04e96516234041886`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES d4f93e222303>>>
```text
intent: La app de escritorio no puede mostrar el diff de un commit del grafo ni de la búsqueda, ni el diff entre dos commits, ni el índice partido en preparado y sin preparar: el canal solo sirve diff.worktree y la app no lee Git por su cuenta (Q-DSK-1, ADR-GRP-017). Eso bloquea T007 de US-DSK-009 y partes de US-DSK-012 y US-DSK-017.
endState: El daemon responde diff.commit, diff.between y diff.index por el mismo QueryPool, con los topes, la cancelación y el reparto a una sola conexión de diff.worktree; el oid se valida estricto y todo lo inexistente, inalcanzable o cortado por plazo da el mismo not-found; diff.index separa staged, unstaged y conflictos con ids de contenido; ningún método se ofrece al perfil mcp; los canarios de frontera (CKP006-008) no saltan con ningún método nuevo.
nonGoals:
  - La vista del diff y el comando diff_commit en apps/desktop (US-DSK-009 T007)
  - Preparar por fragmento (TS-CKP-008)
  - Lista de commits y ramas (TS-CKP-007)
  - Repartir la cola global por solicitante (TD-GRP-009)
  - Diff combinado de merge y detección de renombrados
  - Cambiar neutralize_programs ni el lector compartido reader.rs
  - Medir el p95 del banco en release dentro de esta verificación
```
<<<FIN-DATOS-NO-CONFIABLES d4f93e222303>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 105188c4d685>>>
```text
[1] id: CKP006-001
statement: Contrato: diff.commit, diff.between y diff.index están en hello.methods de TUI y app y no en raptor-mcp; una conexión mcp que los invoca recibe not-found; un campo de más es invalid-params (D9, R6)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_001
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[2] id: CKP006-002
statement: Oid: abreviado, mayúsculas, longitud 39/41/63/65, '-abc…', 'HEAD~', '@{-1}' y ':/regex' dan invalid-params (D4, R1)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_002
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[3] id: CKP006-003
statement: Forma: raíz contra árbol vacío; merge contra el primer padre con parents=2 y merge=true; git mv como borrado y añadido; diff.between con ruta trae solo ese archivo; dos historias sin ancestro común dan el diff de árbol a árbol; rutas con '..', NUL, control o de más de 4.096 bytes son invalid-params (D3, D6, D12, L-01)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-git --test ts_ckp_006_commit_diff ckp006_003
tests:
  - crates/git/tests/ts_ckp_006_commit_diff.rs

[4] id: CKP006-004
statement: Índice: preparado, a medio preparar y sin seguimiento separados; conflicto en conflicts y fuera de las secciones; ids de contenido del índice, de HEAD y del working tree (hash crudo, sin filtros, también con filter.x.clean armado) que salen del mismo índice que la pertenencia a cada sección (D7, D8)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-git --test ts_ckp_006_index_diff ckp006_004
tests:
  - crates/git/tests/ts_ckp_006_index_diff.rs

[5] id: CKP006-005
statement: Alcanzabilidad: blob suelto, commit huérfano, oid de otro repo, oid de otro tipo de hash y recorrido cortado por plazo dan el mismo not-found con el mismo mensaje que un oid inexistente; el corte por plazo se fuerza por el canal con test_diff_deadline (D5)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-git --test ts_ckp_006_commit_diff ckp006_005 && CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_005
tests:
  - crates/git/tests/ts_ckp_006_commit_diff.rs
  - crates/core/tests/ts_ckp_006_diff_query.rs

[6] id: CKP006-006
statement: Topes: archivo de 40 MB too-large; .png binary; más de 1.000 archivos cut files con files_total real; respuesta <= 768 KiB; diff.index con 1.000 conflictos de 4 KiB: conflicts <= 64 KiB, conflicts_total=1000 y respuesta <= 768 KiB (M-01)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_006
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[7] id: CKP006-007
statement: Cancelación y cola: query.cancel sobre un diff.commit en vuelo da query-cancelled; una segunda consulta de la misma conexión da query-busy al momento; cola llena da query-busy (D1)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_007
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[8] id: CKP006-008
statement: Frontera: diff.external sin subsección, diff.x.textconv, filter.x.clean, gpg.program y .gitattributes armados con Canary::arm; tras diff.commit, diff.between y diff.index el canario no salta (D10, R7)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_008_no_exec
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[9] id: CKP006-010
statement: Secretos (Validación 8): el diff con el secreto plantado llega a la conexión que lo pidió (contiene el texto); tras pedirlo varias veces, perfil y logs tienen 0 hallazgos del secreto, el stream no lleva contenido y otra conexión no recibe la respuesta (D13, SEC-05)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_010
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[10] id: CKP006-011
statement: Logs: tras cada consulta existe una línea diff_commit, diff_between y diff_index (según el método) y solo lleva repo, ms, files y cut; sin oids, rutas ni líneas (D13)
verify: CARGO_BUILD_JOBS=4 cargo test -p gitraptor-core --test ts_ckp_006_diff_query ckp006_011
tests:
  - crates/core/tests/ts_ckp_006_diff_query.rs

[11] id: CKP006-012
statement: El banco diff_commit existe y está declarado en crates/core/Cargo.toml; el gate de p95 (§ Valores numéricos) se mide a mano en release, no en esta verificación (T006)
verify: test -f crates/core/benches/diff_commit.rs && grep -q 'name = "diff_commit"' crates/core/Cargo.toml
tests:
  (ninguno)
```
<<<FIN-DATOS-NO-CONFIABLES 105188c4d685>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 25a3c8011cce>>>
```text
docs/requirements/features/cockpit/dev-specs/TS-CKP-006-diff-de-commit.md
```
<<<FIN-DATOS-NO-CONFIABLES 25a3c8011cce>>>

planSha: `4eb62557f9b85136ce39ad0094708ecb0c1c0a56f4617f47ff979e1c1728a73d` (base de la cadena de re-planes)

Léelo en `TS-CKP-006-diff-de-commit.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 48b4bcb22dd3>>>
```text
docs/dev-briefs/ts-ckp-006-commit-diff.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 48b4bcb22dd3>>>

contractSha: `a8a75e3d55bce6d09c83efe283394839053b402befda5c4402c1d7ade92ef85d`

## Tests de criterio

testsSha: `9cb82f71ce903fecd65b70f2cefb62ec6a96ec4ee36270a04e96516234041886`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 64df252cb69b>>>
```text
bd9b787e638fa4a155bebcf7e781617fab03ded925ec8034e986ce4ba9e5c355  crates/core/tests/ts_ckp_006_diff_query.rs
110014facd9766204bac53b33b3f3d318a93fd94e384d4097d9b017955533b50  crates/git/tests/ts_ckp_006_commit_diff.rs
c6af21ba876cec7af31f98aff033daa081cc6b8080bbcea555770318c16608c2  crates/git/tests/ts_ckp_006_index_diff.rs
```
<<<FIN-DATOS-NO-CONFIABLES 64df252cb69b>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 6 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 3 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

3 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES aea62e4e504d>>>
```text
id: PC-8
kind: contradiccion
toHuman: false
evidence: docs/requirements/features/cockpit/dev-specs/TS-CKP-006-diff-de-commit.md:267
ask: ChannelConfig::test_diff_deadline en channel/mod.rs:131

id: PC-9
kind: test
toHuman: false
evidence: docs/requirements/features/cockpit/dev-specs/TS-CKP-006-diff-de-commit.md:329
ask: guard cita un test inexistente

id: PC-10
kind: contrato
toHuman: false
evidence: docs/dev-briefs/ts-ckp-006-commit-diff.contract.json:8
ask: endState y línea 618 aún exigen CKP006-FIT
```
<<<FIN-DATOS-NO-CONFIABLES aea62e4e504d>>>

## Firmas

- planSha (contexto, no firmado): `4eb62557f9b85136ce39ad0094708ecb0c1c0a56f4617f47ff979e1c1728a73d`
- contractSha: `a8a75e3d55bce6d09c83efe283394839053b402befda5c4402c1d7ade92ef85d`
- testsSha: `9cb82f71ce903fecd65b70f2cefb62ec6a96ec4ee36270a04e96516234041886`
- approvalSha: `4eb62557f9b85136ce39ad0094708ecb0c1c0a56f4617f47ff979e1c1728a73d:a8a75e3d55bce6d09c83efe283394839053b402befda5c4402c1d7ade92ef85d:9cb82f71ce903fecd65b70f2cefb62ec6a96ec4ee36270a04e96516234041886`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
