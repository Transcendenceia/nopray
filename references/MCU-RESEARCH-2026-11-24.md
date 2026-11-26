# MCU boot formats for hand-authored machine code

Research date: 2026-11-24. No image was built, no device contacted, and no emulator executed. Raw bytes may be authored without an assembler, compiler, linker or SDK; transport tools are separate. **UNVERIFIED** means primary-source confirmation was not obtained here, not that the fact is false. Addresses, encodings and algorithms below are cited to the primary source adjacent to the claim. Security fuses, firmware and individual boards can change boot behavior.

Local PATH inspection found no qemu-system-riscv32, qemu-system-aarch64, simavr, picotool or esptool. Therefore none of the emulator demonstrations below has been run here.

## 1. ESP32-C3

### (a) ISA

- RV32IMC combines 32-bit base instructions with 16-bit compressed instructions; bytes are little-endian. 32-bit NOP is 0x00000013 (bytes 13 00 00 00); compressed NOP is 0x0001 (bytes 01 00). [RISC-V RV32](https://docs.riscv.org/reference/isa/unpriv/rv32.html), [compressed extension](https://docs.riscv.org/reference/isa/unpriv/c-st-ext.html), [C3 datasheet](https://www.espressif.com/sites/default/files/documentation/esp32-c3_datasheet_en.pdf).
- Instruction addresses may be halfword aligned. Branch displacements and mixed-width instructions require careful hand calculation. [RISC-V ISA](https://docs.riscv.org/reference/isa/unpriv/rv32.html).

### (b) ROM boot image

- C3's second-stage bootloader flash offset is 0x0. A ROM-loaded second stage must be directly executable after ROM loading; an ordinary IDF application image at the same offset is not automatically equivalent. [C3 esptool target](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32c3.py), [bootloader documentation](https://docs.espressif.com/projects/esp-idf/en/stable/esp32c3/api-guides/bootloader.html).
- Packed 24-byte esp_image_header_t, little-endian multibyte fields: [official header](https://github.com/espressif/esp-idf/blob/master/components/bootloader_support/include/esp_app_format.h), [esptool parser](https://github.com/espressif/esptool/blob/master/esptool/bin_image.py).

| Byte offset | Size | Field |
|---:|---:|---|
| 0 | 1 | magic 0xE9 |
| 1 | 1 | segment count; IDF maximum 16 |
| 2 | 1 | SPI mode |
| 3 | 1 | flash speed low nibble; size high nibble |
| 4 | 4 | entry address |
| 8 | 1 | WP pin, 0xEE means disabled in IDF |
| 9 | 3 | SPI pin drive |
| 12 | 2 | chip ID, 0x0005 for C3 |
| 14 | 1 | legacy minimum revision; C3 uses minor revision |
| 15 | 2 | minimum full revision, major × 100 + minor |
| 17 | 2 | maximum full revision, major × 100 + minor |
| 19 | 4 | reserved |
| 23 | 1 | hash_appended, 1 if SHA-256 appended |

- Each segment consists of little-endian 32-bit load address, 32-bit data length, then exactly that many data bytes; headers and data are interleaved. Esptool requires segment length divisible by 4. XOR segment data bytes, starting at 0xEF; pad before the checksum so the checksum is the last byte of a 16-byte-aligned image. If hash_appended is 1 append 32 SHA-256 bytes calculated over the image through the checksum, including padding. This hash detects corruption, not secure-boot authenticity. [esptool writer](https://github.com/espressif/esptool/blob/master/esptool/bin_image.py), [header](https://github.com/espressif/esp-idf/blob/master/components/bootloader_support/include/esp_app_format.h), [image-format documentation](https://docs.espressif.com/projects/esptool/en/latest/esp32c3/advanced-topics/firmware-image-format.html).
- Esptool classifies 0x3FC80000–0x3FCE0000 as DRAM and 0x4037C000–0x403E0000 as IRAM, upper bounds exclusive. It also classifies 0x3C000000–0x3C800000 as DROM and 0x42000000–0x42800000 as IROM. Classification does **not** establish which subranges the ROM permits or preserves for a bootloader, nor that mapped flash is ready. Exact safe ROM-loader segment ranges and stack region: **UNVERIFIED**. [C3 target map](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32c3.py), [C3 TRM](https://www.espressif.com/sites/default/files/documentation/esp32-c3_technical_reference_manual_en.pdf).
- Secure boot and flash encryption can impose additional requirements. Header SHA-256 is not a signature. [Secure Boot V2](https://docs.espressif.com/projects/esp-idf/en/stable/esp32c3/security/secure-boot-v2.html).

### (c) Bring-up

- Establish a valid, aligned RAM stack before calling code; RISC-V traps use machine trap state such as mtvec, not Cortex-M's reset-vector table. ROM handoff register contents and default clocks: **UNVERIFIED**. [RISC-V privileged architecture](https://docs.riscv.org/reference/isa/priv/machine.html).
- RTC WDT: write 0x50D83AA1 to 0x600080A8 to unlock; clear enable bit 31 of configuration register 0x60008090; write 0 to 0x600080A8 to relock. Esptool itself uses a whole-register zero in its reset path; read-modify-write preserves unrelated bits. [C3 esptool](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32c3.py), [RTC register definitions](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/rtc_cntl_reg.h).
- Super WDT: unlock 0x600080B0 with 0x8F1D312A; control 0x600080AC bit 30 is disable, bit 31 is automatic feed; relock with zero. Esptool demonstrates setting auto-feed, not a complete guarantee that disabling is appropriate under every boot condition. [esptool target](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32c3.py), [RTC registers](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/rtc_cntl_reg.h).
- Timer-group MWDT: configuration offset 0x48, enable bit 31, write-protect offset 0x64. Exact absolute bases, write-protect unlock key and initial state: **UNVERIFIED**. Do not substitute RTC addresses or presume all watchdogs are active. [timer-group register definitions](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/timer_group_reg.h), [watchdog overview](https://docs.espressif.com/projects/esp-idf/en/stable/esp32c3/api-reference/system/wdts.html).

### (d) UART and GPIO

- UART0 FIFO 0x60000000; status 0x6000001C, TX FIFO occupancy bits 25:16. This is a count, not a verified single full flag. FIFO capacity and a safe full threshold: **UNVERIFIED**. Establish baud, pin routing and clock before expecting output. [UART registers](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/uart_reg.h), [register bases](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/reg_base.h).
- GPIO base 0x60004000: for valid output pin N, write 1<<N to output-enable W1TS 0x60004024, then to output W1TS 0x60004008 for high or W1TC 0x6000400C for low. Pin mux and GPIO-matrix settings must first be correct; exact full pin-specific mux sequence and board LED pin are **UNVERIFIED**. Some boards do not have a directly connected LED. [GPIO registers](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/gpio_reg.h), [register bases](https://github.com/espressif/esp-idf/blob/master/components/soc/esp32c3/register/soc/reg_base.h).

### (e) Verification

- Offline structural candidate: esptool --chip esp32c3 image-info image.bin. Check the installed version's help; parsing cannot establish real-ROM execution. [esptool commands](https://docs.espressif.com/projects/esptool/en/latest/esp32c3/esptool/basic-commands.html).
- Espressif QEMU fork C3 machine name, ROM requirements, exact command and peripheral fidelity: **UNVERIFIED**. Do not assume -M esp32c3 exists in any particular build. [Espressif QEMU](https://github.com/espressif/qemu).
- Hardware example after identifying the target and ensuring recoverability: esptool --chip esp32c3 --port PORT write-flash 0x0 image.bin. Not executed. Baud and framing after this custom image: **UNVERIFIED** unless configured by the image. [esptool documentation](https://docs.espressif.com/projects/esptool/en/latest/esp32c3/esptool/basic-commands.html).

### (f) C6 and risks

- C6 is not a C3 binary or register-map drop-in. Image chip ID is 0x000D, versus C3 0x0005. C6 ISA extensions, boot offset, clocks, watchdog and UART specifics: **UNVERIFIED** here. [image header](https://github.com/espressif/esp-idf/blob/master/components/bootloader_support/include/esp_app_format.h), [C6 target](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32c6.py).
- Writing at 0x0 replaces the second stage, not just an app; serial recovery can also be restricted by device security configuration. [startup flow](https://docs.espressif.com/projects/esp-idf/en/stable/esp32c3/api-guides/startup.html).

## 2. Raspberry Pi 3, 4 and 5 bare-metal AArch64

### (a) ISA

- A64 instructions are 32-bit; NOP is 0xD503201F, bytes 1F 20 03 D5 in little-endian instruction memory. [Arm A64 NOP reference](https://developer.arm.com/documentation/ddi0602/latest/Base-Instructions/NOP--No-Operation-).

### (b) Firmware image

- kernel8.img is firmware-loaded raw kernel content, not inherently an ELF. Firmware exposes kernel, arm_64bit, kernel_address and enable_uart configuration options, with firmware/model-dependent behavior. [Raspberry Pi config.txt documentation](https://www.raspberrypi.com/documentation/computers/config_txt.html).
- Exact default load address per model, Pi 5 filename and placement, entry EL, initial register state, and which secondary cores run or wait: **UNVERIFIED** here. Do not assume a commonly cited address works with every firmware/configuration. [boot documentation](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#boot-sequence).
- Raw binary has no ELF program headers; firmware-specific validation and boot-medium contents are distinct questions. Image header, checksum and alignment requirements across all firmware versions: **UNVERIFIED**. [configuration](https://www.raspberrypi.com/documentation/computers/config_txt.html).

### (c) Bring-up

- Establish valid aligned per-core stacks, exception vectors as needed, and the desired EL before using privileged functionality. Watchdog state and clock defaults for each model: **UNVERIFIED**. [Arm architecture reference](https://developer.arm.com/documentation/ddi0487/latest/).

### (d) Output

- Pi 3 uses BCM2837, Pi 4 BCM2711, and Pi 5 uses RP1 for much of its I/O. UART selection, addresses, GPIO routing and clocking are not interchangeable. [BCM2835/2837 peripheral reference](https://datasheets.raspberrypi.com/bcm2835/bcm2835-peripherals.pdf), [BCM2711](https://datasheets.raspberrypi.com/bcm2711/bcm2711-peripherals.pdf), [RP1](https://datasheets.raspberrypi.com/rp1/rp1-peripherals.pdf).
- Per-model PL011 and mini-UART physical bases, Pi 5 RP1 translation, TX register/full bit, complete GPIO LED sequence and LED polarity: **UNVERIFIED**. enable_uart alone does not establish the physical header routing. [UART configuration](https://www.raspberrypi.com/documentation/computers/configuration.html#configuring-uarts).

### (e) Verification / (f) pitfalls

- QEMU documents raspi3b with four Cortex-A53 cores and 1 GiB, and raspi4b with four Cortex-A72 cores and 2 GiB. Pi 4 emulation lacks some devices, including PCIe root complex and GENET. [QEMU Raspberry Pi board documentation](https://www.qemu.org/docs/master/system/arm/raspi.html).
- Candidate: qemu-system-aarch64 -M raspi3b -kernel kernel8.img -nographic -serial mon:stdio. A newer build may offer raspi4b instead. Exact binary placement, entry EL, secondary cores and UART for this invocation: **UNVERIFIED**. A QEMU direct-loader test is not an SD firmware boot test. [QEMU invocation](https://www.qemu.org/docs/master/system/invocation.html), [board documentation](https://www.qemu.org/docs/master/system/arm/raspi.html).
- Hardware: use the matching image and config.txt on a correctly prepared boot partition and a correctly wired 3.3 V serial adapter. Exact model-specific boot partition, firmware setup and console baud/framing: **UNVERIFIED**. [Raspberry Pi boot sequence](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#boot-sequence).

## 3. Raspberry Pi OS arm64 Linux userland

### (a) ISA / (b) ELF

- A valid static ELF64 executable with EM_AARCH64 can run on arm64 Linux without any compiler or assembler. It still needs coherent ELF and PT_LOAD program headers, permissions, an executable entry and appropriate kernel support. Static means no dynamic interpreter/dependencies, not no ELF headers. [Arm ELF ABI](https://github.com/ARM-software/abi-aa/blob/main/aaelf64/aaelf64.rst), [Linux ELF loader](https://github.com/torvalds/linux/blob/master/fs/binfmt_elf.c).
- Exact minimal ELF header and offsets for a particular design: **UNVERIFIED** here. No ROM image, bare-metal vector table or flash offset applies.

### (c) Bring-up / (d) output

- Linux prepares the process stack. The AArch64 Linux syscall ABI uses svc #0, x8 for syscall number, x0–x5 for arguments, and x0 for return. Use write to stdout rather than directly accessing UART MMIO. [Linux arm64 nolibc ABI](https://github.com/torvalds/linux/blob/master/tools/include/nolibc/arch-arm64.h).
- Page size depends on the running kernel, not the Pi 5 hardware alone; query getconf PAGESIZE. A specific Pi 5 OS image using 16 KiB pages is **UNVERIFIED** unless that image or kernel is identified. PT_LOAD file offsets and virtual addresses need required ELF and kernel-page congruence; check the actual loader. [ELF program header specification](https://gabi.xinuos.com/elf/07-pheader.html), [Linux loader](https://github.com/torvalds/linux/blob/master/fs/binfmt_elf.c).

### (e) Verification / (f) pitfalls

- Candidate native inspection and execution: file image; readelf -h -l image; ./image. QEMU user-mode candidate: qemu-aarch64 ./image. Neither execution nor availability of qemu-aarch64 here was established. No flash tool or serial console is required. [QEMU user mode](https://www.qemu.org/docs/master/user/main.html).

## 4. RP2040 / RP2350

### (a) ISA

- RP2040 Cortex-M0+ executes Thumb instructions, including 16-bit and 32-bit encodings. NOP is 0xBF00, bytes 00 BF. [RP2040 datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf), [Cortex-M0+ guide](https://developer.arm.com/documentation/dui0662/latest/).

### (b) Boot2 and UF2

- RP2040 Boot ROM loads and checks a 256-byte second-stage block from external flash, then executes it. The checksum is part of the 256 bytes; the second stage must initialize the specific flash / XIP path. [RP2040 datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf).
- Exact ROM CRC polynomial, initialization, reflection, final XOR, coverage and stored byte order: **UNVERIFIED** here. Generic CRC32 defaults are not interchangeable without validation. [RP2040 datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf), [Pico SDK source](https://github.com/raspberrypi/pico-sdk).
- Cortex-M vector-table word 0 is initial MSP and word 1 reset-handler address with Thumb state bit set. Exact application vector address and boot2 transfer procedure: **UNVERIFIED**. [Cortex-M0+ guide](https://developer.arm.com/documentation/dui0662/latest/), [RP2040 datasheet](https://datasheets.raspberrypi.com/rp2040/rp2040-datasheet.pdf).
- UF2 is a flash transport wrapper, not executable instructions. Fields are little-endian; each block has 512 bytes. [UF2 specification](https://github.com/microsoft/uf2/blob/master/README.md).

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | magic 0x0A324655 |
| 4 | 4 | magic 0x9E5D5157 |
| 8 | 4 | flags; 0x00002000 means family ID present |
| 12 | 4 | target address |
| 16 | 4 | payload size |
| 20 | 4 | block number |
| 24 | 4 | total number of blocks |
| 28 | 4 | family ID when flag set |
| 32 | 476 | data area, only payload-size bytes meaningful |
| 508 | 4 | end magic 0x0AB16F30 |

- RP2040 family ID 0xE48BFF56. Payload size and target address should be 4-byte aligned; 256-byte payload is a common choice. [UF2 spec](https://github.com/microsoft/uf2/blob/master/README.md), [family list](https://github.com/microsoft/uf2/blob/master/utils/uf2families.json).

### (c) Bring-up / (d) output

- Watchdog base 0x40058000. Initial state, exact disable/feed sequence and clock defaults: **UNVERIFIED**. [RP2040 address map](https://github.com/raspberrypi/pico-sdk/blob/master/src/rp2040/hardware_regs/include/hardware/regs/addressmap.h), [watchdog definitions](https://github.com/raspberrypi/pico-sdk/blob/master/src/rp2040/hardware_regs/include/hardware/regs/watchdog.h).
- SIO base 0xD0000000: write 1<<N to 0xD0000024 (GPIO OE set), then 0xD0000014 (output set) or 0xD0000018 (output clear). First configure the pin for SIO; exact pin mux writes and LED wiring are **UNVERIFIED**, so these writes alone are not a complete LED recipe. [address map](https://github.com/raspberrypi/pico-sdk/blob/master/src/rp2040/hardware_regs/include/hardware/regs/addressmap.h), [SIO definitions](https://github.com/raspberrypi/pico-sdk/blob/master/src/rp2040/hardware_regs/include/hardware/regs/sio.h).
- UART0 data 0x40034000; flag 0x40034018; TX FIFO full mask 0x20 (bit 5). Wait until clear before writing a byte. Reset release, clock, baud and mux must also be configured; exact sequence **UNVERIFIED**. [address map](https://github.com/raspberrypi/pico-sdk/blob/master/src/rp2040/hardware_regs/include/hardware/regs/addressmap.h), [UART definitions](https://github.com/raspberrypi/pico-sdk/blob/master/src/rp2040/hardware_regs/include/hardware/regs/uart.h).

### (e) Verification / (f) RP2350

- RP2040 emulator name, usable command and fidelity are **UNVERIFIED**. Hardware path: enter BOOTSEL and copy a correctly addressed UF2 onto the exposed USB mass-storage volume; picotool is another option, but exact version-specific command is **UNVERIFIED**. Serial settings depend on the image. [Pico documentation](https://www.raspberrypi.com/documentation/microcontrollers/raspberry-pi-pico.html), [picotool](https://github.com/raspberrypi/picotool).
- RP2350 supports Arm and RISC-V Hazard3 options and has a different boot/image model involving picobin blocks and IMAGE_DEF. Do not assume RP2040 boot2 or family ID applies. Exact IMAGE_DEF fields, marker values, family IDs, security and offsets: **UNVERIFIED**. [RP2350 datasheet](https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf), [picobin headers](https://github.com/raspberrypi/pico-sdk/tree/master/src/common/boot_picobin_headers).

## 5. ESP32 classic / ESP32-S3

- Xtensa LX6/LX7 variable-width instruction encoding, windowed calling conventions and literal handling make raw authoring more involved. Exact width combinations, NOP bytes, endian caveats and call0/windowed setup: **UNVERIFIED** here. [ESP32 datasheet](https://www.espressif.com/sites/default/files/documentation/esp32_datasheet_en.pdf), [ESP32-S3 datasheet](https://www.espressif.com/sites/default/files/documentation/esp32-s3_datasheet_en.pdf).
- Official shared header defines classic chip ID 0x0000, S3 0x0009, C3 0x0005. Similar container fields do not make code, ROM acceptance rules, memory mappings or boot offsets interchangeable. Exact classic and S3 boot offsets, format-specific differences, watchdog, UART and GPIO sequences: **UNVERIFIED** here. [image header](https://github.com/espressif/esp-idf/blob/master/components/bootloader_support/include/esp_app_format.h), [classic esptool target](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32.py), [S3 target](https://github.com/espressif/esptool/blob/master/esptool/targets/esp32s3.py).
- Emulator commands, flashing parameters and serial settings: **UNVERIFIED**. [Espressif QEMU](https://github.com/espressif/qemu), [esptool documentation](https://docs.espressif.com/projects/esptool/en/latest/).

## 6. ATmega328P / Arduino Uno

- AVR instructions are word-oriented, with some instructions occupying two words. NOP encoding, byte ordering and exact reset/boot fuse behavior: **UNVERIFIED** here. [ATmega328P datasheet](https://ww1.microchip.com/downloads/en/DeviceDoc/Atmel-42735-8-bit-AVR-Microcontroller-ATmega328-328P_Datasheet.pdf), [AVR instruction-set manual](https://onlinedocs.microchip.com/oxy/GUID-0B644D8F-67E7-49E6-82C9-1B2B9ABE6A0D-en-US-23/index.html).
- Intel HEX comprises ASCII records with byte count, address, type, data and checksum; exact checksum and extended-address rules: **UNVERIFIED** here. HEX is programmer input, not a ROM-consumed execution header. [Microchip HEX documentation](https://onlinedocs.microchip.com/oxy/GUID-C3F66E96-7CDD-47A0-9AB7-9068BADB46C0-en-US-4/GUID-DF9E479D-6BA8-49E3-A2A5-997BBA49D34D.html).
- Stack, clocks/fuses, watchdog, UART and GPIO register values and LED wiring: **UNVERIFIED**. [ATmega328P datasheet](https://ww1.microchip.com/downloads/en/DeviceDoc/Atmel-42735-8-bit-AVR-Microcontroller-ATmega328-328P_Datasheet.pdf).
- simavr exists, but its exact command for this image, emulator fidelity, hardware programmer command and console parameters are **UNVERIFIED**; simavr is absent locally. [simavr](https://github.com/buserror/simavr).

## Comparison

Difficulty is an editorial estimate. “Here” refers to this host and this research session.

| Target | Can verify in emulator here? | Emulator | Hardware flash tool | Difficulty 1–5 | Key risks |
|---|---|---|---|---:|---|
| ESP32-C3 | No; unavailable and machine unverified | Espressif QEMU fork, C3 support unverified | esptool | 4 | ROM-reserved RAM, watchdogs, secure boot |
| Pi 3 / 4 bare metal | No; QEMU absent | QEMU raspi3b / raspi4b | SD boot partition | 4 | firmware vs QEMU entry, MMIO mapping |
| Pi 5 bare metal | No; emulator model unverified | Unverified | SD / EEPROM boot flow | 5 | RP1 mapping and firmware handoff |
| Pi arm64 Linux | No emulator verified; native architecture not established | qemu-aarch64 | Not applicable | 2 | ELF segment layout, page size |
| RP2040 | No emulator verified | Unverified | UF2 BOOTSEL / picotool | 4 | boot2 CRC, XIP and GPIO mux |
| RP2350 | No emulator verified | Unverified | UF2 / picotool, exact workflow unverified | 5 | IMAGE_DEF, architecture, security |
| ESP32 / S3 | No; fork availability unverified | Espressif QEMU fork, targets unverified | esptool, command unverified | 5 | Xtensa encoding and boot mapping |
| ATmega328P | No; simavr absent | simavr | Programmer / bootloader, command unverified | 3 | fuses, vector and HEX checksum |
