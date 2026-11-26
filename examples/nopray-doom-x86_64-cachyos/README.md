# Transcendence DOOM (`nopray-doom-x86_64-cachyos`)

32 KiB (`32,768` bytes exactos) de código máquina x86-64 escrito a mano en hexadecimal (`DMCS-1.1`, nivel `L2` W^X, `PT_GNU_STACK rw-`, `exposure = "local"`), sin compilador, sin ensamblador, sin `libc` y sin `libX11`.

## Características del motor
- **Resolución:** `640×400` BGRA nativo sobre socket Unix X11/Xwayland (`/tmp/.X11-unix/X0`, 4 franjas `PutImage` de `640×100` a ~60 FPS con `poll` de 16 ms).
- **Syscalls totales (6):** `socket`, `connect`, `read`, `write`, `poll`, `exit`.
- **Gráficos 3D y estética TranscendenceIA:**
  - 7 texturas procedurales `64×64` BGRA generadas al arranque en `.bss` (`0x510000`): paneles de obsidiana con biseles y micro-remaches cian, conductos espectrales 5-bandas (`#ff4b0a → #ff0a8a → #aa35ff → #3867ff → #28d9f7`), reactores ember, monolitos violeta, puertas blindadas con galones de peligro, suelo tech-grid y techo artesonado con luminarias cenitales.
  - Suelo y techo en perspectiva en punto fijo `16.16` + raycaster DDA SSE2 de 640 columnas con `zbuffer` por columna.
  - Niebla de distancia e iluminación dinámica por fogonazo de disparo mediante sombreado SWAR de 256 niveles en 2 multiplicaciones por píxel.
  - 6 sprites procedurales `32×32` RGBA en `.bss` (`0x530000`): Spectral Sentinel (patrulla y telégrafo de ataque), explosión radial, botiquín cian en suelo, célula de munición violeta y bola de plasma.
  - Cañón de plasma bi-raíl en primera persona con *walk-bob*, retroceso, ráfaga de disparo y viñeta de daño/recogida.
  - HUD completo con divisor espectral, tarjetas de `HP`, `AMMO`, `KILLS`, radar minimapa `24×24` conmutable y pantallas de victoria (`SECTOR CLEARED!`) y derrota (`SYSTEM CRITICAL`).
- **Audio PCM en tiempo real sin librerías ni subprocesos:** conexión directa por socket Unix al protocolo nativo de PipeWire/PulseAudio (`/run/user/1000/pulse/native`, `24 kHz S16LE mono`, `400` muestras/frame) manteniendo exactamente las mismas **6 syscalls**. Sintetizador de 5 voces en código máquina puro: disparo de plasma con barrido de frecuencia y ruido, explosiones/impactos con sub-grave, arpegio de recogida/puerta/victoria, alarma de daño y línea de bajo industrial en Mi menor.
- **Jugabilidad:** complejo de `24×24`, 10 Spectral Sentinels con persecución, telégrafo de ataque y proyectiles con colisión contra muros, 3 botiquines, 3 células de munición, puertas blindadas interactivas y combate *hitscan* con oclusión por `zbuffer`.

## Controles
- `W` / `A` / `S` / `D` o `↑` / `↓`: moverse y desplazarse lateralmente
- `←` / `→`, `Q` o ratón: girar cámara
- `Shift_L`: esprintar
- `Space`, `Ctrl_L` o clic izquierdo: disparar cañón de plasma
- `E`: abrir puerta blindada frente al jugador
- `M`: mostrar / ocultar minimapa
- `N` o tecla multimedia `Mute` (`XF86AudioMute`): silenciar / activar audio PCM al instante (indicador `PCM ON N` / `MUTED N` en el HUD)
- `R`: reiniciar nivel
- `Esc`: salir limpiamente (`exit(0)`)

## Verificación
```bash
T=./tools/nopray.sh
$T build  examples/nopray-doom-x86_64-cachyos
$T audit --strict examples/nopray-doom-x86_64-cachyos
$T annotate --check examples/nopray-doom-x86_64-cachyos
```
