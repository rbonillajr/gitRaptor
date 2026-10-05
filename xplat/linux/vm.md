---
title: Receta de VM Linux para lo que el contenedor no cubre
status: draft
generated: 2026-10-05
generator: orquestador
domain: GRP
tags: [xplat, linux, vm, lima, utm, systemd, polkit, inotify, max_user_watches, validacion-multiplataforma]
related: [ADR-GRP-005, ADR-GRP-010, ADR-GRD-008, SPIKE-GRD-002, SPIKE-GRP-002, US-GRP-002, TS-GRP-003]
---

# VM Linux para la etapa de validación multiplataforma

El contenedor de `xplat/run-linux.sh` cubre la suite de Rust, `repo_intact` con `strace` y la matriz de versiones de Git. **No** cubre lo que depende del sistema completo:

| Qué | Por qué no en el contenedor | Origen |
|---|---|---|
| Servicio de usuario `systemd --user` (autoarranque del daemon, reinicio ante fallo, `PATH` mínimo) | El contenedor no arranca systemd ni una sesión de usuario con `loginctl` | ADR-GRP-005, TS-GRP-003 |
| polkit: `CheckAuthorization` con interacción y un agente de autenticación gráfico | No hay bus de sistema, ni `polkitd`, ni sesión gráfica con agente | ADR-GRD-008, SPIKE-GRD-002 |
| Agotamiento de `fs.inotify.max_user_watches`, `IN_Q_OVERFLOW` y modo degradado | `max_user_watches` es un sysctl del kernel de la VM de Docker Desktop, compartido con todo lo demás; bajarlo desde un contenedor exige `--privileged` y afecta al resto | ADR-GRP-010, SPIKE-GRP-002 |

## Elección

> **Decisión del orquestador (2026-10-04), validada por Arquitecto:** dos VMs con papeles distintos, ambas Ubuntu 24.04 arm64 en este Mac (Apple Silicon).
>
> - **Lima** (sin interfaz, scriptable) para `systemd --user` y `max_user_watches`. Se crea y se destruye con un comando, y su kernel es solo suyo, así que se pueden bajar los límites de inotify sin tocar Docker.
> - **UTM con Ubuntu Desktop** para polkit, que necesita una sesión gráfica real con su agente de autenticación (GNOME Shell lo trae).

Las dos se clonan el repo dentro de la VM, igual que el contenedor: **nunca** se trabaja sobre el directorio del Mac montado (inotify no ve los cambios hechos desde el host y los permisos no son los reales).

## 1. Lima: systemd del usuario e inotify

### Crear la VM

```sh
brew install lima
limactl start --name=gitraptor-linux --cpus=4 --memory=6 --mount-none template://ubuntu-24.04
limactl shell gitraptor-linux
```

`--mount-none` evita el montaje del `$HOME` del Mac que Lima hace por defecto.

### Preparar la VM (dentro)

```sh
sudo apt-get update
sudo apt-get install -y build-essential git strace pkg-config libssl-dev curl
curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
. ~/.cargo/env
git clone https://github.com/<org>/gitRaptor.git ~/gitRaptor   # o un bundle copiado con limactl copy
cd ~/gitRaptor && cargo build --workspace
```

Para probar una rama sin publicarla: en el Mac, `git bundle create /tmp/gr.bundle HEAD`, `limactl copy /tmp/gr.bundle gitraptor-linux:/tmp/` y `git clone /tmp/gr.bundle ~/gitRaptor` dentro.

### Servicio `systemd --user` (ADR-GRP-005)

Lima arranca con systemd y con una sesión de usuario (`loginctl show-user $USER` muestra `Linger` y `State=active`). Comprobaciones:

1. Registrar la unidad como lo hará el instalador (`~/.config/systemd/user/gitraptor.service`, `WantedBy=default.target`, `Restart=on-failure`) y `systemctl --user daemon-reload && systemctl --user enable --now gitraptor`.
2. `systemctl --user show -p Environment gitraptor` y `/proc/<pid>/environ`: el `PATH` es el mínimo de systemd; el daemon debe resolver Git por las ubicaciones conocidas (`/usr/bin/git`), no por el `PATH` de la shell.
3. `kill -9 <pid>` → systemd lo reinicia; `systemctl --user stop` → parada ordenada por `SIGTERM` (TS-GRP-003, D3).
4. Sin autoarranque (`systemctl --user disable --now gitraptor`): el arranque bajo demanda desde `raptor` sigue funcionando.
5. Cierre de sesión (`loginctl terminate-user $USER` desde otra sesión, sin linger): el daemon se detiene y el siguiente `raptor` lo arranca bajo demanda.

### `max_user_watches` (ADR-GRP-010)

```sh
cat /proc/sys/fs/inotify/max_user_watches          # valor de partida
sudo sysctl fs.inotify.max_user_watches=512         # límite bajo, solo en esta VM
# repo temporal con más directorios no ignorados que el límite
tmp=$(mktemp -d) && cd "$tmp" && git init -q r && cd r && mkdir -p $(seq -f 'd%g/x' 1 800)
```

Con el daemon observando ese repo hay que ver: el aviso de límite estimado antes de registrar los watches, el modo degradado de **ese** repo sin que caigan los demás y la recuperación al subir el límite (`sudo sysctl fs.inotify.max_user_watches=65536`). Para `IN_Q_OVERFLOW`, bajar `fs.inotify.max_queued_events` (por ejemplo a 64) y generar ráfagas de escrituras. Al terminar, `sudo sysctl --system` restaura los valores.

### Cerrar

```sh
limactl stop gitraptor-linux && limactl delete gitraptor-linux
```

## 2. UTM con Ubuntu Desktop: polkit (ADR-GRD-008)

1. `brew install --cask utm` y descargar la ISO de **Ubuntu 24.04 Desktop arm64**.
2. En UTM: *Create a New Virtual Machine → Virtualize → Linux*, 4 CPU, 6 GB, 40 GB de disco, la ISO como arranque. Instalar con un usuario normal **del grupo `sudo`** y otro **sin** privilegios para probar los dos casos de `auth_self`.
3. Tras instalar: `sudo apt-get install -y spice-vdagent build-essential git curl pkg-config libssl-dev libdbus-1-dev`, rustup y el clon del repo como en Lima.
4. Comprobar el agente: `pgrep -a polkit` (debe aparecer `polkitd`) y el agente de GNOME Shell en la sesión gráfica (`busctl --system list | grep -i polkit`).

Lo que se valida aquí (pendiente de ADR-GRD-008 § Validación; la implementación es de US-GRD-013 y US-GRD-015):

- `CheckAuthorization` con la acción propia (`auth_self`, interacción permitida) desde el daemon arrancado bajo demanda **y** desde `systemd --user`: qué agente atiende, qué sujeto y sesión ve polkit.
- Fail-closed: sin sesión gráfica (por SSH a la VM), sin agente (`pkill -f polkit-gnome` o equivalente) o con el diálogo cancelado, la acción reservada **no** se ejecuta.
- El diálogo lo abre el daemon, no el proceso del agente de IA.

## Registro de resultados

Cada pasada se anota en `docs/architecture/xplat-pendientes.md` (columna Estado) con la fecha, la versión de Ubuntu y del kernel, y el commit probado. Lo que no se pudo verificar se deja como "no verificado", nunca como "pasa".
