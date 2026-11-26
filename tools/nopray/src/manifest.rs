//! `nopray.toml`: the authoritative map of a NOPRAY application.
//!
//! The manifest declares where every byte lives (segments and slots), what each slot is for,
//! which entry points other slots may call, which syscalls or MMIO windows the program may
//! touch, and how exposed the program is. `nopray build` and `nopray audit` treat any
//! disagreement between this map and the bytes as an error.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// File name of the manifest inside an application directory.
pub const MANIFEST_FILE: &str = "nopray.toml";

/// Parsed and validated application manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub app: App,
    pub target: Target,
    #[serde(default, rename = "segment")]
    pub segments: Vec<Segment>,
    #[serde(rename = "slot")]
    pub slots: Vec<Slot>,
    #[serde(default)]
    pub syscalls: Option<Syscalls>,
    #[serde(default, rename = "mmio")]
    pub mmio: Vec<Mmio>,
    #[serde(default, rename = "state")]
    pub state: Vec<StateField>,
    #[serde(default)]
    pub provenance: Option<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub name: String,
    pub version: String,
    /// Standard revision the app claims, e.g. `DMCS-1.1`.
    pub standard: String,
    pub tier: Tier,
    pub exposure: Exposure,
    #[serde(default)]
    pub description: Option<String>,
}

/// Purity / hardening tier declared by the author (a process attestation, see SECURITY.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    L0,
    L1,
    L2,
    L3,
}

/// Who can feed input to the program. Drives the security gates in `audit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Exposure {
    /// Runs on a trusted local session; inputs come from its own user.
    Local,
    /// Reachable from a private network or other local users.
    Lan,
    /// Reachable (directly or through a proxy) from the public internet.
    Internet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Isa {
    X86_64,
    Aarch64,
    Riscv64,
    Riscv32,
    /// Arm Thumb / Thumb-2 (Cortex-M: RP2040, RP2350 Arm cores).
    Thumb,
    /// Tensilica Xtensa LX6/LX7 (ESP32, ESP32-S3).
    Xtensa,
    /// 8-bit AVR (`ATmega328P`).
    Avr,
}

impl Isa {
    /// Default padding pattern for unused code bytes (the ISA's canonical NOP), little-endian.
    #[must_use]
    pub fn default_code_fill(self) -> &'static [u8] {
        match self {
            Isa::X86_64 => &[0x90],
            Isa::Aarch64 => &[0x1f, 0x20, 0x03, 0xd5],
            Isa::Riscv64 | Isa::Riscv32 => &[0x13, 0x00, 0x00, 0x00],
            Isa::Thumb => &[0x00, 0xbf],
            Isa::Xtensa => &[0xf0, 0x20, 0x00],
            Isa::Avr => &[0x00, 0x00],
        }
    }

    /// Minimum instruction alignment in bytes.
    #[must_use]
    pub fn insn_align(self) -> u64 {
        match self {
            Isa::X86_64 | Isa::Xtensa => 1,
            Isa::Aarch64 => 4,
            Isa::Riscv64 | Isa::Riscv32 | Isa::Thumb | Isa::Avr => 2,
        }
    }

    /// ELF `e_machine` value for this ISA.
    #[must_use]
    pub fn elf_machine(self) -> u16 {
        match self {
            Isa::X86_64 => 62,
            Isa::Aarch64 => 183,
            Isa::Riscv64 | Isa::Riscv32 => 243,
            Isa::Thumb => 40,
            Isa::Xtensa => 94,
            Isa::Avr => 83,
        }
    }

    /// `objdump -m` architecture name, used for optional external annotation text.
    #[must_use]
    pub fn objdump_arch(self) -> &'static str {
        match self {
            Isa::X86_64 => "i386:x86-64",
            Isa::Aarch64 => "aarch64",
            Isa::Riscv64 => "riscv:rv64",
            Isa::Riscv32 => "riscv:rv32",
            Isa::Thumb => "arm",
            Isa::Xtensa => "xtensa",
            Isa::Avr => "avr",
        }
    }
}

impl fmt::Display for Isa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Isa::X86_64 => "x86-64",
            Isa::Aarch64 => "aarch64",
            Isa::Riscv64 => "riscv64",
            Isa::Riscv32 => "riscv32",
            Isa::Thumb => "thumb",
            Isa::Xtensa => "xtensa",
            Isa::Avr => "avr",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Os {
    Linux,
    BareMetal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Elf64,
    Elf32,
    /// Flat image: file offset 0 maps to `target.base`.
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub isa: Isa,
    pub os: Os,
    pub format: Format,
    /// Virtual address of file offset 0 when no segment maps it (raw images).
    pub base: u64,
    /// Name of the slot or entry label where execution starts.
    pub entry: String,
    /// Output file name written to `build/`.
    pub output: String,
    /// Total image size in bytes; defaults to the end of the last slot.
    #[serde(default)]
    pub file_size: Option<u64>,
    /// Hex pattern used to pad code slots; defaults to the ISA NOP.
    #[serde(default)]
    pub code_fill: Option<String>,
    /// Hex byte used to pad data, header and uncovered gaps; defaults to `00`.
    #[serde(default)]
    pub data_fill: Option<String>,
    /// Register permanently holding the state base (e.g. `r15`), enables state-field mapping.
    #[serde(default)]
    pub state_register: Option<String>,
    /// Value of `state_register` for the whole run.
    #[serde(default)]
    pub state_base: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub name: String,
    pub vaddr: u64,
    pub offset: u64,
    pub file_size: u64,
    pub mem_size: u64,
    /// `r--`, `r-x`, `rw-`, `rwx`.
    pub perms: String,
    #[serde(default)]
    pub align: Option<u64>,
}

impl Segment {
    #[must_use]
    pub fn readable(&self) -> bool {
        self.perms.contains('r')
    }
    #[must_use]
    pub fn writable(&self) -> bool {
        self.perms.contains('w')
    }
    #[must_use]
    pub fn executable(&self) -> bool {
        self.perms.contains('x')
    }
    #[must_use]
    pub fn contains_vaddr(&self, addr: u64) -> bool {
        addr >= self.vaddr && addr < self.vaddr.saturating_add(self.mem_size)
    }
    /// ELF `p_flags` for the declared permissions.
    #[must_use]
    pub fn elf_flags(&self) -> u32 {
        u32::from(self.executable())
            | (u32::from(self.writable()) << 1)
            | (u32::from(self.readable()) << 2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotKind {
    /// File/format headers (ELF header, program headers, image headers).
    Header,
    /// Executable instructions.
    Code,
    /// Read-only data: strings, tables, constants, assets.
    Rodata,
    /// Initialised writable data.
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub name: String,
    pub kind: SlotKind,
    pub vaddr: u64,
    pub size: u64,
    /// Slot source, relative to the app directory.
    pub file: String,
    pub purpose: String,
    #[serde(default, rename = "label")]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub contract: Option<Contract>,
    /// Justification required when the slot contains indirect branches or calls.
    #[serde(default)]
    pub indirect: Option<String>,
    /// Justification for syscall sites whose number cannot be resolved statically.
    #[serde(default)]
    pub dynamic_syscalls: Option<String>,
    /// Justification for writes into executable memory (only legal below tier L2).
    #[serde(default)]
    pub self_modifying: Option<String>,
}

impl Slot {
    #[must_use]
    pub fn end(&self) -> u64 {
        self.vaddr.saturating_add(self.size)
    }
    #[must_use]
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.vaddr && addr < self.end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub name: String,
    /// Byte offset from the start of the slot.
    pub offset: u64,
    /// Whether other slots may call or jump here.
    #[serde(default)]
    pub entry: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// Register-level calling contract of a code slot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Registers the slot may destroy. Anything else written must be saved and restored.
    #[serde(default)]
    pub clobbers: Vec<String>,
    /// Slots or labels this slot is expected to call.
    #[serde(default)]
    pub calls: Vec<String>,
    /// Whether the slot never returns (exit paths, main loops).
    #[serde(default)]
    pub noreturn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Syscalls {
    /// Allowed syscall names for the target OS/ABI.
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mmio {
    pub name: String,
    pub base: u64,
    pub size: u64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateField {
    pub name: String,
    pub vaddr: u64,
    pub size: u64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub authored_by: String,
    pub method: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// A loaded application: manifest plus the directory it lives in.
#[derive(Debug, Clone)]
pub struct AppDir {
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl AppDir {
    /// Loads `nopray.toml` from `root` and validates its internal consistency.
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let path = root.join(MANIFEST_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let manifest: Manifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        manifest.validate()?;
        Ok(Self { root, manifest })
    }

    #[must_use]
    pub fn slot_path(&self, slot: &Slot) -> PathBuf {
        self.root.join(&slot.file)
    }

    #[must_use]
    pub fn build_dir(&self) -> PathBuf {
        self.root.join("build")
    }
}

impl Manifest {
    /// Structural checks that do not need the slot bytes.
    pub fn validate(&self) -> Result<()> {
        if self.slots.is_empty() {
            bail!("manifest declares no slots");
        }
        let mut names = std::collections::BTreeSet::new();
        for slot in &self.slots {
            if slot.size == 0 {
                bail!("slot `{}` has size 0", slot.name);
            }
            if !names.insert(slot.name.as_str()) {
                bail!("duplicate slot name `{}`", slot.name);
            }
            if slot.kind == SlotKind::Code && slot.vaddr % self.target.isa.insn_align() != 0 {
                bail!(
                    "code slot `{}` at {:#x} breaks {}-byte instruction alignment",
                    slot.name,
                    slot.vaddr,
                    self.target.isa.insn_align()
                );
            }
            for label in &slot.labels {
                if label.offset >= slot.size {
                    bail!(
                        "label `{}` offset {:#x} is outside slot `{}`",
                        label.name,
                        label.offset,
                        slot.name
                    );
                }
                if !names.insert(label.name.as_str()) {
                    bail!("duplicate symbol `{}`", label.name);
                }
            }
            if slot.file.contains("..") || Path::new(&slot.file).is_absolute() {
                bail!(
                    "slot `{}` file must be a relative path inside the app directory",
                    slot.name
                );
            }
        }
        let mut sorted: Vec<&Slot> = self.slots.iter().collect();
        sorted.sort_by_key(|s| s.vaddr);
        for pair in sorted.windows(2) {
            if pair[0].end() > pair[1].vaddr {
                bail!("slots `{}` and `{}` overlap", pair[0].name, pair[1].name);
            }
        }
        if self.symbol(&self.target.entry).is_none() {
            bail!(
                "entry `{}` is neither a slot nor a label",
                self.target.entry
            );
        }
        for seg in &self.segments {
            if seg.file_size > seg.mem_size {
                bail!("segment `{}` file_size exceeds mem_size", seg.name);
            }
            if !matches!(seg.perms.as_str(), "r--" | "r-x" | "rw-" | "rwx" | "--x") {
                bail!(
                    "segment `{}` perms must be one of r--, r-x, rw-, rwx",
                    seg.name
                );
            }
        }
        if let (Some(_), None) | (None, Some(_)) =
            (&self.target.state_register, self.target.state_base)
        {
            bail!("target.state_register and target.state_base must be declared together");
        }
        self.parse_code_fill()?;
        self.parse_data_fill()?;
        Ok(())
    }

    pub fn parse_code_fill(&self) -> Result<Vec<u8>> {
        match &self.target.code_fill {
            Some(h) => hex::decode(h.replace(' ', "")).context("target.code_fill is not hex"),
            None => Ok(self.target.isa.default_code_fill().to_vec()),
        }
    }

    pub fn parse_data_fill(&self) -> Result<u8> {
        match &self.target.data_fill {
            Some(h) => {
                let v = hex::decode(h).context("target.data_fill is not hex")?;
                match v.as_slice() {
                    [b] => Ok(*b),
                    _ => bail!("target.data_fill must be exactly one byte"),
                }
            }
            None => Ok(0),
        }
    }

    /// Address of a slot name or label name.
    #[must_use]
    pub fn symbol(&self, name: &str) -> Option<u64> {
        self.slots.iter().find_map(|s| {
            if s.name == name {
                Some(s.vaddr)
            } else {
                s.labels
                    .iter()
                    .find(|l| l.name == name)
                    .map(|l| s.vaddr + l.offset)
            }
        })
    }

    /// Address of any named symbol usable in `#=` intents: slots, labels, or `[[state]]` fields.
    #[must_use]
    pub fn intent_symbol(&self, name: &str) -> Option<u64> {
        self.symbol(name).or_else(|| {
            self.state
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.vaddr)
        })
    }

    /// Addresses other slots may legally call or jump to: every slot start plus `entry` labels.
    #[must_use]
    pub fn entry_points(&self) -> Vec<(u64, String)> {
        let mut v = Vec::new();
        for s in &self.slots {
            if s.kind == SlotKind::Code {
                v.push((s.vaddr, s.name.clone()));
                for l in s.labels.iter().filter(|l| l.entry) {
                    v.push((s.vaddr + l.offset, l.name.clone()));
                }
            }
        }
        v
    }

    #[must_use]
    pub fn slot_at(&self, addr: u64) -> Option<&Slot> {
        self.slots.iter().find(|s| s.contains(addr))
    }

    #[must_use]
    pub fn segment_at(&self, addr: u64) -> Option<&Segment> {
        self.segments.iter().find(|s| s.contains_vaddr(addr))
    }

    /// File offset backing `vaddr`, if the address is file-backed.
    #[must_use]
    pub fn file_offset(&self, vaddr: u64) -> Option<u64> {
        if self.segments.is_empty() {
            return vaddr.checked_sub(self.target.base);
        }
        self.segments.iter().find_map(|s| {
            (vaddr >= s.vaddr && vaddr < s.vaddr + s.file_size)
                .then(|| s.offset + (vaddr - s.vaddr))
        })
    }

    /// Name of the symbol (slot or label) starting exactly at `addr`.
    #[must_use]
    pub fn symbol_at(&self, addr: u64) -> Option<String> {
        // Labels are more specific than the slot that contains them.
        let label = self.slots.iter().find_map(|s| {
            s.labels
                .iter()
                .find(|l| s.vaddr + l.offset == addr)
                .map(|l| l.name.clone())
        });
        label.or_else(|| {
            self.slots
                .iter()
                .find(|s| s.vaddr == addr)
                .map(|s| s.name.clone())
        })
    }
}
