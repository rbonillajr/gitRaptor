---
id: ADR-GRD-004
title: Configuración efectiva para Guardrails — suelo en la rama principal, endurecimiento por worktree y configuración ilegible
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-003, ADR-GRD-005, ADR-GRD-007, CTX-GRD-001, BR-GRD-001, US-GRP-016]
tags: [guardrails, configuracion, q-grd-17, q-grd-18, d6, suelo, rama-base, rama-principal, br-cons-007, br-edge-004, pq-9, crates-policy, gix, refs-reemplazo]
---

# ADR-GRD-004 — Configuración efectiva para Guardrails: suelo en la rama principal, endurecimiento por worktree y configuración ilegible

## Contexto

ADR-GRP-007 (propuesto, rama `docs/arch-motor-local`) fija el contenedor de la configuración:

- **Formato**: JSON estricto con `$schema`.
- **Archivos**: `.gitraptor/settings.json` (equipo, versionado), `settings.json` (perfil) y `settings.local.json` (local, en el perfil, ADR-GRP-008).
- **Dueño**: `crates/policy` es el dueño del documento.
- **PQ-8**: un nivel inválido se ignora entero.
- **PQ-9**: manda el archivo en disco del worktree principal.

Las decisiones de Guardrails sobre esa configuración son:

- **Q-GRD-17**: rige la última versión commiteada en el worktree de la operación.
- **Q-GRD-18**: la rama base se lee de la rama principal, sin `fetch` (Q12 de motor-local).
- **Q-GRD-12, BR-EDGE-004**: ante un nivel ilegible se aplican el mínimo y lo legible, nunca "todo permitido".
- **R-GRD-8**: hay que alinear esa reacción con BR-CONS-007 (motor-local).

**Decisiones de Rene Bonilla (2026-10-04)**:

- **Decisión 1**: la contradicción con PQ-9 se resuelve a favor de Q-GRD-17 y Q-GRD-18.
- **Decisión 2**: cómo se lee la copia de la rama principal.
- **D6, "Suelo en principal"**:
  - **De dónde salen las relajaciones**: toda relajación (desactivar el mínimo, un `allow` explícito, cualquier valor que relaje frente al valor por defecto) y la rama base se leen **solo** de la copia de la rama principal.
  - **Qué aporta el worktree**: el `HEAD` del worktree de la operación (Q-GRD-17) **solo puede endurecer**.
  - **Cambio de rama base**: si la rama base resuelta cambia, la protección no baja hasta que el humano lo confirma con un comando reservado. Mientras tanto se protege la unión {anterior, nueva} y el cambio genera un evento de estado con aviso.
  - **Para el PO**: D6 se anota como **refinamiento de Q-GRD-17 pendiente para el PO**. No se edita el requerimiento.

La revisión de seguridad añade dos amenazas sobre la copia de la rama principal:

- **J3 / H-05a**: `refs/remotes/*` se pueden forjar en local con `update-ref`, sin red.
- **H-05**: los objetos de reemplazo pueden cambiar el blob leído.

## Decisión

**Un único cargador en `crates/policy` lee el nivel de equipo de objetos commiteados, sin objetos de reemplazo. La copia de la rama principal es el *suelo*: la única fuente de relajaciones y de la rama base. El `HEAD` del worktree de la operación solo endurece. Un cambio del suelo que baje la protección no se aplica hasta que el humano lo confirma.**

### 1. Cargador único (`crates/policy`)

- **Fuentes del nivel de equipo**: `suelo` (un blob de la copia de la rama principal) y `worktree` (un blob del `HEAD` de la operación). Los niveles personales son archivos (ADR-GRP-007/008).
- **Salida por fuente**:
  - `ausente`.
  - `legible(documento)`, con diagnósticos de claves desconocidas o fuera de nivel.
  - `ignorado(diagnósticos)`, con el criterio de PQ-8 (JSON o schema inválido, tamaño, entrada no regular).

  Los diagnósticos nunca llevan contenido (SEC-11 de motor-local).
- **Límites del documento** (L-03):
  - tamaño máximo;
  - profundidad máxima de anidamiento;
  - número máximo de claves por objeto y de elementos por lista;
  - longitud máxima de cadena.

  ⚠️ **ASSUMPTION**: 64 KiB, profundidad 16, 256 claves o elementos y 1 KiB por cadena. Superar cualquiera da `ignorado`.
- **Lecturas sin reemplazo** (H-05): todos los blobs y árboles se leen con los **objetos de reemplazo desactivados** (`refs/replace/*`) y sin grafts, en el daemon y en el modo degradado.
- **Cada consumidor decide qué hace con `ignorado`**:
  - **Motor** (ADR-GRP-007): ignora la fuente.
  - **Guardrails** (BR-EDGE-004, Q-GRD-12): aplica el mínimo, que no se puede desactivar mientras el suelo esté `ignorado`, más lo `legible`, y avisa.
- **Claves desconocidas en `permissions` o `policies`**. ⚠️ **ASSUMPTION** (pendiente de Rene): la fuente queda `parcial`. Aplican las reglas que se entienden y **se fuerza el mínimo**.
- **Alineación con BR-CONS-007 (motor-local)**: un solo criterio de validez, con reacciones distintas por consumidor. Para cerrarlo hacen falta **varias enmiendas de ADR-GRP-007**, no solo la de PQ-9: las lista la tabla de [non-functional-guardrails.md](../non-functional-guardrails.md) (J10).

### 2. Suelo y worktree (D6, Q-GRD-17 refinada)

| Fuente | De dónde se lee | Qué puede aportar |
|---|---|---|
| **Mínimo** | Producto | Denegar force-push y el borrado de la rama base |
| **Suelo** | Blob `.gitraptor/settings.json` de la **copia de la rama principal** (§ 3) | **Todo**: endurecer y relajar (desactivar el mínimo, `allow` explícito, valores menos restrictivos que el valor por defecto) y la rama base |
| **Worktree** | Blob del commit al que apunta el `HEAD` del worktree de la operación (Q-GRD-17) | **Solo endurecer**: se combina como un nivel personal (BR-CONS-001). No desactiva el mínimo ni cambia la rama base |
| **Perfil** y **local** | Archivos (ADR-GRP-007/008) | Solo endurecer (BR-CONS-001) |

- **Efectiva** = mínimo (salvo que el suelo lo desactive) ∪ suelo, endurecido por worktree, perfil y local con las reglas de combinación de BR-CONS-001:
  - las listas se unen;
  - los límites y los plazos se quedan con el menor;
  - el formato de commit es el del suelo si lo fija y, si no, el más específico que lo exija.
- **Casos límite del worktree**:
  - `HEAD` sin nacer o sin la ruta → la fuente es `ausente`.
  - `HEAD` separado → el commit al que apunta.
  - Rebase en curso → el `HEAD` del momento de cada evaluación.
- **Validación de la entrada** (SEC-11): solo un blob regular, nunca un enlace ni un submódulo, con los límites del § 1.
- **Caché**: LRU acotada de documentos parseados, indexada por el identificador del blob (L-01).
- **Qué cambia frente a Q-GRD-17 literal** (refinamiento para el PO):
  - Una relajación commiteada en la rama de un worktree **nunca** rige hasta que llega a la copia de la rama principal.
  - Un endurecimiento que ya está en la copia principal rige **en todos** los worktrees, aunque no lo hayan integrado.
  - El ejemplo de BR-VAL-001 ("en `feat-y` rige la versión commiteada en `feat-y` hasta que integre ese commit") solo se mantiene para endurecimientos del propio worktree.

### 3. Rama principal y rama base (Q-GRD-18; decisión 2; D6)

1. **Remoto**: `origin` si hay varios; el único que haya si hay uno solo. Si hay varios y ninguno es `origin`, no hay remoto.
2. **Rama principal**: el destino de `refs/remotes/<remoto>/HEAD`, si existe. Si no, `main`.
3. **Copia de la rama principal**, la primera que exista: `refs/remotes/<remoto>/<principal>` (la copia conocida, **sin `fetch`**), `refs/heads/<principal>` o ninguna.
4. **Rama base resuelta**: `engine.baseBranch` del suelo legible; si no, `main`.
5. **Rama base confirmada** (D6; Judge ronda 2, hallazgo 6): el daemon la guarda por repo en el perfil. **Es el único valor de rama base del repo**: contra ella calcula el motor el ahead/behind (US-GRP-016) y es la que protege Guardrails.
   - **Primera confirmación, siempre del humano**: no hay ninguna vía automática. La rama base y el suelo iniciales los confirma el humano con un comando reservado (ADR-GRD-007, D8), mostrando la rama base, el suelo y su origen:
     - al **añadir el repo a la observación**, que ya es un comando reservado del desarrollador (ADR-GRP-005 § 6);
     - en un repo **ya observado**, al **instalar la protección** o con una **confirmación explícita**.
   - **Mientras no hay confirmación inicial**:
     - **Guardrails** protege la unión {`main`, la rama principal, la rama base resuelta} y aplica el suelo solo para endurecer, como en el modo degradado.
     - **El motor** muestra la rama base como "pendiente de confirmar" y **calcula el ahead/behind contra la resuelta, marcado como no confirmado**.
     - **Por qué esta opción y no dejar de calcular**: el ahead/behind es información, no protección. Calcularlo contra una rama forjada no baja ninguna protección, y la marca evita que se lea como definitivo. No calcularlo dejaría US-GRP-016 sin datos en cada repo observado antes de esta regla, hasta una confirmación que puede tardar.
   - **Si la rama base resuelta difiere de la confirmada**:
     - El motor y Guardrails **siguen usando la confirmada**.
     - La resuelta aparece **solo como diagnóstico**, `base-change-pending`, con aviso (ADR-GRD-005).
     - **Refuerzo propio de Guardrails**: mientras el cambio está pendiente, el mínimo protege además la resuelta, es decir, la **unión** {confirmada, resuelta}. El motor no usa la unión.
     - Solo el humano confirma el cambio, con un comando reservado (ADR-GRD-007, D8). Entonces la resuelta pasa a confirmada para los dos.
6. **Fail-safe**: si el suelo está `ignorado`, Guardrails protege {`main`, la rama principal, la rama base confirmada}. El motor sigue usando la confirmada.

Con esto:

- **Un commit local sin push** en la rama principal no cambia el suelo mientras haya copia de seguimiento.
- **Una sola rama base por repo**, la confirmada, para Guardrails y para el motor (BR-CONS-003).

### 4. Suelo no forjable en silencio (J3 / H-05a; SEC-GRD-17)

- **La amenaza**: un agente puede forjar la copia de la rama principal sin red, con `git update-ref refs/remotes/origin/main <commit>` o moviendo `refs/remotes/origin/HEAD`. También puede llevar a la rama principal una configuración más laxa mediante un push permitido, que luego se trae con un `fetch`. En cualquiera de los dos casos el suelo, la única fuente de relajaciones, cambia.
- **Suelo confirmado**: el daemon guarda por repo el identificador del blob del suelo **confirmado**. La confirmación inicial la hace siempre el humano, como la de la rama base (§ 3).
- **Cuando el suelo resuelto cambia**, el daemon lo compara con el confirmado:
  - **Solo endurece o es igual**: rige de inmediato, sin confirmación.
  - **Relaja algo** (desactiva el mínimo, cambia un permiso a menos restrictivo, quita una rama o una ruta, sube un límite):
    - Rige la **combinación más restrictiva** de los dos suelos, como si el nuevo solo pudiera endurecer.
    - El estado genera el evento `floor-relax-pending` con aviso.
    - El humano confirma con un comando reservado (mecanismo MVP de D5, por D8), y entonces el nuevo suelo pasa a confirmado.

  **Decisión D7 de Rene Bonilla (2026-10-04)**: confirma esta ampliación de D6. Toda relajación que llegue por un cambio en la copia de la rama principal espera la confirmación del humano, no solo la rama base. Hasta entonces rige la combinación más restrictiva y se avisa. **Afecta al requerimiento** (pendiente para el PO): BR-EDGE-001 ("el equipo lo puede desactivar") y BR-CONS-003, porque cada relajación del equipo exige una confirmación humana en cada máquina.
- **Por qué funciona**: es el mismo mecanismo de "no baja hasta confirmar" de D6. No hace falta distinguir un `fetch` de un `update-ref`, cosa que Git no permite saber desde un hook.

### 5. Dependencias y cuándo entra cada pieza

- **Enmiendas en motor-local**: la de PQ-9 (decisión 1), las secciones `permissions`/`policies` con la clave del mínimo, el estado por fuente del cargador y la lectura sin reemplazo. No se editan desde aquí; están todas en la tabla de enmiendas.
- **US-GRP-016** (motor) y **US-GRD-014** leen la rama base con la misma función. La prueba de integración que pide el índice de historias pasa por construcción.
- **US-GRD-001..006 no leen configuración**: aplican el mínimo con la rama base `main` (US-GRP-012).
- **TS-GRD-001** provee las dos fuentes commiteadas, el suelo y la detección de cambios del suelo y de la rama base. Está bloqueado hasta que ADR-GRP-007 pase a `accepted` con las enmiendas.
- **Los comandos de confirmación** (rama base y suelo) los aportan US-GRD-014 y US-GRD-007. Son acciones que relajan y usan el mecanismo MVP de D5. **No esperan al factor fuera de banda** (D8; ADR-GRD-007 § 1).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| PQ-9 tal cual (archivo en disco del worktree principal) | Relajable sin commitear (contradice Q-GRD-17). Descartada por la decisión 1 |
| Q-GRD-17 literal (el `HEAD` del worktree rige entero) | Un agente saca un commit antiguo o una rama huérfana, o commitea en su rama una configuración más laxa, y relaja las reglas en su worktree. Lo cierra D6 |
| Rama base desde el `HEAD` de cada worktree | Varias ramas base por repo. Contradice Q-GRD-18 |
| `fetch` para conocer la rama principal actual | Red y escritura. Contradice Q12, NFR-03 y ADR-GRP-009 |
| Confiar en `refs/remotes` sin suelo confirmado | Se forja en local sin red (J3) |
| Respetar los objetos de reemplazo | `git replace` sustituye el blob de configuración leído (H-05) |
| Dos cargadores (motor y Guardrails) | Dos criterios sobre el mismo documento (R-GRD-8) |

## Consecuencias

- ✅ Las ediciones sin commitear, un checkout antiguo, una rama huérfana o un commit laxo en la rama de un agente **no relajan nada**. D6 cierra el riesgo de Q-GRD-17 y el hueco entre US-GRD-007 y US-GRD-012 que señaló la primera ronda.
- ✅ La rama base es única, sin red y estable. Un cambio de rama base nunca deja ninguna de las dos sin proteger.
- ✅ Una sola lectura y un solo criterio de validez para el motor y Guardrails.
- ⚠️ **Refinamiento de Q-GRD-17 (D6) pendiente para el PO.** Cambia el ejemplo de BR-VAL-001 y la decisión anotada en el contexto. No se edita el requerimiento.
- ⚠️ **Confirmación de relajaciones del suelo** (§ 4, D7): cada relajación que el equipo integre en la rama principal exige una confirmación humana por máquina. Es el coste de un suelo no forjable. Afecta a BR-EDGE-001 y BR-CONS-003 (pendiente para el PO).
- ⚠️ **Rama base confirmada también para el motor** (§ 3): un cambio de `engine.baseBranch` en la rama principal no cambia el ahead/behind del motor hasta que el humano lo confirma. Afecta a US-GRP-016 (motor-local); está en la tabla de enmiendas.
- ⚠️ **Contradicción con ADR-GRP-007 PQ-9**, resuelta por la decisión 1. **Dependencia**: las enmiendas de motor-local.

## Validación

1. **D6, worktree solo endurece**: en el worktree `feat-x`, un commit de un agente que permite force-push o desactiva el mínimo no cambia la decisión. Un commit que deniega push en `feat-x` sí rige en `feat-x`.
2. **Checkout antiguo y rama huérfana**: sacar un commit sin configuración, o una rama huérfana, no retira las reglas del suelo.
3. **Endurecimiento en la copia principal**: un `deny` que llega a `origin/main` rige en todos los worktrees.
4. **Sin commitear**: una edición o un conflicto en el working tree no cambian nada y no dan aviso de ilegible (US-GRD-011).
5. **Rama principal**: con `origin/HEAD → origin/trunk` se usa `origin/trunk`. Sin remoto, `refs/heads/main`. Sin nada, `main`. Con varios remotos sin `origin`, la rama local.
6. **Cambio de rama base** (D6; Judge ronda 2): con el suelo cambiando `baseBranch` de `main` a `develop`:
   - **Hasta la confirmación**: Guardrails deniega el borrado de `main` y el de `develop`. El motor sigue calculando contra `main` (la confirmada). `develop` aparece como diagnóstico `base-change-pending`, con aviso.
   - **Tras confirmar**: el motor y Guardrails usan solo `develop`.
7. **Forja** (J3): `git update-ref refs/remotes/origin/main <commit con suelo laxo>` → la relajación no rige, hay aviso `floor-relax-pending` y rige la combinación más restrictiva. Un `update-ref` de `origin/HEAD` hacia otra rama → `base-change-pending` y la unión protegida.
8. **Reemplazo** (H-05): `git replace <blob del suelo> <blob laxo>` no cambia el documento leído.
9. **Límites** (L-03): un JSON con profundidad 1.000, un millón de claves o una cadena de 10 MB da `ignorado` y el mínimo activo, sin agotar memoria.
10. **Sin confirmación inicial** (Judge ronda 4): en un repo observado sin confirmación, Guardrails deniega el borrado de `main`, el de la rama principal y el de la resuelta. El motor muestra el ahead/behind contra la resuelta marcado como no confirmado. Ninguna lectura confirma nada por sí sola, y tras la confirmación explícita los dos usan la confirmada.
11. **Mismo cargador y mismo valor**: el motor y Guardrails obtienen la misma rama base **confirmada**, también con un cambio pendiente (prueba de integración de US-GRD-014 y US-GRP-016). La unión solo aparece en las decisiones de Guardrails.
12. **Sin escrituras ni red** (INF-GRP-001).

## Referencias

- **Reglas**: BR-VAL-001, BR-CONS-001, BR-CONS-003, BR-EDGE-001, BR-EDGE-004, BR-AUTH-004; Q-GRD-7, Q-GRD-12, Q-GRD-17, Q-GRD-18; R-GRD-4, R-GRD-8.
- **Decisiones**: decisiones 1 y 2, D6, D7 (suelo confirmado) y D8 (confirmación con el mecanismo MVP de D5), de Rene Bonilla (2026-10-04).
- **Motor local**: BR-CONS-006 (motor-local), BR-CONS-007 (motor-local); Q12, Q23, Q24 de motor-local; US-GRP-012, US-GRP-016.
- **Historias**: US-GRD-001, US-GRD-007, US-GRD-011, US-GRD-012, US-GRD-014.
- **ADRs de otros frentes**: ADR-GRP-007 (PQ-8, PQ-9), ADR-GRP-008, ADR-GRP-009, ADR-GRP-010 (`docs/arch-motor-local`).
- **Enablers**: TS-GRD-001.
- **Seguridad**: SEC-GRD-13, SEC-GRD-17.

## Revisión de seguridad (2026-10-04)

| Hallazgo | Cómo se cubre |
|---|---|
| H-05 / D6 · El `HEAD` del worktree relaja | § 2: el suelo en la rama principal es la única fuente de relajaciones y de la rama base, y el worktree solo endurece; Validación 1 a 3 |
| J3 / H-05a · `refs/remotes` forjable | § 4: suelo confirmado y combinación más restrictiva mientras no se confirma una relajación (D7, Rene Bonilla, 2026-10-04); § 3: rama base confirmada y unión; Validación 6 y 7 |
| Judge ronda 2, hallazgo 6 · Rama base pendiente | § 3: la confirmada es el valor único para el motor y Guardrails; la pendiente es un diagnóstico; la unión es solo un refuerzo de Guardrails; Validación 6 y 11 |
| Judge ronda 4 · Confirmación inicial | § 3.5: sin vía automática; la confirmación inicial siempre es del humano (al añadir el repo, al instalar o explícita); mientras falta, Guardrails protege {`main`, principal, resuelta} y el motor calcula contra la resuelta marcada como no confirmada (opción justificada); Validación 10 |
| H-05 · Objetos de reemplazo y grafts | § 1: lecturas sin reemplazo; Validación 8 |
| L-03 · Límites del JSON | § 1: tamaño, profundidad, claves y cadenas; Validación 9 |
| L-01 · Caché | § 2: LRU acotada |
| J10 · "No hace falta cambiar motor-local salvo PQ-9" | § 1 y § 5: corregido, con remisión a la tabla de enmiendas |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes |
