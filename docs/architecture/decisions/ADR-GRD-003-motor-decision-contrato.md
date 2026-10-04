---
id: ADR-GRD-003
title: Motor de decisión, mínimo seguro y contrato de decisión (hooks hoy, MCP como interfaz)
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRP-002, ADR-GRD-001, ADR-GRD-002, ADR-GRD-004, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, motor-decision, crates-policy, contrato, mcp, minimo-seguro, actor, fail-safe, br-calc-001, br-cons-002, modo-degradado, canal-autenticado]
---

# ADR-GRD-003 — Motor de decisión, mínimo seguro y contrato de decisión

## Contexto

Para cada operación gobernada Guardrails decide permitir, pedir confirmación o denegar. Las reglas que fijan esa decisión son:

- **BR-CALC-001**: gana la más restrictiva de las reglas que aplican y la decisión nombra todas las reglas que la causan, con su nivel.
- **BR-CONS-002**: la decisión es la misma por MCP y por hooks.
- **BR-AUTH-003, Q-GRD-1**: las reglas se aplican igual a un "agente X" que a "sin atribuir".
- **S-GRD-9**: "pedir confirmación" se trata como "denegar" mientras no exista la cola.
- **BR-EDGE-001, Q-GRD-5**: sin configuración aplica un mínimo seguro (denegar force-push y el borrado de la rama base), visible y desactivable por el equipo. Por D6 (2026-10-04), solo se desactiva desde la rama principal (ADR-GRD-004).

Las herramientas MCP son de F-001-05. Aquí solo se fija la interfaz que consumen (BR-AUTH-004). El actor lo emite el motor (Q34 y Q35 de motor-local; ADR-GRP-013 § 6, propuesto).

La decisión tiene que ser explicable (contexto § 6), i18n en/es (NFR-10), < 100 ms (ADR-GRD-002 § 5) y fail-safe ante cualquier duda. El hook corre en el entorno del agente, así que ni el canal ni el entorno son de fiar (revisión de seguridad, H-03).

## Decisión

**La evaluación es una función pura y determinista en `crates/policy`. El daemon la aloja y la expone por el canal local (`crates/api`); el cliente del hook solo habla con un daemon autenticado, por una ruta fijada al instalar. El actor y la identidad de la operación salen del `git` antecesor más cercano al hook. Si no hay daemon, un modo degradado más estricto evalúa sin leer el perfil.**

### 1. Función de evaluación (`crates/policy`)

Sin E/S, sin reloj y sin aleatoriedad:

`evaluar(operación normalizada, contexto, configuración efectiva) → Decisión`.

- **Operación normalizada**: una del catálogo de BR-VAL-002 con sus **transiciones exactas** (ref normalizada, valor viejo y valor nuevo; ADR-GRD-002 § 4), el remoto y, si una política activa lo pide, los hechos de contenido.
  - **Nombre del remoto** (M-06): si es una URL, se guarda sin `userinfo`, o como `<url>` si no se puede limpiar.
  - **Lo que nunca guarda**: argv.
- **Contexto**: repo (clave de ADR-GRP-006), worktree, rama o ramas base protegidas (ADR-GRD-004 § 3), capa (`hooks` o `mcp`) y actor.
- **Configuración efectiva**: la de ADR-GRD-004, por fuentes, cada una con su estado: `mínimo`, `suelo` (la rama principal), `worktree` (el `HEAD` de la operación, que solo endurece) y los niveles personales.
- **Combinación**:
  - Primero, el suelo y el mínimo. Después, lo que endurecen el worktree y los niveles personales (BR-CONS-001, D6).
  - Después, `decisión = máx(reglas)` con `denegar > pedir confirmación > permitir`.
  - Las razones son todas las reglas que producen el máximo (BR-CALC-001).
- **Coste acotado** (L-03):
  - Las expresiones del formato de commit usan un motor de **tiempo lineal** con tope de tamaño.
  - Los globs de rutas se evalúan en tiempo lineal.
  - Los límites del documento JSON están en ADR-GRD-004 § 1.
- **El actor no cambia la decisión** (Q-GRD-1). Solo entra en el registro y en las excepciones.

### 2. Mínimo seguro (BR-EDGE-001, Q-GRD-5)

| Regla | Código | Efecto | Ámbito |
|---|---|---|---|
| Prohibir force-push | `minimum.force-push` | Denegar | Cualquier rama, cualquier remoto |
| Proteger la rama base | `minimum.base-branch-delete` | Denegar | Borrar la rama base, local o en cualquier remoto. Si hay un cambio de rama base pendiente de confirmar (D6), aplica a la **unión** {anterior, nueva} |

- **Activo** salvo que el **suelo** (configuración del equipo en la rama principal, D6) lo desactive. La clave es una de las enmiendas pendientes de ADR-GRP-007.
- **No pueden desactivarlo** ni un nivel personal ni el `HEAD` del worktree, ni el suelo cuando es ilegible o el sistema está en modo degradado.
- **Visible** en el estado de protección (US-GRD-004, ADR-GRD-005).
- **Rama base**: hasta TS-GRD-001 es `main`, el valor por defecto de BR-CONS-003 y US-GRP-012.

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
- **Ejecutor del daemon** (Cockpit y MCP, ADR-TMC-002 § 5): si el **padre directo** del `git` más cercano es el ejecutor, con una operación registrada para esa identidad, el daemon devuelve la decisión ya tomada **para cada transición registrada**. Cualquier transición distinta se evalúa de nuevo.
- **Escrituras internas de la Time Machine**: desactivan los hooks (ADR-TMC-002 § 2) y no pasan por Guardrails. Su autorización es de ADR-TMC-005.
- **Modo degradado** (daemon no arrancable, o daemon auténtico de otra instancia; H-03; J7). Un servidor que **no** es el binario instalado no entra aquí: da deny en refs gobernadas. El cliente evalúa con el mismo crate con estas reglas, todas más estrictas que el modo normal:
  - **Mínimo forzado**: nadie lo puede desactivar.
  - **Equipo que solo endurece**: se lee el `HEAD` del worktree y la copia de la rama principal, y ninguno de los dos relaja.
  - **Sin niveles personales**: no se leen; no se abre el perfil para configuración.
  - **Rama base protegida**: la unión {`main`, la rama principal, la última rama base confirmada, la rama base resuelta del suelo}.
    - **La resuelta** es el `engine.baseBranch` que el cliente lee de la copia de la rama principal. Esa copia ya la lee para el endurecimiento del equipo.
    - **La última confirmada** sale de la instantánea de solo lectura que el daemon deja en el directorio de estado del perfil, cuya ruta es una constante del dispatcher (⚠️ **ASSUMPTION**; enmienda pendiente). Si falta, la unión se queda sin ella.
    - **Así el modo degradado nunca protege menos ramas base que el modo normal**, que protege la confirmada más la resuelta mientras hay un cambio pendiente (ADR-GRD-004 § 3).
  - **Lecturas aisladas**: `gix` aislado del entorno (sin `GIT_*`, sin configuración global ni de sistema) y sin objetos de reemplazo.
  - **Sin excepciones**: no hay excepción posible y el actor es "sin atribuir".
  - **Registro**: la entrada va al spool (ADR-GRD-006 § 4).
  - **Trazabilidad**: el estado registra la causa (`daemon-unreachable`, `instance-mismatch` o, para el deny, `channel-not-authentic`) con aviso, y el daemon guarda la **ventana degradada** (inicio y fin) cuando vuelve, no solo las entradas del spool (ADR-GRD-005, ADR-GRD-006).
  - **Garantía**: el modo degradado nunca es menos restrictivo que el mínimo más el suelo legible. Solo pierde los endurecimientos personales y las excepciones.

### 5. Contrato para F-001-05 (solo interfaz)

- **Método**: el mismo de evaluación, con la capa `mcp`.
  - **Primero, la decisión**: el MCP evalúa antes de pedir la operación protegida (ADR-TMC-004 § 1).
  - **Después, la ejecución**: si `appliedEffect = allow`, la pide; si no, devuelve la decisión estructurada (§ 3).
- **Garantías**:
  - El MCP no ofrece métodos para editar la configuración, decidir en la cola, instalar o desinstalar hooks, ni para la excepción consciente (BR-AUTH-004, ADR-GRD-007).
  - No fija el actor ni la capa `hooks`.
  - La decisión de una operación ejecutada por el MCP se registra una sola vez, con la capa `mcp`.
- **Pendiente para F-001-05**: el catálogo de herramientas y su correspondencia con la operación normalizada; la allowlist.

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
- ⚠️ **El modo degradado escribe un spool y lee una instantánea del perfil desde un cliente.** Choca con ADR-GRP-005 § 1 y ADR-GRP-006 § 4. Son excepciones acotadas que tiene que reconocer el frente motor-local (tabla de enmiendas en [non-functional-guardrails.md](../non-functional-guardrails.md)).
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
- **ADRs de otros frentes**: ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-012, ADR-GRP-013 (`docs/arch-motor-local`); ADR-TMC-002, ADR-TMC-004, ADR-TMC-005 (`docs/arch-time-machine`).
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
