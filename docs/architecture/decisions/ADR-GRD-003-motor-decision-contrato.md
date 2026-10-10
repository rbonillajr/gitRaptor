---
id: ADR-GRD-003
title: Motor de decisión, mínimo seguro y contrato de decisión (hooks hoy, MCP como interfaz)
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-06
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRP-002, ADR-GRD-001, ADR-GRD-002, ADR-GRD-004, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007, ADR-CKP-002, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, motor-decision, crates-policy, contrato, mcp, minimo-seguro, actor, fail-safe, br-calc-001, br-cons-002, modo-degradado, canal-autenticado]
---

# ADR-GRD-003 — Motor de decisión, mínimo seguro y contrato de decisión

> **Estado**: aceptado por Rene Bonilla el 2026-10-04. Enmendado el 2026-10-05 por US-GRD-001 (ver "Enmienda (2026-10-05, US-GRD-001)").

## Contexto

Para cada operación gobernada Guardrails decide permitir, pedir confirmación o denegar. Las reglas que fijan esa decisión son:

- **BR-CALC-001**: gana la más restrictiva de las reglas que aplican y la decisión nombra todas las reglas que la causan, con su nivel.
- **BR-CONS-002**: la decisión es la misma por MCP y por hooks.
- **BR-AUTH-003, Q-GRD-1**: las reglas se aplican igual a un "agente X" que a "sin atribuir".
- **S-GRD-9**: "pedir confirmación" se trata como "denegar" mientras no exista la cola.
- **BR-EDGE-001, Q-GRD-5**: sin configuración aplica un mínimo seguro (denegar force-push y el borrado de la rama base), visible y desactivable por el equipo. Por D6 (2026-10-04), solo se desactiva desde la rama principal (ADR-GRD-004).

Las herramientas MCP son de F-001-05. Aquí solo se fija la interfaz que consumen (BR-AUTH-004). El actor lo emite el motor (Q34 y Q35 de motor-local; ADR-GRP-013 § 6, aceptado).

La decisión tiene que ser explicable (contexto § 6), i18n en/es (NFR-10), < 100 ms (ADR-GRD-002 § 5) y fail-safe ante cualquier duda. El hook corre en el entorno del agente, así que ni el canal ni el entorno son de fiar (revisión de seguridad, H-03).

## Decisión

**La evaluación es una función pura y determinista en `crates/policy`. El daemon la aloja y la expone por el canal local (`crates/api`); el cliente del hook solo habla con un daemon autenticado, por una ruta fijada al instalar. El actor y la identidad de la operación salen del `git` antecesor más cercano al hook. Si no hay daemon, un modo degradado más estricto evalúa sin leer el perfil.**

### 1. Función de evaluación (`crates/policy`)

Sin E/S, sin reloj y sin aleatoriedad:

`evaluar(operación normalizada, contexto, configuración efectiva) → Decisión`.

- **Operación normalizada**: una del catálogo de BR-VAL-002 con sus **transiciones exactas** (ref normalizada, valor viejo y valor nuevo; ADR-GRD-002 § 4), el remoto y, si una política activa lo pide, los hechos de contenido.
  - **Nombre del remoto** (M-06): si es una URL, se guarda sin `userinfo`, o como `<url>` si no se puede limpiar.
  - **Lo que nunca guarda**: argv.
- **Contexto**: repo (clave de ADR-GRP-006), worktree, rama o ramas base protegidas (ADR-GRD-004 § 3), capa (`hooks` o `mcp`) y actor. (Enmienda 2026-10-04, Cockpit: también `cockpit`; ver la sección final.)
- **Configuración efectiva**: la de ADR-GRD-004, por fuentes, cada una con su estado: `mínimo`, `suelo` (la rama principal), `worktree` (el `HEAD` de la operación, que solo endurece) y los niveles personales.
- **Combinación**:
  - Primero, el suelo y el mínimo. Después, lo que endurecen el worktree y los niveles personales (BR-CONS-001, D6).
  - Después, `decisión = máx(reglas)` con `denegar > pedir confirmación > permitir`.
  - Las razones son todas las reglas que producen el máximo (BR-CALC-001).
- **Coste acotado** (L-03):
  - Las expresiones del formato de commit usan un motor de **tiempo lineal** con tope de tamaño.
  - Los globs de rutas se evalúan en tiempo lineal.
  - Los límites del documento JSON están en ADR-GRD-004 § 1.
- **El actor no cambia la decisión** (Q-GRD-1). Solo entra en el registro y en las excepciones. (Enmienda 2026-10-06, US-GRD-018: salvo las reglas de autoría de BR-AUTH-005, que solo endurecen y solo con un actor agente; ver la sección final.) (Enmienda 2026-10-08, US-GRD-008: también las reglas `policy.protected-branch` y `policy.forbidden-path`, que solo añaden denegaciones; ver la sección final.)

### 2. Mínimo seguro (BR-EDGE-001, Q-GRD-5)

| Regla | Código | Efecto | Ámbito |
|---|---|---|---|
| Prohibir force-push | `minimum.force-push` | Denegar | Cualquier rama, cualquier remoto |
| Proteger la rama base | `minimum.base-branch-delete` | Denegar | Borrar la rama base, local o en cualquier remoto. Si hay un cambio de rama base pendiente de confirmar (D6), aplica a la **unión** {anterior, nueva} |

- **Activo** salvo que el **suelo** (configuración del equipo en la rama principal, D6) lo desactive. La clave está en ADR-GRP-007 (enmienda aplicada el 2026-10-04). Su nombre y su tipo los fija el schema de TS-GRD-001 (decisión del Arquitecto, 2026-10-04).
- **No pueden desactivarlo** ni un nivel personal ni el `HEAD` del worktree, ni el suelo cuando es ilegible o `parcial` (D12), ni el sistema en modo degradado.
- **Visible** en el estado de protección (US-GRD-004, ADR-GRD-005).
- **Rama base**: hasta TS-GRD-001, en repos sin configuración del equipo es `main`, confirmada al instalar (BR-CONS-003, US-GRP-012); con configuración, `base-unconfirmed` y la unión {`main`, rama principal} protegida (ADR-GRD-004 § 3.5, fase 1).

### 3. Decisión (contrato en `crates/api`)

| Campo | Contenido |
|---|---|
| `decisionId` | Identificador opaco |
| `effect` | `allow` \| `ask` \| `deny` |
| `appliedEffect` | Lo que aplica la capa. Sin cola, `ask` se aplica como `deny` con `ask-unavailable` (S-GRD-9) |
| `reasons[]` | `{rule, level: minimum\|floor\|worktree\|profile\|local\|system, params}`. `system` cubre `config-unreadable`, `degraded`, `snapshot-failed` y `ask-unavailable`. `params` son **datos etiquetados y delimitados** (p. ej. `{kind: branch, value}`), marcados como no confiables (SEC-12 de motor-local) |
| `exception` | `none` \| `applied` \| `rejected` (ADR-GRD-007) |
| `configStatus[]` | Por fuente: `ok` \| `absent` \| `unreadable` \| `partial` \| `pending-confirmation`, con códigos sin contenido del archivo |
| `configRef` | Identificadores de los blobs evaluados |

- **Mensaje al agente** (M-05; SEC-GRD-06): el cliente del hook escribe en la salida de error una **plantilla fija por código**.
  - **Parámetros**: como datos delimitados y etiquetados (`rama=«…»`).
  - **Saneado**: se neutralizan las categorías Unicode Cc, Cf, Zl y Zp (escapes ANSI/OSC, controles bidi de tipo Trojan Source y separadores de línea). Longitud acotada.
  - **Lo que nunca lleva**: mensajes de commit, contenido, instrucciones para desactivar la protección, ni menciones de `raptor guard exec` u otra vía de excepción.
- **Versionado** con el protocolo del canal (handshake de ADR-GRP-005 § 5).
- **Códigos de motivo del hook** (Enmienda 2026-10-04, SPIKE-GRD-001): `historia-superficial` (el push se trata como forzado porque el clon es superficial; la plantilla puede sugerir `git fetch --unshallow`, que no desactiva la protección) y `renombrado-sobre-base` (con el backend de archivos, `branch -M x base` se deniega después de que Git ya borrara `x`; `params` lleva el oid que iba a escribirse para recuperar la rama origen). Recuperar no es una vía de excepción: SEC-GRD-06 lo permite.

### 4. Dónde se evalúa

- **Canal autenticado** (H-03; M-07; SEC-GRD-16; Judge ronda 2, hallazgos 1 y 3):
  - **Ruta del canal y id del directorio común**: son **constantes del dispatcher** (ADR-GRD-001 § 2). El cliente del hook no las deriva del entorno (ni `XDG_RUNTIME_DIR` ni `HOME`, que además no recibe) ni las lee del diario o del perfil. Si el perfil se borra o `HOME` es falso, el hook sigue llegando al daemon real.
  - **Servidor = binario instalado**: antes de enviar nada, el cliente toma el pid del par por las credenciales del socket (en Windows, el servidor del named pipe), obtiene la ruta del ejecutable y comprueba la firma, o la huella fijada en Linux, contra la del propio cliente. Si falla: **deny en refs gobernadas**, causa `channel-not-authentic`.
  - **Servidor = esta instancia**: en el handshake el daemon presenta el **id de la instancia de su perfil**, y el cliente lo compara con la constante del dispatcher. Si no coinciden (un daemon auténtico arrancado con otro perfil, o un perfil recreado tras borrarlo), el cliente **no usa esa decisión** y pasa al **modo degradado**, causa `instance-mismatch`. El modo degradado es más estricto, así que un perfil ajeno no da ninguna ventaja. Una instalación huérfana sigue protegida así hasta que se adopta (ADR-GRD-005 § 1).
  - **Residuo declarado**: una **copia completa del perfil real**, con su id de instancia, permitiría arrancar un daemon auténtico que pase la comprobación. No da ventaja, porque copia también el suelo confirmado, la rama base confirmada y la lista de repos protegidos, y por tanto decide igual. Para cambiar las decisiones habría que editar el perfil, cosa que un proceso del mismo usuario ya puede hacer sobre el perfil real. Es el residuo aceptado de ADR-GRP-006 (el perfil no es una frontera frente al mismo usuario).
  - **Arranque bajo demanda desde un hook**: la biblioteca cliente arranca el daemon con entorno limpio (ADR-GRP-005 § 3). Si el daemon arrancado presenta otro id de instancia, se aplica el modo degradado.
- **En el daemon** (módulo `guardrails` de `crates/core`):
  - **Directorio común** (M-02; SEC-GRD-19): el cliente envía el **identificador del directorio común** fijado en el dispatcher. El daemon exige que el cwd del proceso del hook, que lee con el identificador no reutilizable del canal (ADR-GRP-005 § 6), pertenezca a un worktree de **ese** directorio común. Si no, **deny** en refs gobernadas. Nunca es un error que caiga a la vía rápida.
  - **Configuración**: la lee el propio daemon (ADR-GRD-004), sin `GIT_*` ni contenido del cliente.
- **Identidad de la operación y actor** (H-02):
  - **De dónde sale la identidad**: la operación se identifica por el **`git` antecesor más cercano** al proceso del hook. Su identidad es `(pid, hora de inicio)` o pidfd, el audit token en macOS y el handle con hora de creación en Windows, y se verifica en **cada** antecesor recorrido.
  - **Actor**, desde ese `git` hacia arriba:
    - un proceso de agente detectado (ADR-GRP-012, S1) → "agente X (detectado)";
    - si no, las reglas de ADR-GRP-013 § 3 (registro explícito) → "agente X (registrado)";
    - si no → "sin atribuir".

    Es la señal S4 de ADR-GRP-012, opcional para el motor.
- **Ejecutor del daemon** (Cockpit y MCP, ADR-TMC-002 § 5): si el **padre directo** del `git` más cercano es el ejecutor, con una operación registrada para esa identidad, el daemon devuelve la decisión ya tomada **para cada transición registrada**. Cualquier transición distinta se evalúa de nuevo. (Enmienda 2026-10-04, Cockpit: registro del hijo con la capa de la petición, sin `previo_hook` y sin esperar al cerrojo del repo; ver la sección final.)
- **Escrituras internas de la Time Machine**: desactivan los hooks (ADR-TMC-002 § 2) y no pasan por Guardrails. Su autorización es de ADR-TMC-005.
- **Modo degradado** (daemon no arrancable, o daemon auténtico de otra instancia; H-03; J7). Un servidor que **no** es el binario instalado no entra aquí: da deny en refs gobernadas. El cliente evalúa con el mismo crate con estas reglas, todas más estrictas que el modo normal:
  - **Mínimo forzado**: nadie lo puede desactivar.
  - **Equipo que solo endurece**: se lee el `HEAD` del worktree y la copia de la rama principal, y ninguno de los dos relaja.
  - **Sin niveles personales**: no se leen; no se abre el perfil para configuración.
  - **Rama base protegida**: la unión {`main`, la rama principal, la última rama base confirmada, la rama base resuelta del suelo}.
    - **La resuelta** es el `engine.baseBranch` que el cliente lee de la copia de la rama principal. Esa copia ya la lee para el endurecimiento del equipo.
    - **La última confirmada** sale de la instantánea de solo lectura que el daemon deja en el directorio de estado del perfil, cuya ruta es una constante del dispatcher. La excepción para leerla desde un cliente está **aplicada (2026-10-04)** en ADR-GRP-005 § 1 y ADR-GRP-006 § 4. El valor de origen, la rama base confirmada, se guarda en el almacén por repo (ADR-GRP-006 § 4); la instantánea es una copia de solo lectura que el daemon exporta desde allí, y el cliente nunca abre el SQLite (un lector en modo WAL escribiría en el `-shm`). Si la instantánea falta, la unión se queda sin ella.
    - **Así el modo degradado nunca protege menos ramas base que el modo normal**, que protege la confirmada más la resuelta mientras hay un cambio pendiente (ADR-GRD-004 § 3).
  - **Lecturas aisladas**: `gix` aislado del entorno (sin `GIT_*`, sin configuración global ni de sistema) y sin objetos de reemplazo.
  - **Sin excepciones**: no hay excepción posible y el actor es "sin atribuir".
  - **Registro**: la entrada va al spool (ADR-GRD-006 § 4).
  - **Trazabilidad**: el estado registra la causa (`daemon-unreachable`, `instance-mismatch` o, para el deny, `channel-not-authentic`) con aviso, y el daemon guarda la **ventana degradada** (inicio y fin) cuando vuelve, no solo las entradas del spool (ADR-GRD-005, ADR-GRD-006).
  - **Garantía**: el modo degradado nunca es menos restrictivo que el mínimo más el suelo legible. Solo pierde los endurecimientos personales, las excepciones y las reglas de autoría, porque el actor es "sin atribuir" (Enmienda 2026-10-06, US-GRD-018). Tampoco aplica las reglas `agents` de ramas protegidas y rutas prohibidas, pero sí las `everyone` del suelo (Enmienda 2026-10-08, US-GRD-008).

### 5. Contrato para F-001-05 (solo interfaz)

- **Método**: el mismo de evaluación, con la capa `mcp`.
  - **Primero, la decisión**: el MCP evalúa antes de pedir la operación protegida (ADR-TMC-004 § 1).
  - **Después, la ejecución**: si `appliedEffect = allow`, la pide; si no, devuelve la decisión estructurada (§ 3).
- **Garantías**:
  - El MCP no ofrece métodos para editar la configuración, decidir en la cola, instalar o desinstalar hooks, ni para la excepción consciente (BR-AUTH-004, ADR-GRD-007).
  - No fija el actor ni la capa `hooks`.
  - La decisión de una operación ejecutada por el MCP se registra una sola vez, con la capa `mcp`.
- **Pendiente para F-001-05**: el catálogo de herramientas y su correspondencia con la operación normalizada; la allowlist.
- **Capa `cockpit`** (Enmienda 2026-10-04, Cockpit): sigue este mismo contrato; ver la sección final.

### 6. Una decisión por transición, una entrada por operación (H-02)

- **Agrupación**: el daemon agrupa por la identidad del `git` más cercano.
- **Cada transición se evalúa**: un `allow` dado a una transición **nunca cubre otra**.
- **Reutilizar solo deduplica el registro**: si llega otra evaluación de la misma transición desde el mismo `git` (`pre-rebase` y después `reference-transaction` sobre la misma rama y la misma base), se devuelve la misma decisión y se escribe una sola entrada.

### 7. Diseñado para encajar (historias bloqueadas)

- **US-GRD-007..010**: añaden reglas en `crates/policy` sin cambiar el contrato.
- **US-GRD-015**: con `effect = ask`, el hook espera la resolución hasta el plazo (Q-GRD-6).
- **US-GRD-017**: con `allow` y una operación destructiva, el daemon pide el snapshot `previo_hook` (ADR-TMC-004 § 3) en la misma llamada; si falla → `deny` con `snapshot-failed` (BR-EDGE-005).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Evaluar en el cliente del hook como vía principal | Más superficie del entorno del agente, sin actor ni excepciones. Se conserva solo como modo degradado más estricto |
| Una implementación por capa | BR-CONS-002 dependería de la disciplina |
| El cliente declara el actor | Lo suplantaría un agente |
| Deducir la ruta del canal del entorno | Un agente apunta `XDG_RUNTIME_DIR` a un daemon falso que siempre permite (H-03) |
| Ligar la decisión a "cualquier antecesor" | Un `git` lanzado por un agente bajo un proceso legítimo heredaría la decisión o la excepción (H-02) |
| Fail-open si el daemon no responde | Contradice el fail-safe |
| Leer los niveles personales en modo degradado | Abre el perfil desde un cliente (ADR-GRP-006 § 4) y no aporta seguridad (J7) |
| El motivo como texto ya traducido | Rompe el i18n y facilita inyectar instrucciones en el texto que lee el agente |

## Consecuencias

- ✅ Una sola implementación para las dos capas, con pruebas de propiedades sobre una función pura.
- ✅ Ni un canal falso ni un repo cruzado consiguen un `allow`: dan deny en refs gobernadas.
- ✅ El modo degradado es más estricto que el normal y deja rastro en el daemon.
- ⚠️ **El modo degradado escribe un spool y lee una instantánea del perfil desde un cliente.** Choca con ADR-GRP-005 § 1 y ADR-GRP-006 § 4. Son excepciones acotadas, **reconocidas por motor-local el 2026-10-04** (ADR-GRP-005 § 1, ADR-GRP-006 § 4; tabla de [non-functional-guardrails.md](../non-functional-guardrails.md)).
- ⚠️ **Verificar el servidor** añade la lectura de la imagen del par y una comprobación de firma o de huella por conexión. **Mitigación**: el cliente la cachea por pid y hora de inicio del servidor.
- ⚠️ **En modo degradado no hay relajaciones del equipo.** Puede bloquear lo que el equipo permitía, mientras dure la ventana.

## Validación

1. **Pureza**: la misma entrada da la misma decisión (prueba de propiedades); `crates/policy` no depende de E/S.
2. **BR-CALC-001**: todas las reglas que deniegan aparecen en `reasons`.
3. **Monotonía**: añadir una fuente que endurece (worktree o personal) nunca relaja (prueba de propiedades).
4. **Mínimo**: sin configuración, el force-push y el borrado de `main` se deniegan y el commit pasa (US-GRD-001). Un `HEAD` de worktree que intenta desactivar el mínimo no lo desactiva.
5. **Actor**: la misma operación como agente detectado, como agente registrado y como "sin atribuir" recibe la misma decisión, con el actor correcto en el registro (US-GRD-006).
6. **Canal** (H-03; Judge ronda 2):
   - Con `XDG_RUNTIME_DIR`/`HOME` apuntando a un socket falso, el cliente usa la ruta fijada.
   - **Con el perfil borrado o con `HOME` falso, el hook sigue llegando al daemon real.** Si el perfil se recrea, aplica el modo degradado con `instance-mismatch`.
   - Un servidor que no es el binario instalado da deny en refs gobernadas, con la causa `channel-not-authentic`.
   - Un daemon auténtico arrancado con otro perfil en la ruta del canal da modo degradado con `instance-mismatch`, nunca un `allow` de ese daemon.
7. **Directorio común** (M-02): con `GIT_DIR` apuntando a otro repo protegido o no protegido, se deniega el borrado de la rama base del repo fijado en el dispatcher.
8. **`git` más cercano** (H-02): un `git` lanzado por un agente bajo un proceso del ejecutor no hereda su decisión. Un `allow` dado a una transición no cubre otra del mismo `git`.
9. **Modo degradado** (H-03, J7): con el daemon parado e imposible de arrancar, el force-push se deniega con `degraded`. Un suelo que desactiva el mínimo no lo desactiva. No se abre ningún archivo de configuración del perfil (traza de apertura de archivos). Al volver el daemon, la ventana degradada aparece en el estado y en el registro.
10. **Mensajes** (M-05): una rama con escapes ANSI/OSC, controles bidi o U+2028, o con texto de instrucciones, sale neutralizada, etiquetada y truncada. Ningún mensaje menciona la excepción.
11. **Remoto como URL** (M-06): un push a `https://user:token@host/r.git` no deja el token en el registro ni en la auditoría.
12. **Coste** (L-03): una expresión de formato patológica y un glob patológico se evalúan en tiempo lineal.

## Referencias

- **Reglas**: BR-CALC-001, BR-CONS-001, BR-CONS-002, BR-AUTH-003, BR-AUTH-004, BR-EDGE-001, BR-EDGE-004, BR-EDGE-005, BR-VAL-002; S-GRD-9; Q-GRD-1, Q-GRD-5, Q-GRD-6; D6 (Rene Bonilla, 2026-10-04).
- **Historias**: US-GRD-001, US-GRD-005, US-GRD-006; diseñado para US-GRD-007..017.
- **ADRs de otros frentes**: ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-012, ADR-GRP-013 (motor-local, en `main`); ADR-TMC-002, ADR-TMC-004, ADR-TMC-005 (time-machine, en `main`).
- **Diagrama**: [seq-guardrails-decision-hook.md](../diagrams/seq-guardrails-decision-hook.md).
- **Seguridad**: SEC-GRD-03, 06, 11, 16, 19.

## Revisión de seguridad (2026-10-04)

| Hallazgo | Cómo se cubre |
|---|---|
| H-02 · Ligadura y deduplicación por "cualquier antecesor" | § 4 y § 6: `git` antecesor más cercano, identidad verificada en cada antecesor, ejecutor solo como padre directo, un `allow` nunca cubre otra transición; Validación 8 |
| H-03 · Canal suplantable y modo degradado forzable | § 4: ruta del canal fijada, servidor verificado (deny si no lo es), reglas del modo degradado más estrictas, causa `daemon-unreachable`, ventana registrada en el daemon; la frase falsa de SEC-GRD-03 está corregida; Validación 6 y 9 |
| J7 · Lectura del perfil en modo degradado | § 4: no se leen niveles personales |
| M-02 · `GIT_DIR`/`GIT_WORK_TREE` cruzados | § 4: identificador del directorio común fijado y cwd verificado; deny en refs gobernadas; Validación 7 |
| M-05 · Mensajes al agente | § 3: datos etiquetados, neutralización de Cc/Cf/Zl/Zp, sin menciones de la excepción; Validación 10 |
| M-06 · URL del remoto y argv | § 1: remoto sin `userinfo`, nunca argv; Validación 11 |
| M-07 · Servidor del pipe en Windows | § 4: imagen del servidor del named pipe verificada por Authenticode |
| L-03 · Coste de las expresiones | § 1: motor de tiempo lineal y globs lineales; Validación 12 |
| D6 · Suelo en la rama principal | § 1 y § 2: el suelo es la única fuente que desactiva el mínimo |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes |
| Judge ronda 2, hallazgo 1 · Canal y directorio común como constantes | § 4: sin derivación del entorno ni lectura del perfil; Validación 6 con el perfil borrado y `HOME` falso |
| Judge ronda 2, hallazgo 3 · Daemon auténtico con perfil ajeno | § 4: id de instancia en el handshake, modo degradado con `instance-mismatch`, residuo de la copia completa del perfil declarado; Validación 6 |

## Cambios (2026-10-04, coherencia con motor-local)

- § 2: el nombre y el tipo de la clave del mínimo los fija el schema de TS-GRD-001; una fuente `parcial` tampoco desactiva el mínimo (D12).
- § 4: la rama base confirmada vive en el almacén por repo; la excepción del spool y de la instantánea está aplicada en ADR-GRP-005 § 1 y ADR-GRP-006 § 4.
- Consecuencias: la contradicción con "único escritor del perfil" pasa a reconocida (aplicada, 2026-10-04).
- Cierre (ronda 3): la instantánea es una copia de solo lectura exportada desde el almacén por repo; el cliente nunca abre el SQLite.
- Corrección tras el Judge (ronda combinada): § 2, la rama base hasta TS-GRD-001 sigue las dos fases de ADR-GRD-004 § 3.5 (D9 / Q-GRD-23).

## Enmienda (2026-10-04, SPIKE-GRD-001)

Derivada de la Enmienda de ADR-GRD-002 (E-02-7 y E-02-8). **Decisión del orquestador (2026-10-04), validada por el Arquitecto y el PO.** El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| Códigos `historia-superficial` y `renombrado-sobre-base` (con el oid para recuperar) | § 3 | SPIKE-GRD-001 F07 y D11 |

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-10 de [CTX-CKP-001](../../requirements/features/cockpit/context.md), con [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) § 2, § 4 y § 12 (accepted 2026-10-04). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia la función de evaluación, el mínimo seguro, la forma de la decisión ni el modo degradado. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| Capa del contexto: `hooks` \| `mcp` \| `cockpit` | § 1 | DEP-CKP-10; ADR-CKP-002 § 4 |
| El ejecutor registra el hijo `git` y sus transiciones **con la capa de la petición**; con un `git` del ejecutor no se pide `previo_hook`; la evaluación de un hook **nunca espera** al cerrojo de escritura del repo | § 4 | ADR-CKP-002 § 4 y § 5 |
| La capa `cockpit` sigue el contrato del § 5: decidir antes de la operación protegida | § 5 | ADR-CKP-002 § 2 |
| **La capa la fija el daemon** según el solicitante resuelto, no el cliente: `mcp` para un agente, `cockpit` solo para un "sin atribuir" que pasa los controles 1 a 3 (pasada de endurecimiento, 2026-10-04) | § 1 y § 5 | M-03; ADR-CKP-002 § 4 |
| **Actor de los `git` nietos**: el `git` lanzado por un hook bajo un hijo registrado del ejecutor se evalúa de nuevo, con el **solicitante del plan** como actor (pasada de endurecimiento, 2026-10-04) | § 4 y § 6 | H-01; ADR-TMC-005 § 1 (Enmienda, Cockpit) |
| **Abort del rebase atómico**: sus transiciones se registran con el plan y no se reevalúan (pasada de endurecimiento, 2026-10-04) | § 4 y § 6 | DEP-MCP-5 (CTX-MCP-001); ADR-CKP-002 § 4 y § 9 |

- **Capa `cockpit`**: la de las peticiones de la TUI al ejecutor. Igual que el actor, **la capa no cambia la decisión** (§ 1); entra en el registro y en lo que se puede confirmar. "Pedir confirmación" se aplica como denegar mientras no haya cola (S-GRD-9), igual que en `mcp`.
- **Antes de cualquier efecto**: el daemon evalúa al preparar (vista previa) y **otra vez al ejecutar**, bajo el cerrojo del repo y antes del snapshot previo. La segunda es la que cuenta (BR-CKP-WF-002). Es la única protección para merge y borrar worktree, que los hooks no impiden (ADR-GRD-002).
- **Ligadura con los hooks** (§ 4): al lanzar `git`, el ejecutor registra **en el mismo paso** la identidad del hijo y las transiciones del plan, con la capa de la petición (`cockpit` o `mcp`). Las transiciones que no se conocen de antemano se registran como en ADR-GRD-007 § 3, paso 4: ref y base en el rebase; ref, valor viejo y oid integrado en el merge. Cuando un hook pregunta y el `git` más cercano es ese hijo, con el daemon como padre directo, recibe la decisión ya tomada; otra transición se evalúa de nuevo (§ 6). Un `git` lanzado por un hook del usuario (nieto del daemon) no hereda la decisión.
- **Sin `previo_hook`** con un `git` del ejecutor: ya existe el `previo_garantizado` de la operación protegida (ADR-TMC-004).
- **Sin interbloqueo**: la evaluación de un hook bajo el ejecutor nunca espera al cerrojo de escritura del repo (ADR-TMC-002, Enmienda (2026-10-04, Cockpit)); si esperara, el hook del propio `git` del ejecutor se bloquearía.
- **Registro**: una entrada como mucho por plan, con `layer = cockpit` (o `mcp`), escrita al cerrarse el plan (ADR-GRD-006, Enmienda (2026-10-04, Cockpit)).
- **Validación añadida** (ADR-CKP-002, Validación 5 a 7): un merge permitido sin regla no deja entrada; un borrado de rama denegado deja una `denial` con `layer = cockpit`; un `git` nieto no hereda la decisión; un hook bajo el ejecutor recibe su decisión con el cerrojo tomado.

**Pasada de endurecimiento (2026-10-04)**. Decisión del orquestador (2026-10-04), validada por Arquitecto, PO y security-expert.

- **Capa** (§ 1 y § 5; M-03): el daemon la fija según el solicitante resuelto, sea cual sea el cliente. `mcp` si es un agente, también desde la TUI; `cockpit` solo para un "sin atribuir" que pasa los controles 1 a 3 de ADR-GRP-005 § 6, incluido el rechazo `daemon-descendant` (Enmienda (2026-10-04, TS-GRP-004), punto 7). Ningún cliente la declara. La capa sigue sin cambiar la decisión de la función pura; cambia lo que se puede confirmar y qué operaciones del catálogo se admiten (ADR-CKP-002 § 1).
- **Actor de los `git` nietos** (§ 6; H-01): el actor de la regla del § 4 se busca desde el `git` más cercano hacia arriba. Si en ese recorrido aparece un **hijo registrado del ejecutor**, el actor es el **solicitante del plan de ese hijo**, nunca "sin atribuir". La transición del nieto **se evalúa de nuevo** (no hereda la decisión del plan) con la capa `hooks`, y deja su propia entrada si corresponde. Así un hook que lanza `git push --force` bajo un `commit` de claude-1 se evalúa y se registra como claude-1. La atribución no concede nada: si ese hook pide además un comando reservado por el canal, se rechaza con `daemon-descendant`.
- **Abort del rebase atómico** (§ 4 y § 6; DEP-MCP-5): el `git rebase --abort` que el ejecutor lanza dentro de la misma operación cuando un `safe_rebase` choca o vence su tiempo es **otro hijo registrado del mismo plan**. Sus transiciones (HEAD y la rama vuelven a su valor previo) se registran antes de liberarlo y reciben la decisión ya tomada: **no se reevalúan** y no crean entrada propia. Un `git` nieto lanzado por un hook durante el abort sigue la regla anterior.
- **Barrera de arranque** (§ 4; I-02): el hijo del ejecutor no ejecuta nada hasta que su identidad y sus transiciones están registradas. Ningún hook pregunta por un `git` del ejecutor que el daemon aún no conoce.
- **Validación añadida** (ADR-CKP-002, Validación 17, 18, 21 y 28): un `git` nieto bajo un plan de claude-1 se evalúa con actor claude-1; el abort de un `safe_rebase` que choca no genera una segunda evaluación; un hook que se conecta al arrancar encuentra al hijo registrado; un agente que abre la TUI recibe la capa `mcp`.

## Enmienda (2026-10-05, MCP)

Decisión del orquestador (2026-10-05), validada por Arquitecto y PO. Origen: DEP-MCP-5 (CTX-MCP-001), ADR-MCP-001 § 4.1 y § 4.4. El resto de DEP-MCP-5 (abort del rebase atómico) ya está en la Enmienda (2026-10-04, Cockpit).

| Herramienta MCP | Operación del catálogo | Operación normalizada (BR-VAL-002) |
|---|---|---|
| `safe_commit` | `commit` | Commit |
| `safe_rebase` | `rebase-onto-base` (`atomic`) | Rebase |
| `create_worktree` | `create-worktree` | Crear worktree |
| `snapshot` | `snapshot` | **Ninguna: no gobernada** |
| `undo` | — (Time Machine) | **Ninguna en el MVP** (política del undo: US-TMC-021, Fase 2) |
| Lecturas y `register_agent` / `unregister_agent` | — | Ninguna |

- La decisión de las tres operaciones gobernadas se toma con capa `mcp` (fijada por el daemon) dentro del flujo del ejecutor (§ 4 y § 5); "pedir confirmación" se aplica como denegar mientras no exista la cola (S-GRD-9).

## Enmienda (2026-10-05, US-GRD-001)

Desviaciones de la implementación de US-GRD-001 respecto al § 4 ([DS-US-GRD-001](../../requirements/features/guardrails/dev-specs/US-GRD-001-proteger-repo-force-push.md), D9 a D11). **Decisión del orquestador (2026-10-05), validada por Arquitecto.** No cambian la función de evaluación, el mínimo seguro ni la forma de la decisión. El `status` sigue en `accepted`.

| Cambio | Resolución | Fuente |
|---|---|---|
| Servidor = binario instalado | El cliente compara la **identidad del archivo** del ejecutable del par (pid del kernel; ruta y `dev/inode`) con la de su propio ejecutable, **antes de enviar nada**, en lugar de la firma o la huella: los binarios de desarrollo no están firmados. En Linux es el inodo real (`/proc/<pid>/exe`); en macOS es la ruta y `stat`, más débil (ventana entre comprobación y uso), y se declara | § 4; H-03 |
| Directorio común (M-02) | Lo comprueba el **cliente del hook** (que es el proceso del hook y ve su `GIT_DIR` y su cwd), además de que el stub compruebe sus constantes. La comprobación del lado del daemon queda pendiente en macOS (US-GRP-009: `process_cwd` necesita un wrapper seguro de libproc) | § 4; M-02 |
| Arranque bajo demanda desde un hook | **No en US-GRD-001**: sin daemon, el modo degradado (más estricto) decide; queda para US-GRD-005 con el registro | § 4 |
| Sin interbloqueo | `guard.evaluate` se atiende en el hilo de la conexión desde un registro que publica el bucle; nunca espera al bucle ni al cerrojo del repo | Enmienda (2026-10-04, Cockpit) |
| Mensajes (M-05) | El saneado neutraliza también Zl, Zp y Cf; los parámetros se acotan a 120 caracteres | § 3 |

## Enmienda (2026-10-06, US-GRD-018)

Origen: BR-26 y D6 del BRD (Rene Bonilla, 2026-10-06), BR-AUTH-005 y ADR-GRP-012, Enmienda (2026-10-06, autoría de commits) § 4 y § 6, que pedían enmendar el § 1 y el § 4 con la Dev Spec de US-GRD-018 ([DS-US-GRD-018](../../requirements/features/guardrails/dev-specs/US-GRD-018-autoria-commits-persona-y-agente.md)). **Decisión del orquestador (2026-10-06), validada por el Arquitecto.** No cambia el mínimo seguro, el orden `deny > ask > allow` ni el canal. No relaja la garantía del modo degradado: le añade un residuo declarado (pierde las reglas de autoría). El `status` sigue en `accepted`.

| Cambio | Resolución | Dónde |
|---|---|---|
| **El actor entra en la condición de las reglas de autoría** | `authorship.trailer-required` (`agents-commit`) y `authorship.human-author` son las únicas reglas que leen el actor. Solo actúan si el actor es un agente, detectado o registrado: con "sin atribuir" no deniegan ni avisan (BR-EDGE-004; excepción consciente al fail-safe de BR-AUTH-003, declarada en BR-AUTH-005). Solo endurecen y no eximen a nadie de otra regla, así que el principio de Q-GRD-1 (el actor nunca relaja) se mantiene | § 1 |
| **Hechos de autoría** | Entran como hechos de contenido: los tipos de agente de los `Co-Authored-By` reconocidos por la tabla versionada de `crates/policy`, si el mensaje era legible y la versión de la tabla. El cliente del hook los calcula; al daemon nunca llegan el mensaje, nombres ni correos (M-06) | § 1 |
| **El actor lo resuelve el daemon** | Señal S4 desde el `git` antecesor más cercano: sesión detectada, después agente registrado en el worktree, después "sin atribuir". El cliente no puede declarar el actor. Lo implementa US-GRD-018 (adelanta el pendiente de US-GRD-005/006) | § 4 |
| **Avisos** | `Decision.notices[]` (forma de `reasons[]`). La capacidad `guard.authorship` de ADR-GRP-016 cubre este campo y `EvaluateParams.authorship`: el cliente solo envía los hechos de autoría si el daemon la concede, así que un daemon sin ella evalúa como antes y no rechaza la petición: reglas que avisan sin cambiar `effect`. `human-author` con `warn` da `allow` más un aviso, nunca `ask`, porque `ask` se aplica hoy como `deny` (S-GRD-9). Si `appliedEffect` no es `allow`, los avisos se descartan | § 3 |
| **Dónde se evalúan** | Dispatchers nuevos `pre-commit` y `commit-msg` (plantilla 2, ADR-GRD-001) y `reference-transaction` `prepared` como segunda línea para `--no-verify`: con actor agente, un único commit nuevo con la forma de un commit, una fusión o un amend se evalúa **siempre**, salvo que el subcomando del `git` antecesor sea con certeza `rebase`, `cherry-pick`, `revert` o `am` (conservan autor y trailers del original). Un alias, `commit-tree` + `update-ref` o una línea de órdenes ilegible se evalúan. El subcomando se lee tras saltar las opciones globales, solo para clasificar; no se guarda ni se envía (lo que nunca guarda sigue siendo argv) | § 4; ADR-GRD-002 § 1 |
| **Garantía del modo degradado** | El actor es siempre "sin atribuir", así que el modo degradado **pierde las reglas de autoría** aunque estén en el suelo legible. Residuo declarado: no relaja nada del mínimo ni de las demás reglas | § 4 |

**Validación añadida**: con un actor agente, `agents-commit` deniega sin su trailer y `human-author` deniega o avisa; con "sin atribuir" (incluido el modo degradado) ninguna regla de autoría deniega; un aviso nunca cambia `effect`; un cliente frente a un daemon sin `guard.authorship` no envía los hechos de autoría y no recibe avisos; el daemon nunca recibe el mensaje de commit.

## Enmienda (2026-10-08, US-GRD-008)

Origen: BR-VAL-003 (filas "Rama protegida" y "Ruta prohibida") con BR-CALC-001, y la Dev Spec de US-GRD-008 ([DS-US-GRD-008](../../requirements/features/guardrails/dev-specs/US-GRD-008-ramas-protegidas-rutas-prohibidas.md), D1 a D12), que pedía enmendar el § 1 y el § 4. **Decisión del orquestador (2026-10-08), validada por el Arquitecto y el PO.** No cambia el mínimo seguro, el orden `deny > ask > allow` ni el canal, y solo añade denegaciones. Añade un residuo declarado al modo degradado. El `status` sigue en `accepted`.

| Cambio | Resolución | Dónde |
|---|---|---|
| **El actor entra en la condición de `policy.protected-branch` y `policy.forbidden-path`** | Con `appliesTo: agents` (valor por defecto) la regla deniega solo si el actor es un agente, detectado o registrado; con "sin atribuir" pasa. Con `appliesTo: everyone` (opt-in) deniega a todos. Solo añaden denegaciones y no eximen de ninguna otra regla. **Precisa** la frase "El actor no cambia la decisión" del § 1: tras US-GRD-018 las reglas de autoría y ahora estas dos son las únicas que leen el actor. Es una excepción consciente al "toda operación, sea cual sea el actor" de Q-GRD-1, que refina Q-GRD-35 de `context.md` | § 1 |
| **Configuración combinada por unión** | `policies.protectedBranches` y `policies.forbiddenPaths` son la unión de las reglas del suelo, el suelo confirmado, el worktree y el perfil. Cada regla conserva su nivel y su `appliesTo`. Ninguna fuente quita el patrón de otra; si dos niveles declaran el mismo patrón con distinto `appliesTo`, gana `everyone`. Un patrón inválido se descarta solo él y deja la fuente en `parcial`, que fuerza el mínimo seguro (ADR-GRD-004 § 1) | § 1; ADR-GRD-004 |
| **Hechos de contenido: las rutas del movimiento** | El daemon lee los commits nuevos del movimiento con el lector aislado, y solo si hay una regla de rutas aplicable al actor. Primera pasada: oculta solo `old`. Si no hay un hit, no se hace más. Si lo hay, confirma ocultando también las demás ramas y ramas remotas, para no denegar lo que ya existía. Topes: 256 commits nuevos y 100 000 visitados por línea, 4 096 puntas de ramas, y 4 096 commits nuevos y 100 000 rutas por evaluación. Pasar un tope, un árbol ilegible, objetos ausentes o un repo superficial que no permite probar el rango es `unverifiable`: deny con fail-closed, **solo cuando hay una regla aplicable**. Al daemon y al cliente no viaja nada nuevo: el cliente no envía rutas | § 1 |
| **Capacidad `guard.policies`** | El daemon aplica estas reglas y resuelve el actor de `RefTransaction` y `Push` solo si la conexión la pidió (ADR-GRP-016). Es una puerta de compatibilidad, no de seguridad: un daemon anterior o un hook anterior no cambian de comportamiento, y las dos direcciones tienen prueba | § 3 |
| **Garantía del modo degradado** | Se enmienda: se aplican las reglas `everyone` del suelo, con el mismo código. Las reglas `agents` no, porque el actor es "sin atribuir". **Residuo declarado**: un agente puede forzar el modo degradado borrando el perfil (`instance-mismatch`) o impidiendo que arranque el daemon, y entonces las reglas `agents` no actúan. El modo degradado no pierde nada del mínimo ni del suelo `everyone`. **Pregunta abierta para Rene** (va en el PR): aplicar también las `agents` a "sin atribuir" en degradado sería más seguro, pero bloquearía a la persona mientras el daemon esté caído | § 4 |
| **Razones, causa y parámetros nuevos** | Reglas `policy.protected-branch` y `policy.forbidden-path`; causa `unverifiable`; parámetros `pattern` y `path`, no confiables como los demás y saneados. Todas las reglas incumplidas se nombran juntas en una decisión (BR-CALC-001), una razón por patrón con la primera ruta que lo cumple. Ningún código de error nuevo | § 3 |
| **Lista "no se puede impedir"** | Tres entradas nuevas: `policy-floor` (las reglas del equipo se leen de la copia de la rama principal que el repo tiene del remoto, que un agente puede reescribir a mano; el cierre es una marca de agua por repo que necesita la confirmación de US-GRD-014), `policy-actor` (un agente que el daemon no detecta ni tiene registrado cuenta como persona; en degradado solo rigen las `everyone`) y `policy-reach` (lo que ningún hook ve: un objeto suelto, `stash`, el servidor, `reftable` con `branch -m/-M`, una rama remota escrita a mano y `branch -c/-m` que crean una rama protegida y **un push solo a tags u otras refs no gobernadas**, que el dispatcher deja salir sin evaluar y cuyo cierre exige una plantilla nueva: [TD-GRD-001](../../requirements/features/guardrails/technical-stories/TD-GRD-001-dispatcher-plantilla-3-pre-push-toda-ref.md)) | § 3; ADR-GRD-002 § 3 |

**Validación añadida**: un agente no mueve (crea, actualiza ni borra, ni local ni en el remoto) una rama protegida, ni con `--no-verify`, y la persona sí salvo con `everyone`. Un agente no commitea una ruta prohibida (modificar, crear, borrar) por ninguna vía que cree un commit nuevo, y la ruta tampoco sale por `push` desde un `HEAD` separado. Dos reglas incumplidas se nombran juntas. Un nivel personal solo endurece. Sin daemon rigen las `everyone` del suelo y no las `agents`. Un tope superado deniega con `unverifiable` solo si hay regla aplicable. Al daemon no llega ninguna ruta del cliente, y cliente y daemon sin `guard.policies` se comportan como antes.

**Decisión del orquestador (2026-10-08), validada por el Arquitecto.** Del Arquitecto, B2 (garantía del modo degradado), B3 (decisión de negocio nueva, Q-GRD-35) y D4/D5 (pasada propia de la rama protegida, topes de trabajo). Su B1 pedía cerrar el push solo a tags; el orquestador lo declaró como `policy-reach` y lo derivó a TD-GRD-001, porque exige cambiar la instalación del dispatcher, fuera de esta historia.

## Nota (2026-10-08, XP-15): identidad del ejecutable en Windows

La comprobación "servidor = binario instalado" de la Enmienda (2026-10-05, US-GRD-001) funciona también en Windows: la identidad del archivo es `(número de serie del volumen, índice de archivo)`, leída siguiendo enlaces, y es la misma para cualquier grafía de la ruta (mayúsculas o nombre 8.3). Antes el ejecutable del par quedaba como desconocido y el hook pasaba al modo degradado. No cambia el contrato. **Decisión del orquestador (2026-10-08), validada por Arquitecto.**

## Enmienda (2026-10-08, US-GRD-010 y US-GRD-012)

Origen: BR-CONS-001, BR-VAL-001, BR-AUTH-004 y BR-AUTH-001, con la Dev Spec `docs/dev-briefs/layered-config.md`. **Decisión del orquestador (2026-10-08), validada por Arquitecto y PO; el coordinador ajustó D11.**

| Cambio | Resolución | Dónde |
|---|---|---|
| **Regla `policy.config-protected`** | Regla de producto de nivel `minimum`, solo para agentes, sobre `/.gitraptor` (el directorio y la ruta exacta, sea archivo, enlace o submódulo). No es el mínimo de BR-EDGE-001: `disableSafeMinimum` no la apaga y ninguna configuración la quita. `raptor guard status` la muestra siempre | § 1, § 2 |
| **Lo no verificable deniega al agente** | Con un actor agente, un movimiento cuyos commits no se pueden leer o pasan de 256 nuevos se deniega (causa `unverifiable`). Se enmienda "unverifiable solo con regla aplicable" de US-GRD-008: ya no vale para agentes. Casos: rebase largo, primer push de una rama local con más de 256 commits, clon superficial, repo sin ramas de seguimiento | § 6 |
| **Niveles personales solo endurecen** | El permiso efectivo es el más restrictivo entre el equipo y el personal (local sobre perfil); un nivel personal ignorado o ilegible nunca relaja. Los niveles personales no se escriben desde el motor | § 1 |
| **Quién confirma una relajación (D11)** | Se decide por el **actor** que ve el hook en el commit (ascendencia del proceso), nunca por el autor, el committer ni el trailer, porque el agente commitea con la identidad de la persona. Actor agente: la relajación se ignora y se avisa. "Sin atribuir": cuenta como la persona (**riesgo residual aceptado del MVP**, Q35 y R-GRD-3). Un commit sin hooks no confirma nada | § 4 |
| **Aviso `config.relax-ignored`** | Una operación deja una entrada de decisión; los avisos de configuración con el mismo `decision_id` son aparte, no cuentan en el KPI de bloqueos ni aplican al ejecutor. Solo con actor agente y en `ref-transaction` o `push`. Lleva el nivel y nunca valores. Dice que se ignoró una relajación mientras actuaba el agente, sin afirmar quién la escribió | § 6 |
| **Capacidad `guard.config-protection`** | Un cliente sin ella recibe la denegación como `policy.forbidden-path`, nunca como un permiso | § 4 |

**Corrección de la revisión de seguridad (2026-10-09)**: el recorrido de commits nuevos oculta solo las puntas de `refs/heads/*`, nunca `refs/remotes/*` (un commit aparcado bajo `refs/remotes` ya no cuenta como existente), y un cambio de `.gitraptor` seguido de su reversión **sigue denegándose** (aceptarlo lavaba el commit intermedio: quedaba alcanzable desde una rama local y se podía crear una rama o subirlo sin commits nuevos). Se mantiene el tope de 256 commits (deniega como `unverifiable`). Costo: un agente que integra el `origin/main` cuando la persona cambió la configuración y el `main` local está desactualizado se deniega (falla cerrado) hasta que la persona avance el `main` local; cierre posible: comparar con el blob del suelo confirmado.

**Declarado, no cerrado**: las puntas de `refs/heads/*` escritas sin hooks cuentan como existentes (mismo residuo que `core.hooksPath`, ADR-GRD-002); `git branch -C` sobrescribe una rama existente sin ejecutar el hook con el backend de archivos (a añadir a `policy-reach`); M-01 (retroceder la rama base o forjar `refs/remotes/<r>/main` no crea commits nuevos; cierre: marca de agua de US-GRD-014; residuos `policy-floor` y `policy-reach` aplicados a la configuración) y R-GRD-4 (un agente puede borrar o corromper los endurecimientos de su propio perfil o local, nunca bajar del equipo; el cierre es el trinquete de Q-GRD-32). Las ediciones sin commitear no se registran (ADR-GRD-004, validación 4). Windows y Linux: XP-40.

## Enmienda (2026-10-09, TS-CKP-003)

Origen: [DS-TS-CKP-003](../../requirements/features/cockpit/dev-specs/TS-CKP-003-capa-cockpit-guardrails.md), D5, D6 y D12. **Decisión del orquestador (2026-10-09), validada por Arquitecto; D12 también por PO.** No cambia la función de evaluación, el mínimo seguro, la forma de la decisión ni el modo degradado. El `status` sigue en `accepted`.

| Cambio | Resolución | Dónde |
|---|---|---|
| **Transición distinta del hijo directo** | Se evalúa de nuevo con la capa y el actor del plan y con **todas las reglas** (nunca las capacidades de la conexión del hook), y **no crea entrada propia**: se acumula con la decisión del plan y la entrada única del plan lleva el máximo. Si la ejecución falla por ella, la respuesta lleva esa decisión con el mismo `decision_id` que la entrada | § 6 y Enmienda (2026-10-04, Cockpit) |
| **Solo el padre directo hereda** | Se confirma el "padre directo": un `git` interno de un hijo registrado o un `exec git` desde un hook es un nieto (evaluación nueva, capa `hooks`, actor del plan, entrada propia si corresponde). El daemon reconoce un `git` solo por el nombre del ejecutable, así que heredar por cadena de procesos `git` violaría H-01 y H-02 | § 4 |
| **Repos sin la capa de hooks** | El ejecutor evalúa igual, como una instalación huérfana, y sin confirmación inicial protege la unión {`main`, rama principal, rama base leída} (Q-GRD-21, BR-CONS-003). Merge y borrar worktree no tienen otra protección. Que el estado de protección refleje esta cobertura es un pendiente de US-GRD-004 (BR-WF-002) | § 4 |
