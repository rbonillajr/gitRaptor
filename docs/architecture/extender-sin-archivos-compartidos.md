---
title: Cómo extender sin tocar archivos compartidos
status: expanded
generated: 2026-10-06
updated: 2026-10-08
generator: orquestador
domain: GRP
tags: [flota-de-agentes, conflictos, registro, capacidades, codigos-de-error, i18n, cli, daemon]
related: [ADR-GRP-016, ADR-GRP-005, ADR-CKP-003, API-GRP-IPC]
---

# Cómo extender sin tocar archivos compartidos

Guía corta para cualquier agente que implemente una historia. Con estas reglas, dos historias en paralelo no editan el mismo archivo. La decisión y sus porqués están en [ADR-GRP-016](./decisions/ADR-GRP-016-extension-registro-capacidades.md).

**Regla de oro:** lo nuevo va en el archivo de **tu** módulo. Si una historia te obliga a editar uno de los archivos de la tabla, detente: casi siempre hay un archivo propio para eso.

| Archivo compartido | Qué solo se toca ahí | Qué nunca se añade ahí |
|---|---|---|
| `crates/api/src/lib.rs` | Nada (protocolo congelado en 9) | `PROTOCOL_VERSION`, `API_VERSION` |
| `crates/api/src/methods/mod.rs` | Un módulo del contrato nuevo (3 líneas) | Métodos, notificaciones, capacidades, errores |
| `crates/api/src/rpc.rs` | Nada (lista de códigos congelada en `-32016`) | Códigos de error |
| `apps/cli/src/main.rs` | Nada | Subcomandos |
| `apps/cli/src/commands/mod.rs` | Un subcomando de primer nivel nuevo (1 línea) | Acciones de subcomandos que ya existen |
| `crates/core/src/daemon/mod.rs` | La vida del motor (inicio, bucle y parada) | Campos y llamadas de una feature |
| `crates/core/src/daemon/modules/mod.rs` | Un módulo del daemon nuevo (1 línea) | La lógica del módulo |

`crates/api/tests/architecture.rs` falla si alguien declara un método, una capacidad o un código en `methods/mod.rs` o en `rpc.rs`, o si sube el protocolo.

## Añadir un método

1. Abre el archivo de su módulo en `crates/api/src/methods/<módulo>.rs`. Por ejemplo, `guard.rs` para `guard.uninstall`.
2. Declara la constante y añádela a `methods` en el `GROUP` del archivo:

   ```rust
   /// Removes the hook layer (reserved).
   pub const GUARD_UNINSTALL: &str = "guard.uninstall";
   // en GROUP.methods:
   method(GUARD_UNINSTALL, true, false),
   ```

   - El nombre empieza por el prefijo del módulo (`guard.`); un test lo comprueba.
   - **Sin `.since(...)`**: un método nuevo existe para toda conexión de protocolo 9 o superior. El cliente lo descubre en `hello.methods` (`support::offers`).
3. Atiéndelo en el `match` de `crates/core/src/channel/conn.rs`. Ese dispatch sigue siendo central: añade tu brazo **junto a los de tu módulo**, no al final.
4. Si es un **módulo nuevo** del contrato, crea `methods/<módulo>.rs` con `pub(super) const GROUP: Group = Group { …, ..Group::new("<módulo>") };`. Luego añade en `methods/mod.rs` `mod <módulo>;`, `pub use <módulo>::*;` y `&<módulo>::GROUP` en `GROUPS`.

Si lo que cambia es la **forma** de algo que ya existe (un tipo de evento, un campo o un código nuevo de un método que ya existe), no basta con el método: necesitas una capacidad.

## Añadir una capacidad (cambio de forma)

1. En el archivo de tu módulo:

   ```rust
   /// Git events of kind `cherry-pick`.
   pub const CAP_GIT_CHERRY_PICK: Capability = Capability::new("events.git-cherry-pick");
   // en GROUP:
   capabilities: &[CAP_GIT_RESET, CAP_GIT_CHERRY_PICK],
   ```

2. En el daemon, sirve la forma nueva solo si la conexión la tiene: `self.has(methods::CAP_GIT_CHERRY_PICK.name)` en `conn.rs`. Si cambia lo que la conexión recibe, ajusta `apply_capabilities`.
3. **En el mismo PR**, adapta todos los clientes del repo (CLI, TUI y `raptor-mcp`). Un cliente de `crates/core` pide en `connection.accept` toda capacidad posterior al 9 que conoce. Conocerla significa entenderla.
4. Un daemon más viejo con el mismo protocolo que no la anuncia se reemplaza si quien lo pide es el binario instalado. Si no, el cliente sigue sin ella.

## Añadir un código de error

1. Si tu módulo no tiene bloque todavía, elige el siguiente libre: `-33000`, `-33020`, `-33040`, … Declara `error_block: Some(-33020)` en su `GROUP`. Un test falla si dos módulos comparten bloque, así que si tu PR choca con otro, CI lo dice al rebasar.
2. Declara el código dentro del bloque y regístralo:

   ```rust
   /// The hook layer could not be removed: `data` says why.
   pub const GUARD_UNINSTALL_REFUSED: ErrorSpec = ErrorSpec::new(-33020, "guard-uninstall-refused");
   // en GROUP:
   error_block: Some(-33020),
   errors: &[GUARD_UNINSTALL_REFUSED],
   ```

3. Devuélvelo con `ErrorObject::new(GUARD_UNINSTALL_REFUSED.code, "…")`.
4. Añade su mensaje `error.guard-uninstall-refused` en `apps/cli/i18n/{en,es}/contract.txt`. El test de V8 (`codes.rs`) lo exige para todo código de los módulos.
5. Si lo devuelve un método **que ya existía**, es un cambio de forma: pide una capacidad y responde a quien no la tenga con un código de la lista congelada.

## Añadir un mensaje (i18n)

1. Escribe la clave en el archivo de tu feature, en los dos idiomas: `apps/cli/i18n/en/<feature>.txt` y `apps/cli/i18n/es/<feature>.txt`. Para una feature nueva, crea los dos archivos: `build.rs` los registra solos.
2. Úsala con `crate::i18n::t("<grupo>.<clave>", &[("nombre", &valor)])`.
3. Los tests comprueban que:
   - Los dos idiomas tienen los mismos archivos.
   - Cada archivo tiene las mismas claves y los mismos marcadores `{nombre}` en `en` y en `es`.
   - Ninguna clave aparece dos veces, ni dentro de un archivo ni entre archivos.
   - Cada grupo de claves (`guard.` en `guard.deny.x`) vive en **un solo** archivo. Usa el grupo de tu feature y no añadas claves de otro grupo.

La TUI tiene su propio catálogo tipado en `apps/cli/src/present/i18n.rs`, con un `match` exhaustivo por idioma.

## Añadir un subcomando o una acción

- **Una acción de un subcomando que ya existe** (por ejemplo `raptor guard uninstall`): edita solo `apps/cli/src/commands/guard.rs`. Añade la variante a su enum de acciones y su brazo en `Cmd::run`. La lógica puede vivir en el módulo de la feature (`apps/cli/src/guard.rs`).
- **Un subcomando de primer nivel nuevo**: crea `apps/cli/src/commands/<nombre>.rs` con:

  ```rust
  /// One line of help: it is the subcommand's `about`.
  #[derive(clap::Args)]
  pub(crate) struct Cmd { /* argumentos */ }

  impl Cmd {
      pub(crate) fn run(self, global: &Global) -> ExitCode { /* … */ }
  }
  ```

  Luego añade **una línea** en `commands!` de `commands/mod.rs`, en el lugar que debe ocupar en `--help`. Las utilidades compartidas (`engine`, `error_text`, `refusal_text`, …) están en `support.rs`, reexportadas en la raíz del crate.

## Añadir una operación del catálogo

Para que el ejecutor sirva una operación nueva (`snapshot` fue la primera, US-MCP-008; ADR-CKP-002, Enmienda (2026-10-08, US-MCP-008)):

1. Crea `crates/core/src/executor/ops/<op>.rs` con su brazo: la parte propia de la operación (`plan_op`) y lo que ejecuta.
2. Añade **una línea** en `ops/mod.rs`, en el orden de `OperationId`, para cablear ese brazo en `OperationsWiring::production()`.
3. Una operación **gobernada** (con decisión de Guardrails) no se cablea con `NoGuardrails`: `production()` rechaza al construirse un brazo gobernado mientras la puerta sea `NoGuardrails`. Una no gobernada, como `snapshot`, declara `governed == None`.

## Añadir un módulo del daemon

Si tu feature corre dentro del daemon junto al motor (un hilo, una cola o una captura) y tiene que enterarse de su ciclo de vida:

1. Crea `crates/core/src/daemon/modules/<nombre>.rs` con una función `start(&Daemon) -> Option<Box<dyn DaemonModule>>` y una implementación de `DaemonModule`. Implementa solo los puntos que necesites: `repo_retired`, `observer_hooks`, `git_event` y `stop`. Guarda los handles tipados que necesites (`Arc<…>`), no el `Daemon`.
2. Añade **una línea** a `MODULES` en `modules/mod.rs`. El orden es el orden de cada punto, también el de la parada, y lo fija un test.
3. ¿Necesitas un punto que el trait no tiene? Añádelo al trait con implementación por defecto y llámalo desde `daemon/` **en un solo sitio**. Así cuesta una línea compartida, y una sola vez.

Toda configuración nueva sigue entrando por `DaemonConfig` (`daemon/mod.rs`): es un punto de conflicto que queda pendiente.

## Qué sigue siendo compartido (pendiente)

- El `match` de dispatch de `crates/core/src/channel/conn.rs`.
- `DaemonConfig` y los mensajes `Control` de `daemon/shutdown.rs`.
- Los tipos de `crates/api/src/messages.rs`. Una historia nueva puede poner los suyos en su propio archivo del API, como `guard.rs` y `timemachine.rs`.
