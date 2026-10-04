---
id: ADR-GRP-007
title: Configuración en tres niveles — formato JSON con `$schema`, estructura y precedencia
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-004, ADR-GRP-005, ADR-GRP-006, ADR-GRP-008, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRD-003, ADR-GRD-004, TS-GRD-001]
tags: [configuracion, settings-json, json-schema, schemars, precedencia, niveles, guardrails, motor-local, p8, seguridad, pq-9-sustituida, suelo, rama-base-confirmada, refs-reemplazo]
---

# ADR-GRP-007 — Configuración en tres niveles: formato y precedencia

> **Feature:** `motor-local` (F-001-01). Cierra la pregunta **P8** del contexto. **Coautoría:** las secciones `permissions` y `policies`, la clave del mínimo seguro y la regla de que un nivel personal no relaja una prohibición del equipo son de Guardrails (F-001-04). Este ADR fija el contenedor, el formato, los niveles, el cargador y la sección `engine`. Coautoría cerrada con ADR-GRD-003/004.
>
> **Enmienda (2026-10-04, Guardrails)**: **PQ-9 queda sustituida** por la decisión 1 de Rene Bonilla (2026-10-04): el nivel de equipo se lee de lo commiteado y la rama base efectiva es la confirmada por el humano. Detalle en "Qué configuración de equipo manda" y en la sección final [Enmienda (2026-10-04, Guardrails)](#enmienda-2026-10-04-guardrails).

## Contexto

El motor **lee** su configuración en tres niveles y **nunca la escribe** (Q23, BR-CONS-007). Cada valor declara qué niveles lo admiten y, dentro de ellos, gana el más específico (Q24):

- **Rama base**: solo el nivel de equipo; `main` por defecto (BR-CONS-006).
- **Umbral de inactividad**: solo el perfil y el nivel local; 5 minutos por defecto (BR-TIME-001, Q20).

El comando para editar la configuración es de Guardrails (Q27). La misma configuración de equipo aloja las políticas por repo de BR-11, y el BRD v0.4 dejó de fijar el archivo `.gitraptor/policy.yaml` para que el formato lo decida un ADR. US-GRP-013 (umbral) y US-GRP-016 (rama base del equipo, diferida por Q36) están bloqueadas por esta decisión.

Restricciones: Rust (ADR-GRP-001), todo local y sin red (NFR-03), licencias permisivas (NFR-11) y frontera de solo lectura sobre el repo (BR-CONS-001).

## Decisión

**JSON estricto con `$schema`, al estilo de Claude Code**, en tres archivos con la misma estructura (propuesta base del BRD v0.4, aceptada por Rene Bonilla el 2026-10-03). Los nombres de los archivos y de la sección `engine` son la decisión PQ-4 (ver la tabla de PQ en el [índice](./index.md)).

### Archivos

| Nivel | Archivo | Versionado | Dónde |
|---|---|---|---|
| Perfil | `settings.json` | No | Carpeta de configuración del perfil (ADR-GRP-006) |
| Equipo | `.gitraptor/settings.json` | Sí, con el repo | **Objetos commiteados**, nunca el archivo en disco: el blob de la **copia de la rama principal** (el *suelo*) y el blob del `HEAD` del worktree de la operación, que solo endurece (decisión 1 de Guardrails, que sustituye a PQ-9; ver abajo) |
| Local | `settings.local.json` | No, por diseño | En el **perfil**, indexado por repo (ADR-GRP-008, PQ-3) |

### Estructura

Un solo documento JSON con secciones de primer nivel:

- `$schema`: URL del schema publicado. Sirve a los editores y el motor no la descarga nunca (NFR-03).
- `engine`: valores del motor. Los define este ADR.
- `permissions` (`allow` / `ask` / `deny`) y `policies`: su semántica es de Guardrails (ADR-GRD-003 § 1 y § 2, ADR-GRD-004 § 2). Sus tipos viven en `crates/policy`, llevan `x-gitraptor-levels` como las claves de `engine` y el motor no los interpreta (Enmienda 2026-10-04).

Claves de la sección `engine`:

| Clave | Tipo | Niveles admitidos | Por defecto | Regla |
|---|---|---|---|---|
| `engine.baseBranch` | string (nombre de rama) | Solo equipo | `"main"` | BR-CONS-006, Q24 |
| `engine.idleThresholdMinutes` | entero, de 1 a 1440 | Perfil y local | `5` | BR-TIME-001, Q20, Q24 |
| `engine.gitPath` | string (ruta absoluta al ejecutable de Git) | Solo perfil | sin valor (resolución automática) | ADR-GRP-009 § 4, Q28 |
| `engine.watcher.fallbackPollSeconds` | entero, de 5 a 3600 | Perfil y local | `30` | ADR-GRP-010 § 5 (sondeo de respaldo) |
| `engine.watcher.degradedPollSeconds` | entero, de 1 a 60 | Perfil y local | `2` | ADR-GRP-010 § 5 (modo degradado) |

Criterio de niveles de los valores nuevos:

- **`engine.gitPath`, solo perfil**: es una ruta de la máquina, y el motor resuelve un único Git para todos los repos (ADR-GRP-009). Un valor por repo (local) implicaría un Git distinto por repo, que el motor no admite; en el equipo sería una ruta de otra máquina.
- **Intervalos del watcher, perfil y local, nunca equipo**: son preferencias de coste de la máquina (CPU y disco). El nivel local permite ajustarlos para un repo concreto muy grande. En el equipo impondrían ese coste en las máquinas de todos.
- **`engine.gitPath` que no sirve** (no existe, no es ejecutable, es anterior a 2.38 o no supera la validación del ejecutable de ADR-GRP-009 § 4: absoluto, archivo regular, propiedad del usuario o de root y no escribible por grupo ni otros, SEC-10): no invalida el nivel. El candidato se descarta con diagnóstico y la resolución sigue con el resto de candidatos de ADR-GRP-009. Al cambiar el valor, el motor vuelve a resolver Git.

Secciones de Guardrails (Enmienda 2026-10-04; semántica en ADR-GRD-003 y ADR-GRD-004):

| Clave | Niveles admitidos (`x-gitraptor-levels`) | Por defecto | Regla |
|---|---|---|---|
| `permissions` (`allow` / `ask` / `deny`) | Equipo, perfil y local | Sin reglas | Solo el **suelo** relaja. El `HEAD` del worktree, el perfil y el local solo endurecen (D6, BR-CONS-001; ADR-GRD-004 § 2) |
| `policies` | Equipo, perfil y local | Sin políticas | Igual que `permissions` |
| Clave para desactivar el mínimo seguro | **Solo equipo, y solo desde el suelo** | Mínimo activo | ADR-GRD-003 § 2 y D6. En el `HEAD` del worktree o en un nivel personal no tiene efecto; tampoco con el suelo `ignorado` o `parcial` (D12) ni en modo degradado. El nombre y el tipo de la clave los fija el schema de TS-GRD-001 (decisión del Arquitecto, 2026-10-04) |

**Parámetros que no son configurables** (internos, fijados por su ADR): la ventana de debounce de 75 ms (forma parte del presupuesto de ADR-GRP-011; cambiarla exige revisar ese ADR), el intervalo de recomprobación de Git en "Esperando Git" (ADR-GRP-009), el intervalo de escaneo de procesos y la ventana Δ de atribución (ADR-GRP-012, los mide SPIKE-GRP-001), los tiempos del handshake y del arranque bajo demanda (ADR-GRP-005) y la persistencia del histograma de frescura (ADR-GRP-011). La variable de entorno de sobreescritura del perfil (ADR-GRP-006) tampoco es una clave de configuración: decide dónde están los archivos de configuración, así que no puede vivir dentro de ellos.

Ejemplos:

```jsonc
// Equipo: .gitraptor/settings.json commiteado (se lee del suelo y del HEAD del worktree)
{
  "$schema": "<URL del schema publicado>",
  "engine": { "baseBranch": "develop" },
  "permissions": { "deny": ["…"] },   // semántica: Guardrails (ADR-GRD-003)
  "policies": { }                      // semántica: Guardrails (ADR-GRD-003)
}
```

```jsonc
// Perfil: <config del perfil>/settings.json
{ "$schema": "<URL del schema publicado>", "engine": { "idleThresholdMinutes": 10 } }
```

```jsonc
// Local: <config del perfil>/repos/<id-repo>/settings.local.json  (ADR-GRP-008)
{ "$schema": "<URL del schema publicado>", "engine": { "idleThresholdMinutes": 15 } }
```

Los archivos son JSON estricto: no admiten comentarios. Los `//` de los ejemplos son solo explicativos.

### Schema

- Se **genera desde los tipos Rust** con `schemars` (MIT). Los tipos viven en `crates/policy`, que pasa a ser el dueño del documento de configuración completo: carga, precedencia, validación y diagnósticos. El motor (`crates/core`) consume la sección `engine` ya resuelta. Guardrails añade los tipos de `permissions` y `policies` en el mismo crate.
- Cada clave lleva la anotación propia `x-gitraptor-levels` (p. ej. `["team"]` o `["profile", "local"]`), que se genera desde un atributo en el tipo Rust. Es la fuente única de los niveles admitidos.
- El schema se **embebe en el binario** (`include_str!`) para validar sin red, se versiona en el repo y se publica en la URL de `$schema`. Un test de CI falla si el schema generado difiere del versionado.

### Precedencia

Valor efectivo de una clave = el del nivel **más específico, entre los que la admiten**, que la defina; si ninguno la define, el valor por defecto. El orden de especificidad es perfil < equipo < local (BR-CONS-007).

| Clave | Perfil | Equipo | Local | Resultado |
|---|---|---|---|---|
| `baseBranch` | ignorada, con diagnóstico | **la usa, solo desde el suelo** | ignorada, con diagnóstico | la rama base **confirmada**; la resuelta (suelo, o `main`) solo propone un cambio (ver "Qué configuración de equipo manda") |
| `idleThresholdMinutes` | la usa | ignorada, con diagnóstico | **la usa y gana** | local, si no perfil, si no `5` |
| `gitPath` | **la usa** | ignorada, con diagnóstico | ignorada, con diagnóstico | perfil, si no resolución automática |
| `watcher.fallbackPollSeconds` | la usa | ignorada, con diagnóstico | **la usa y gana** | local, si no perfil, si no `30` |
| `watcher.degradedPollSeconds` | la usa | ignorada, con diagnóstico | **la usa y gana** | local, si no perfil, si no `2` |

La regla de que un nivel personal no relaja una prohibición del equipo se aplica a `permissions` y `policies`. Es de Guardrails y este ADR no la redefine: la combinación (mínimo ∪ suelo, endurecido por el worktree, el perfil y el local) está en ADR-GRD-004 § 2 y la evaluación en ADR-GRD-003 § 1.

### Validación y diagnósticos

| Situación | Comportamiento |
|---|---|
| Archivo ausente | El nivel no aporta valores. No es un error. |
| JSON inválido | Se **ignora el nivel entero** y se emite un diagnóstico con archivo, línea y columna. Los demás niveles siguen aplicando (decisión de Rene Bonilla, 2026-10-03, PQ-8). |
| El documento no valida contra el schema (tipo o rango incorrecto) | Se **ignora el nivel entero**, con diagnóstico y la ruta JSON de la clave (PQ-8). |
| Clave desconocida | Se ignora la clave y se emite un **diagnóstico, no un error fatal**. El resto del nivel aplica. |
| Clave en un nivel que no la admite | Se ignora esa clave, con diagnóstico (Q24). El resto del nivel aplica. |
| Niveles personales: el archivo no es un archivo regular, es un enlace que sale de la carpeta de configuración del perfil o supera el tamaño máximo. Nivel de equipo: la entrada del árbol no es un **blob regular** (es un enlace o un submódulo) | Se **ignora el nivel o la fuente entera**, con diagnóstico **sin contenido** del archivo (SEC-11; ADR-GRD-004 § 2). |
| El documento supera los **límites del JSON** (L-03): tamaño máximo, profundidad máxima de anidamiento, número máximo de claves por objeto y de elementos por lista, longitud máxima de cadena. ⚠️ **ASSUMPTION** (ADR-GRD-004 § 1): 64 KiB, profundidad 16, 256 claves o elementos y 1 KiB por cadena | Se **ignora el nivel o la fuente entera**, sin agotar memoria y con diagnóstico sin contenido (Enmienda 2026-10-04). |
| `engine.baseBranch` no es un nombre de rama válido según `check-ref-format` o empieza por `-` | Se ignora la clave, con diagnóstico, y nunca se pasa a Git. Como el equipo sí declaró una rama base, el motor no recurre a `main`: indica que no puede calcular ahead/behind, igual que con una rama inexistente (Q42) (SEC-11). **Con una rama base confirmada** (decisión del Arquitecto, 2026-10-04): un suelo nuevo con una `baseBranch` inválida no cambia nada; el motor sigue calculando contra la confirmada y emite el diagnóstico `base-change-pending` marcado como inválido, y Guardrails protege la unión {confirmada, `main`, rama principal}. Q42 solo aplica cuando no hay ninguna rama base confirmada. |

**Por qué una clave desconocida no es fatal**: el archivo de equipo viaja con el repo y lo leen binarios de versiones distintas en las máquinas del equipo. Si una clave nueva invalidara el nivel en un binario antiguo, ese binario perdería la rama base y los demás valores del equipo. Además Guardrails y el motor amplían el documento por separado. El diagnóstico detecta las erratas sin romper la compatibilidad hacia adelante. Por eso el schema no declara `additionalProperties: false`, y el cargador compara las claves contra el schema para avisar.

**Estado por fuente del cargador** (Enmienda 2026-10-04; ADR-GRD-004 § 1). Un único cargador en `crates/policy` devuelve, por cada fuente (perfil, local, suelo y `HEAD` del worktree), uno de estos estados. Es un solo criterio de validez, con reacciones distintas por consumidor:

| Estado | Cuándo | Motor | Guardrails |
|---|---|---|---|
| `ausente` | Archivo ausente; en el equipo, sin copia de la rama principal, `HEAD` sin nacer o sin la ruta | La fuente no aporta valores | Ídem; el mínimo sigue activo |
| `legible` | El documento valida. Lleva los diagnósticos de claves desconocidas o fuera de nivel | Aplica lo legible (tabla de arriba) | Aplica lo legible |
| `ignorado` | JSON inválido, schema inválido, límites superados o entrada no regular (PQ-8) | Ignora la fuente entera (PQ-8, sin cambio) | Aplica el mínimo, que no se puede desactivar mientras el suelo esté `ignorado`, más lo `legible`, y avisa (BR-EDGE-004, Q-GRD-12) |
| `parcial` | Claves desconocidas dentro de `permissions` o `policies` (**D12**, Rene Bonilla, 2026-10-04; ADR-GRD-004 § 1) | Sin efecto: el motor no lee esas secciones | Aplica lo legible, **fuerza el mínimo seguro aunque el equipo lo hubiera desactivado** y avisa |

Los diagnósticos de todos los estados nunca llevan contenido (SEC-11).

Los diagnósticos se exponen a los clientes por el canal local (ADR-GRP-005) como una lista por repo con tipo, archivo, posición o ruta JSON, y nivel. **Nunca incluyen fragmentos del contenido** del archivo ni valores leídos: `.gitraptor/settings.json` viaja con el repo y lo puede escribir un agente o un PR, y podría apuntar a un secreto (SEC-11). El texto que ve el usuario lo traduce el cliente (i18n en/es). Un diagnóstico nunca detiene la observación.

### Qué configuración de equipo manda (PQ-9, sustituida el 2026-10-04)

**Decisión original (PQ-9, Rene Bonilla, 2026-10-03), sustituida y conservada como registro**: "Cada worktree puede tener en su rama una versión distinta de `.gitraptor/settings.json`. Manda la del **worktree principal** del repo, leída del archivo en disco. Así hay un único valor de repo, como la rama base, para todos los worktrees. Las versiones de los worktrees enlazados se ignoran para los valores del motor. Si el repo no tiene worktree principal con árbol de trabajo (repo bare), el nivel de equipo no aporta valores y se emite un diagnóstico. Coordinar con el context de Guardrails, que puede necesitar la misma regla para sus políticas."

**Decisión vigente (decisión 1 de Rene Bonilla, 2026-10-04, Guardrails; ADR-GRD-004)**. PQ-9 dejaba relajar las reglas con una edición sin commitear (contradice Q-GRD-17 y Q-GRD-18). La decisión 1 la resuelve a su favor:

- **Lo commiteado**: el nivel de equipo se lee de **objetos commiteados**, nunca del archivo en disco. Una edición o un conflicto en el working tree no cambian nada y no dan aviso de ilegible.
- **Dos fuentes del nivel de equipo** (ADR-GRD-004 § 1 y § 2):
  - **Suelo**: el blob `.gitraptor/settings.json` de la **copia de la rama principal**. Es la **única** fuente de `engine.baseBranch` y de toda relajación de `permissions` y `policies`, incluida la clave para desactivar el mínimo (D6).
  - **Worktree**: el blob del commit al que apunta el `HEAD` del worktree de la operación (Q-GRD-17). **Solo endurece** (D6): no relaja, no desactiva el mínimo y no cambia la rama base. Para el motor no aporta nada, porque la única clave de `engine` que admite el equipo es `baseBranch`. Casos límite (`HEAD` sin nacer, separado, rebase en curso) en ADR-GRD-004 § 2.
- **Copia de la rama principal, sin `fetch`** (decisión 2; ADR-GRD-004 § 3):
  1. **Remoto**: `origin` si hay varios; el único que haya si hay uno solo; si hay varios y ninguno es `origin`, no hay remoto.
  2. **Rama principal**: el destino de `refs/remotes/<remoto>/HEAD`, si existe; si no, `main`.
  3. **Copia**, la primera que exista: `refs/remotes/<remoto>/<principal>` (la copia conocida, sin red), `refs/heads/<principal>` (la rama local) o ninguna.
  4. **Rama base resuelta**: `engine.baseBranch` del suelo legible; si no, `main`.
- **Rama base confirmada** (D6, D7 y D8; ADR-GRD-004 § 3 y § 4): el daemon guarda en el **almacén por repo** del perfil la **rama base confirmada** y el **suelo confirmado** (ADR-GRP-006 § 4). La confirmada es el **único valor de rama base** del repo: contra ella calcula el motor el ahead/behind (US-GRP-016) y es la que protege Guardrails.
  - **La primera confirmación siempre es del humano**, con un comando reservado de Guardrails (ADR-GRP-005 § 6, ADR-GRD-007), solo en dos momentos (**D9**, Rene Bonilla, 2026-10-04): al instalar la protección (US-GRD-001), que confirma `main` **sin leer el suelo**, no relaja y no tiene ventana; o con un comando explícito (US-GRD-014), que lee el suelo y pasa por D5 (anuncio, ventana cancelable y auditoría completa) solo si el suelo trae relajaciones. **Añadir el repo a la observación no confirma nada.** Mientras no hay confirmación inicial, Guardrails expone el diagnóstico `base-unconfirmed` y protege la unión de ramas base en las dos fases de ADR-GRD-004 § 3.5, y el motor muestra la rama base como "no confirmada" y calcula el ahead/behind contra la resuelta, **marcado como no confirmado**.
  - **Si la resuelta difiere de la confirmada**, el motor sigue con la confirmada y la resuelta es solo el diagnóstico `base-change-pending`, con aviso. Solo el humano confirma el cambio, y entonces la resuelta pasa a confirmada. La unión {confirmada, resuelta} es un refuerzo propio de Guardrails; el motor no la usa.
  - **Si el suelo nuevo relaja algo** frente al confirmado, rige la combinación más restrictiva de los dos, con el diagnóstico `floor-relax-pending`, hasta que el humano lo confirma (D7).
- **Lecturas sin objetos de reemplazo** (H-05): los blobs y los árboles se leen con los objetos de reemplazo (`refs/replace/*`) desactivados y sin grafts, en el daemon y en el modo degradado de Guardrails.
- **Repo bare**: el caso que PQ-9 trataba aparte desaparece, porque el suelo se lee de objetos y no de un árbol de trabajo.
- **Sin red ni escrituras** (Q12 de motor-local, NFR-03, ADR-GRP-009).

### Lectura y recarga

El motor abre los archivos de los niveles personales **solo para leer**, nunca los crea, y vigila sus cambios con el observador de ADR-GRP-010. El nivel de equipo se vuelve a leer cuando cambia una ref de la copia de la rama principal o el `HEAD` de un worktree; el observador ya vigila `.git/refs/`, `packed-refs` y los `HEAD` (ADR-GRP-010 § 2). Los documentos parseados se guardan en una caché LRU acotada, indexada por el identificador del blob (ADR-GRD-004 § 2). Al cambiar una fuente, el cargador reaplica la precedencia sin reiniciar el motor (BR-CONS-007).

## Alternativas consideradas

- **TOML**: admite comentarios y es idiomático en Rust, pero no tiene `$schema` en línea y no comparte el modelo mental de `.claude/settings.json`, que es la referencia del usuario objetivo. Descartada.
- **YAML (`.gitraptor/policy.yaml`, BRD ≤ v0.3)**: tipado ambiguo (el "problema de Noruega") y el crate `serde_yaml` está sin mantenimiento. Además, un solo archivo de equipo no cubre los tres niveles. Descartada; el BRD v0.4 ya la retiró.
- **JSONC o JSON5**: admiten comentarios, pero el soporte de editores y validadores es desigual y se aleja de Claude Code. Descartada. Se puede revisar si los usuarios piden comentarios.
- **Nivel local en el repo + `.gitignore`**, o **en `.git/info/exclude`**: ver ADR-GRP-008. Se descartan porque el motor no escribe en el repo (Q21) y no puede garantizar que el archivo no se versione.
- **Clave desconocida o schema inválido como error fatal**: rompe la compatibilidad entre versiones del binario y dejaría de observar por una errata, en contra del ASSUMPTION de BR-CONS-007 y de PQ-8.

## Consecuencias

- ✅ Un modelo familiar para quien ya usa `.claude/settings.json`, con validación y autocompletado en el editor gracias a `$schema`.
- ✅ Una sola fuente de verdad para los niveles admitidos: el tipo Rust genera el schema y la anotación `x-gitraptor-levels`.
- ✅ Desbloquea US-GRP-013 y, con la coautoría cerrada con ADR-GRD-003/004, US-GRP-016 (que calcula contra la rama base confirmada).
- ✅ El motor sigue observando con cualquier configuración, válida o no.
- ✅ Un solo cargador y un solo criterio de validez para el motor y Guardrails, con reacciones por consumidor (R-GRD-8; Enmienda 2026-10-04).
- ✅ Ni una edición sin commitear, ni un checkout antiguo, ni un commit laxo en la rama de un agente, ni una ref forjada, ni un objeto de reemplazo cambian la rama base del motor (decisión 1, D6, D7; ADR-GRD-004).
- ⚠️ Un nivel de equipo `ignorado` deja sin efecto sus `permissions` y `policies`. **Mitigación (decidida en ADR-GRD-004 § 1)**: Guardrails aplica el mínimo, que no se puede desactivar mientras el suelo esté `ignorado`, más lo legible, y avisa. El diagnóstico es visible en todos los clientes.
- ⚠️ JSON estricto no admite comentarios. **Mitigación:** las descripciones del schema se ven en el editor; se puede reconsiderar JSONC más adelante sin cambiar la estructura.
- ⚠️ Un `engine.baseBranch` cambiado en la rama de un worktree no tiene efecto (D6), y uno que llega a la copia de la rama principal no tiene efecto hasta que el humano lo confirma (Enmienda 2026-10-04). **Mitigación:** los diagnósticos `base-change-pending`, `base-unconfirmed` y `floor-relax-pending` (D11; ADR-GRD-005 § 1), con aviso en todos los clientes y la acción para confirmar. US-GRP-016 y BR-CONS-006 de motor-local ya calculan contra la rama base confirmada (Q-GRD-21).
- ⚠️ Cada relajación del equipo que llega a la rama principal exige una confirmación humana por máquina (D7). Es el coste de un suelo no forjable (ADR-GRD-004 § 4).
- ⚠️ Esta decisión obliga a actualizar ADR-GRP-002 (`crates/policy`) y ADR-GRP-004 (editor de configuración), que citaban `policy.yaml`. Hecho el 2026-10-03.

Nota de integración (Time Machine, ADR-TMC-007 y SEC-TMC-06/12, aceptados el 2026-10-03): nueva sección `timeMachine`, con tipos en `crates/policy`. Claves: `timeMachine.retentionDays` (entero de 1 a 3650; niveles perfil y local; por defecto 30); `timeMachine.storeQuotaGB` (entero ≥ 1; solo perfil; por defecto 20); `timeMachine.includeCredentialFiles` (booleano; solo perfil; por defecto false). En el nivel de equipo, las tres se ignoran con diagnóstico.

## Validación

Tests de `crates/policy`, siempre con repos y perfiles temporales (variable de entorno de sobreescritura del perfil, ADR-GRP-006), nunca con este repo:

1. **Precedencia**: un caso por fila de las tablas de BR-CONS-006 y BR-CONS-007 (p. ej. perfil 5 + local 15 → 15; umbral en el equipo → ignorado; `baseBranch` en local → ignorado).
2. **JSON inválido** en cada nivel: se ignora solo ese nivel, los demás aplican y el diagnóstico trae archivo, línea y columna.
3. **Schema inválido** (tipo o rango): se ignora el nivel entero y el diagnóstico trae la ruta JSON.
4. **Clave desconocida** y **clave fuera de nivel**: el nivel aplica y se emite el diagnóstico.
5. **Valores nuevos**: `gitPath` en local o equipo → ignorado con diagnóstico; `gitPath` inexistente en el perfil → diagnóstico y resolución automática (ADR-GRP-009); intervalos del watcher en el equipo → ignorados; local gana al perfil; un intervalo fuera de rango invalida el nivel (PQ-8).
6. **Rama base del equipo (decisión 1 de Guardrails; sustituye a la validación de PQ-9)**:
   - Un commit en la rama de un worktree enlazado que cambia `baseBranch` no cambia nada (D6). Una edición sin commitear en cualquier worktree tampoco.
   - Rama principal: con `origin/HEAD → origin/trunk` se usa `origin/trunk`; sin remoto, `refs/heads/main`; con varios remotos sin `origin`, la rama local; sin nada, `main`. Un repo bare con la copia de la rama principal se lee igual.
   - Sin confirmación inicial, el ahead/behind se calcula contra la resuelta, marcado como no confirmado.
   - Con el suelo cambiando `baseBranch` de `main` a `develop`: hasta la confirmación, el motor calcula contra `main` y expone `base-change-pending`; tras confirmar, contra `develop`.
   - El motor y Guardrails obtienen la misma rama base confirmada del mismo cargador (ADR-GRD-004, Validación 11).
7. **Recarga**: editar un archivo personal cambia el valor efectivo sin reiniciar el motor. Un commit que mueve la copia de la rama principal vuelve a leer el suelo.
8. **Archivos hostiles (SEC-11)**: `.gitraptor/settings.json` commiteado como enlace (a `~/.ssh/id_rsa`) o como submódulo → fuente ignorada y diagnóstico sin contenido; en un nivel personal, FIFO o archivo de tamaño excesivo → ignorado sin bloquear; `baseBranch` `--upload-pack=x` → ignorada; `engine.gitPath` relativa o escribible por otros → descartada (SEC-10).
9. **Solo lectura**: tras cargar y recargar, los hashes y las fechas de modificación de los tres archivos y el estado observable del repo no cambian (BR-CONS-001).
10. **Deriva del schema**: el schema generado es igual al versionado y al embebido (CI).
11. **Límites del JSON (L-03)**: un documento con profundidad 1.000, un millón de claves o una cadena de 10 MB da `ignorado` sin agotar memoria (ADR-GRD-004, Validación 9).
12. **Objetos de reemplazo (H-05)**: `git replace <blob del suelo> <blob laxo>` y un graft no cambian el documento leído (ADR-GRD-004, Validación 8).
13. **Forja (J3)**: `git update-ref refs/remotes/origin/main <commit con otra baseBranch>` no cambia la rama base del motor; aparece `base-change-pending` (ADR-GRD-004, Validación 7).
14. **Estado por fuente**: un caso por estado (`ausente`, `legible`, `ignorado`, `parcial`) con la reacción del motor de la tabla de "Validación y diagnósticos".
15. **`baseBranch` inválida con rama base confirmada**: con `main` confirmada, un suelo nuevo con `baseBranch` `--x` deja el ahead/behind contra `main` y emite `base-change-pending` marcado como inválido. Sin ninguna confirmada, aplica Q42.
16. **Añadir un repo** (D9): tras añadirlo, la rama base sigue "no confirmada" hasta instalar la protección o confirmar de forma explícita.

## Referencias

- [BRD-GRP-001](../../business/gitraptor-documento-de-negocio.md): BR-11, changelog v0.4, NFR-03, NFR-11.
- [Contexto `motor-local`](../../requirements/features/motor-local/context.md): Q3, Q20, Q23, Q24, Q27, Q31, Q36; P8.
- [Reglas de negocio `motor-local`](../../requirements/features/motor-local/business-rules.md): BR-CONS-001, BR-CONS-006, BR-CONS-007, BR-TIME-001.
- Historias: US-GRP-012, US-GRP-013, US-GRP-016.
- ADRs: [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md), [ADR-GRP-002](./ADR-GRP-002-monorepo-nx.md), [ADR-GRP-004](./ADR-GRP-004-estado-frontend-ux.md), ADR-GRP-005, ADR-GRP-006, [ADR-GRP-008](./ADR-GRP-008-configuracion-local-no-versionada.md), ADR-GRP-010.
- Decisiones de Rene Bonilla, 2026-10-03: propuesta base del BRD v0.4, PQ-3, PQ-4, PQ-8, PQ-9 (sustituida el 2026-10-04).
- Decisiones de Rene Bonilla, 2026-10-04 (Guardrails): decisiones 1 y 2, D6, D7, D8, D9, D11 y D12.
- Guardrails: [ADR-GRD-003](./ADR-GRD-003-motor-decision-contrato.md) (evaluación y mínimo seguro), [ADR-GRD-004](./ADR-GRD-004-configuracion-efectiva.md) (configuración efectiva), [ADR-GRD-007](./ADR-GRD-007-acciones-reservadas-excepcion.md) (confirmaciones); TS-GRD-001; Q-GRD-12, Q-GRD-17, Q-GRD-18, BR-EDGE-004, R-GRD-8.
- [Claude Code — Settings](https://docs.anthropic.com/en/docs/claude-code/settings) (modelo de referencia), [`schemars`](https://crates.io/crates/schemars).

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia formato, niveles ni precedencia.

| Hallazgo | Cómo se cubre |
|---|---|
| M10 · `.gitraptor/settings.json` y la ruta explícita de Git sin validar | Validación y diagnósticos: solo archivo regular, sin enlaces que salgan del worktree, tope de tamaño, diagnósticos sin contenido, `baseBranch` validada como ref (SEC-11); `engine.gitPath` validada según ADR-GRP-009 § 4 (SEC-10) |

Validación ampliada: SEC-11 y SEC-10 (punto 8).

## Enmienda (2026-10-04, Guardrails)

Aplicada desde la tabla de enmiendas de [non-functional-guardrails.md](../non-functional-guardrails.md) (J10). Fuentes: ADR-GRD-003, ADR-GRD-004, TS-GRD-001 y las decisiones de Rene Bonilla del 2026-10-04. No cambia el formato, los nombres de archivo, la sección `engine` ni PQ-8. El `status` sigue en `proposed`.

| Cambio | Dónde | Fuente |
|---|---|---|
| **PQ-9 sustituida** (no borrada) por la decisión 1: el nivel de equipo se lee de lo commiteado; la rama base y toda relajación salen solo de la copia de la rama principal (sin `fetch`; luego la rama local; luego `main`); el `HEAD` del worktree solo endurece; la rama base efectiva es la confirmada por el humano | Tabla de archivos, "Qué configuración de equipo manda", precedencia, Lectura y recarga, Consecuencias, Validación 6 y 7 | Decisiones 1 y 2, D6, D7, D8; ADR-GRD-004 § 2 a § 4 |
| `permissions` y `policies` con `x-gitraptor-levels`; la clave para desactivar el mínimo, solo en el suelo | Estructura | ADR-GRD-003 § 1 y § 2, ADR-GRD-004 § 2 |
| Estado por fuente del cargador: `ausente`, `legible`, `ignorado`, `parcial` | Validación y diagnósticos | ADR-GRD-004 § 1 |
| Límites del JSON; blob regular en el nivel de equipo | Validación y diagnósticos; Validación 8 y 11 | ADR-GRD-004 § 1 y § 2 (L-03, SEC-11) |
| Lecturas sin objetos de reemplazo ni grafts | "Qué configuración de equipo manda"; Validación 12 | ADR-GRD-004 § 1 (H-05) |
| Nota de cabecera: "Coautoría cerrada con ADR-GRD-003/004" | Cabecera | ADR-GRD-003, ADR-GRD-004 |

**Ronda de coherencia (2026-10-04)**:

| Cambio | Dónde | Fuente |
|---|---|---|
| `parcial` decidido, sin ⚠️: se aplica lo legible, se fuerza el mínimo aunque el equipo lo hubiera desactivado y se avisa | Estado por fuente; tabla de Guardrails | **D12** (Rene Bonilla, 2026-10-04) |
| La confirmación inicial, nunca al añadir el repo: al instalar (US-GRD-001), `main` sin leer el suelo, sin relajar y sin ventana; con un comando explícito (US-GRD-014), D5 si el suelo relaja | "Qué configuración de equipo manda" | **D9** (Rene Bonilla, 2026-10-04) |
| `baseBranch` inválida en un suelo nuevo: se mantiene la confirmada, `base-change-pending` inválido, unión {confirmada, `main`, rama principal} en Guardrails; Q42 solo sin confirmada. Cierra el punto que quedó pendiente de decidir | Validación y diagnósticos | Decisión del Arquitecto (2026-10-04) |
| El nombre y el tipo de la clave del mínimo los fija el schema de TS-GRD-001 | Tabla de Guardrails | Decisión del Arquitecto (2026-10-04) |
| La rama base y el suelo confirmados viven en el almacén por repo | "Qué configuración de equipo manda" | Decisión del Arquitecto (2026-10-04); ADR-GRP-006 § 4 |
| **Corrección tras el Judge (2026-10-04)**: confirmación inicial de US-GRD-001 sin relajar y sin ventana, D5 solo en la explícita de US-GRD-014; los tres diagnósticos de D11 nombrados, siempre como diagnóstico; nota del PO resuelta (Q-GRD-21); D9 y D12 en Referencias | "Qué configuración de equipo manda"; Consecuencias; Referencias | ADR-GRD-004 § 3.5, ADR-GRD-005 § 1 |
