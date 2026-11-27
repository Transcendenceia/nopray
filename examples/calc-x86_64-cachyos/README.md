# Referencia #001 (`NOPRAY`) — `Calc x86-64`: Calculadora Gráfica Completa en 4,096 Bytes de Código Máquina Puro

Implementación de referencia oficial del estándar **NOPRAY (`NOPRAY-1.0` / `DMCS-1.0`, Nivel `DMCS-L1`)**, creada y verificada en vivo sobre **CachyOS Linux (`7.3.5-2-cachyos x86_64`) + Hyprland + Xwayland**.

---

## Certificado de Cumplimiento `DMCS-1.0`

```text
Standard:       DMCS-1.0 (Tier L1 — Pure Raw Hex + Audited Disassembly & Evidence)
Target ISA/OS:  x86_64 / Linux (ELF64 static executable)
Binary Size:    4,096 bytes (0x1000 — exact single OS memory page, matches p_filesz)
Memory RSS:     ~572 KB (includes 320x440x4 = 563,200 B software framebuffer in .bss)
SHA-256:        93227890dd57f148b30d3f77855097b74201504017a062e917cff4d371879db6
Dynamic Libs:   0 (ldd: "not a dynamic executable", no libc, no libX11, no libm)
Build Tool:     xxd -r -p calc.hex calc (zero compilers, zero assemblers, zero linkers)
Syscall Set:    read(0), write(1), socket(41), connect(42), exit(60)
Verified On:    Cachyos-x8664 (Linux 7.3.5-2-cachyos, Hyprland + Xwayland :0), 2026-11-24
```

---

## Qué Contiene Exactamente Esta Única Página de 4,096 Bytes

### Mapa de Memoria Virtual (`VADDR_BASE = 0x400000`)

| Rango Virtual | Offset Archivo | Tamaño Útil / Ranura | Contenido |
| :--- | :--- | :--- | :--- |
| `0x400000–0x40003F` | `0x000–0x03F` | `64 B / 64 B` | Cabecera **ELF64** (`ET_EXEC`, `EM_X86_64`, `e_entry = 0x400100`) |
| `0x400040–0x40007F` | `0x040–0x07F` | `56 B / 64 B` | Único Program Header **`PT_LOAD`** (`filesz = 0x1000`, `memsz = 0xA0000`, `RWE`) |
| `0x400080–0x4000AF` | `0x080–0x0AF` | `31 B / 48 B` | `read_exact(fd, buf, len)` — bucle garantizado sobre `SYS_read (0)` |
| `0x4000B0–0x4000DF` | `0x0B0–0x0DF` | `26 B / 48 B` | `write_all(fd, buf, len)` — bucle garantizado sobre `SYS_write (1)` |
| `0x4000E0–0x4000FF` | `0x0E0–0x0FF` | `9 B / 32 B` | `exit_clean()` — `SYS_exit (60)` con código `0` |
| `0x400100–0x4002EF` | `0x100–0x2EF` | `488 B / 496 B` | **`_start`**: fija `r15 = 0x402000`, conecta `/tmp/.X11-unix/X0`, handshake X11, extrae `res_base`/`root_win`/`root_visual`/`root_depth`, envía `CreateWindow` (`320×440`), `WM_NAME` (`"Calc x86-64"`), `WM_NORMAL_HINTS` (`min=max=320×440` para flotado automático en Hyprland), `CreateGC`, `MapWindow` y render inicial |
| `0x4002F0–0x40041F` | `0x2F0–0x41F` | `255 B / 304 B` | **`event_loop`**: atiende `Expose (12)`, `ButtonPress (4)` con hit-testing `4×5`, `ButtonRelease (5)` y `KeyPress (2)` con detección de `Shift` y soporte de teclado español + US + bloque numérico |
| `0x400420–0x4005D7` | `0x420–0x5D7` | `433 B / 440 B` | **`handle_action(btn_idx)`**: máquina de estados completa (`C`, `CE`, `0–9`, `.`, `+ - × ÷`, `=`, `±`, `%`, encadenamiento) |
| `0x4005D8–0x40067F` | `0x5D8–0x67F` | `156 B / 168 B` | **`apply_op()`**: aritmética de punto fijo con signo de 64 bits (`×10000`), productos/cocientes de 128 bits (`rdx:rax`) y guardas explícitas anti-overflow y anti-división por cero (cero trampas `#DE`) |
| `0x400680–0x4007FF` | `0x680–0x7FF` | `207 B / 384 B` | **`format_display()`**: convierte entero escalado `×10000` a glifos con supresión limpia de ceros decimales a la derecha, punto decimal vivo al escribir, signo negativo y estado `ERR` |
| `0x400800–0x40087F` | `0x800–0x87F` | `44 B / 128 B` | **`fill_rect(x, y, w, h, color)`**: relleno rápido con `rep stosd` en framebuffer `0x408000` |
| `0x400880–0x40095F` | `0x880–0x95F` | `93 B / 224 B` | **`draw_glyph(x, y, idx, scale, color)`**: rasterizador vectorial por escala entera sobre fuente bitmap `5×7` |
| `0x400960–0x400BFF` | `0x960–0xBFF` | `573 B / 672 B` | **`render_and_blit()`**: pinta fondo `#181825`, marco OLED `#11111B`, insignia de operador pendiente `#FAB387`, display auto-escalado (`4×` hasta 10 dígitos, `3×` hasta 14), 20 botones con esquinas recortadas y highlight al pulsar, y blitea en 4 franjas X11 `PutImage` (`320×110`) |
| `0x400C00–0x400FFF` | `0xC00–0xFFF` | `1,024 B` | **`.rodata`**: `sockaddr_un`, paquete setup X11, `"Calc x86-64"`, `WM_SIZE_HINTS` (72 B), fuente bitmap `5×7` (22 glifos = 154 B), tablas de glifos/tipos/valores/colores de los 20 botones y tabla de 43 atajos de teclado |
| `0x402000–0x49FFFF` | *(zero-fill)* | `640 KiB` | **`.bss`** (creada a cero por el kernel): estado (`0x402000`), búfer de eventos (`0x402080`), búfer de peticiones X11 (`0x402100`), respuesta de setup (`0x402200`) y framebuffer BGRA (`0x408000`, `563,200 B`) |

---

## Diseño Visual y Controles

### Cuadrícula `4×5` (`320×440`, Paleta Catppuccin Mocha)

| Fila | Col 0 | Col 1 | Col 2 | Col 3 |
| :--- | :---: | :---: | :---: | :---: |
| **0** | `C` (`#5C2A3A` / `#F38BA8`) | `CE` (`#45475A`) | `%` (`#45475A`) | `÷` (`#E08E45`) |
| **1** | `7` (`#313244`) | `8` (`#313244`) | `9` (`#313244`) | `×` (`#E08E45`) |
| **2** | `4` (`#313244`) | `5` (`#313244`) | `6` (`#313244`) | `-` (`#E08E45`) |
| **3** | `1` (`#313244`) | `2` (`#313244`) | `3` (`#313244`) | `+` (`#E08E45`) |
| **4** | `±` (`#313244`) | `0` (`#313244`) | `.` (`#313244`) | `=` (`#2FA87A`) |

### Atajos de Teclado Soportados
- **Dígitos:** fila superior `0–9` y bloque numérico `KP_0–KP_9`
- **Decimal:** `.`, `,` y `KP_Decimal`
- **Operadores:** `+` / `KP_Add`, `-` / `KP_Subtract`, `*` / `x` / `KP_Multiply`, `/` (`Shift+7` en ES) / `KP_Divide`, `%` (`Shift+5`)
- **Calcular:** `Enter`, `KP_Enter`, `=` (`Shift+0` en ES y `=` en US)
- **Limpiar / Salir:** `Backspace` (`CE`), `Esc` o `c` (`C`), `q` (cerrar limpio con `exit(0)`)

---

## Evidencia Verificada en Vivo (CachyOS + Hyprland)

Todas las capturas fueron tomadas en tiempo real directamente desde el compositor Wayland/Hyprland con `grim -g "100,100 320x440"` tras pulsar botones reales con ratón y teclado (`xdotool`):

| Prueba | Secuencia Ejecutada | Resultado Observado | Captura |
| :--- | :--- | :--- | :--- |
| **Estado inicial** | Arranque directo `./calc` | Ventana flotante `320×440`, display `0` | [`evidence/calc_init.png`](./evidence/calc_init.png) |
| **Suma por ratón** | Clic en `1` `2` `+` `3` `4` `=` | `46` | [`evidence/calc_46.png`](./evidence/calc_46.png) |
| **Decimales + producto** | Clic en `1` `.` `5` `×` `2` `.` `5` `=` | `3.75` | [`evidence/calc_375.png`](./evidence/calc_375.png) |
| **División no entera** | Clic en `7` `÷` `2` `=` | `3.5` | [`evidence/calc_35.png`](./evidence/calc_35.png) |
| **Encadenado + insignia** | Teclado `c 5 + 3 *` | `8` + insignia `×` activa arriba a la izquierda | [`evidence/calc_chain_8x.png`](./evidence/calc_chain_8x.png) |
| **Resultado negativo** | Continuación `2 Enter - 3 1 Enter` | `-15` | [`evidence/calc_neg15.png`](./evidence/calc_neg15.png) |
| **Protección división/0** | Clic en `5` `÷` `0` `=` | `ERR` en coral `#F38BA8` (sin crash `#DE`) | [`evidence/calc_err.png`](./evidence/calc_err.png) |

---

## Replicación y Auditoría Independiente (En Cualquier Linux `x86_64` con X11/Xwayland)

```bash
# 1. Materializar el ejecutable directamente desde el hexadecimal crudo
xxd -r -p calc.hex calc
chmod +x calc

# 2. Verificar integridad y pureza (0 librerías, 4096 bytes exactos)
wc -c calc
sha256sum calc
# Esperado: 93227890dd57f148b30d3f77855097b74201504017a062e917cff4d371879db6
file calc
ldd calc
readelf -h -l calc

# 3. Auditar las 5 únicas syscalls en todo el ejecutable
objdump -D -b binary -m i386:x86-64 -M intel --adjust-vma=0x400000 \
  --start-address=0x400080 --stop-address=0x400ba0 calc | grep -B 4 'syscall'

# 4. Ejecutar
DISPLAY=:0 ./calc
```

### Archivos de Esta Referencia
- [`calc.hex`](./calc.hex) — Los 4,096 bytes exactos en hexadecimal crudo.
- [`slots/`](./slots/) — Desensamblado anotado por ranura, regenerado desde los bytes con `nopray annotate`.
- [`build/MAP.md`](./build/MAP.md) — Mapa generado por `nopray audit`: ranuras, contratos y sitios de syscall.
- [`evidence/`](./evidence/) — Las 7 capturas reales de validación sobre Hyprland.

---

## Mapa NOPRAY (`DMCS-1.1`)

Este directorio es ahora una app mapeada: `nopray.toml` declara segmentos, ranuras, contratos y
syscalls; `slots/` contiene los bytes anotados; `build/MAP.md` es el mapa navegable.
`calc.hex` se conserva como referencia histórica y es idéntico byte a byte a la versión mapeada.

- Nivel: L1 (una sola PT_LOAD RWX por diseño: solo uso local).
- Auditoría: 0 errores; avisos: segmento RWX (S001), sin PT_GNU_STACK (S003) y caída de `_start` en `event_loop` (C015), heredados del diseño original.
- Reproducir: `../../tools/nopray.sh audit .` (escribe `build/audit.json` y `build/MAP.md`).
