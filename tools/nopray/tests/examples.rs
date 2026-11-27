//! Every mapped example in the skill must build to its locked bytes and audit without errors.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn examples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("examples directory is readable") {
            let path = entry.expect("directory entry").path();
            if path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|n| n != "build" && n != "slots")
            {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "nopray.toml") {
                found.push(dir.clone());
            }
        }
    }
    found.sort();
    found
}

/// Copies an app (manifest + slots) to a scratch directory so tests never touch the skill.
fn scratch_copy(app: &Path, tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!("nopray-test-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dst);
    fs::create_dir_all(dst.join("slots")).expect("create scratch dir");
    fs::copy(app.join("nopray.toml"), dst.join("nopray.toml")).expect("copy manifest");
    for e in fs::read_dir(app.join("slots")).expect("slots dir") {
        let p = e.expect("slot entry").path();
        fs::copy(
            &p,
            dst.join("slots").join(p.file_name().expect("file name")),
        )
        .expect("copy slot");
    }
    dst
}

#[test]
fn examples_rebuild_to_locked_bytes_and_audit_clean() {
    let bin = env!("CARGO_BIN_EXE_nopray");
    let apps = examples();
    assert!(
        apps.len() >= 6,
        "expected the mapped examples (including qwen3-tts-x86_64-baremetal), found {apps:?}"
    );
    for (i, app) in apps.iter().enumerate() {
        let lock: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(app.join("build/manifest.lock.json")).expect("committed lock file"),
        )
        .expect("lock is JSON");
        let dir = scratch_copy(app, &i.to_string());
        let build = Command::new(bin)
            .arg("build")
            .arg(&dir)
            .output()
            .expect("run build");
        assert!(
            build.status.success(),
            "{}: build failed: {}",
            app.display(),
            String::from_utf8_lossy(&build.stderr)
        );
        let out = String::from_utf8_lossy(&build.stdout);
        let want = lock["sha256"].as_str().expect("sha256 in lock");
        assert!(
            out.contains(want),
            "{}: rebuilt image differs from the locked sha256 {want}: {out}",
            app.display()
        );
        let audit = Command::new(bin)
            .arg("audit")
            .arg(&dir)
            .output()
            .expect("run audit");
        assert!(
            audit.status.success(),
            "{}: audit reported errors:\n{}",
            app.display(),
            String::from_utf8_lossy(&audit.stdout)
        );
        let check = Command::new(bin)
            .args(["annotate", "--check"])
            .arg(&dir)
            .output()
            .expect("run annotate --check");
        assert!(
            check.status.success(),
            "{}: committed slot comments are stale",
            app.display()
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_flipped_byte_is_reported() {
    let bin = env!("CARGO_BIN_EXE_nopray");
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello-aarch64-linux");
    let dir = scratch_copy(&app, "tamper");
    let good = Command::new(bin)
        .arg("build")
        .arg(&dir)
        .output()
        .expect("build");
    assert!(good.status.success());
    let built = dir.join("build/hello-aarch64");
    let mut bytes = fs::read(&built).expect("built binary");
    bytes[0x104] ^= 0x01; // corrupt `adr x1, msg`
    let tampered = dir.join("tampered.bin");
    fs::write(&tampered, &bytes).expect("write tampered copy");
    let audit = Command::new(bin)
        .arg("audit")
        .arg(&dir)
        .arg("--binary")
        .arg(&tampered)
        .output()
        .expect("audit");
    let text = String::from_utf8_lossy(&audit.stdout);
    assert!(
        !audit.status.success() && text.contains("R001"),
        "tampering must fail the audit:\n{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unlabelled_data_interior_pointer_triggers_c017() {
    let bin = env!("CARGO_BIN_EXE_nopray");
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello-aarch64-linux");
    let dir = scratch_copy(&app, "c017");
    // Add an explicit label at offset 0x04 inside `msg`, then shift `adr x1, 0x400140` to 0x400142.
    let mut toml = fs::read_to_string(dir.join("nopray.toml")).expect("read manifest");
    toml.push_str("\n[[slot.label]]\nname = \"msg_inner\"\noffset = 0x4\n");
    fs::write(dir.join("nopray.toml"), &toml).expect("write manifest");

    // Clean check: pointing to slot start 0x400140 passes strict audit.
    let ok_audit = Command::new(bin)
        .args(["audit", "--strict"])
        .arg(&dir)
        .output()
        .expect("audit clean");
    assert!(
        ok_audit.status.success(),
        "slot start must remain clean:\n{}",
        String::from_utf8_lossy(&ok_audit.stdout)
    );

    // Shift `adr x1, 0x400140` (e1 01 00 10 -> pc + 0x3c) by +2 bytes to 0x400142 (e1 01 00 50 -> immlo=2, immhi=0x0f => 0x3e).
    let slot_path = dir.join("slots/0100_start.hex");
    let slot = fs::read_to_string(&slot_path).expect("read start slot");
    let shifted = slot.replace(
        "e1 01 00 10  # 0x00400104  adr x1, 0x400140  → msg  #= adr x1, msg",
        "e1 01 00 50  #= adr x1, 0x400142",
    );
    assert_ne!(slot, shifted, "expected adr instruction in hello-aarch64");
    fs::write(&slot_path, shifted).expect("write shifted slot");

    let bad_audit = Command::new(bin)
        .args(["audit", "--strict"])
        .arg(&dir)
        .output()
        .expect("audit shifted");
    let out = String::from_utf8_lossy(&bad_audit.stdout);
    assert!(
        !bad_audit.status.success()
            && out.contains("C017")
            && out.contains("nearest label `msg` is at 0x400140 (-2 byte(s))"),
        "off-by-2 interior data pointer must trigger C017:\n{out}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn data_images_skip_entry_checks_and_reject_code() {
    let bin = env!("CARGO_BIN_EXE_nopray");
    let dir = std::env::temp_dir().join(format!("nopray-data-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("slots")).expect("scratch dir");
    let manifest = |kind: &str, perms: &str| {
        format!(
            "[app]\nname = \"table\"\nversion = \"1\"\nstandard = \"DMCS-1.1\"\ntier = \"L1\"\nexposure = \"local\"\n\n\
             [target]\nisa = \"xtensa\"\nos = \"bare-metal\"\nformat = \"data\"\nbase = 0\nentry = \"table\"\noutput = \"table.bin\"\nfile_size = 0x4\n\n\
             [[segment]]\nname = \"table\"\nvaddr = 0x0\noffset = 0x0\nfile_size = 0x4\nmem_size = 0x4\nperms = \"{perms}\"\nalign = 1\n\n\
             [[slot]]\nname = \"table\"\nkind = \"{kind}\"\nvaddr = 0x0\nsize = 0x4\nfile = \"slots/table.hex\"\npurpose = \"test\"\n"
        )
    };
    fs::write(
        dir.join("slots/table.hex"),
        "aa 50 01 02  #= table magic and type\n",
    )
    .expect("slot");
    fs::write(dir.join("nopray.toml"), manifest("rodata", "r--")).expect("manifest");
    let ok = Command::new(bin)
        .arg("build")
        .arg(&dir)
        .output()
        .expect("build");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let audit = Command::new(bin)
        .arg("audit")
        .arg(&dir)
        .output()
        .expect("audit");
    let text = String::from_utf8_lossy(&audit.stdout);
    assert!(
        audit.status.success() && !text.contains("E006"),
        "data image must audit clean:\n{text}"
    );
    fs::write(dir.join("nopray.toml"), manifest("code", "r-x")).expect("manifest");
    let bad = Command::new(bin)
        .arg("build")
        .arg(&dir)
        .output()
        .expect("build code");
    assert!(
        !bad.status.success() && String::from_utf8_lossy(&bad.stderr).contains("code slot"),
        "a code slot in a data image must be rejected"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Rewrites every slot file of a scratch app with `edit(line) -> Option<new line>`.
fn rewrite_slots(dir: &Path, edit: impl Fn(&str) -> Option<String>) -> usize {
    let mut changed = 0;
    let mut paths: Vec<PathBuf> = fs::read_dir(dir.join("slots"))
        .expect("slots dir")
        .map(|e| e.expect("slot entry").path())
        .collect();
    paths.sort(); // read_dir order differs between filesystems
    for p in paths {
        let text = fs::read_to_string(&p).expect("slot text");
        let mut out = String::new();
        for line in text.lines() {
            if let Some(new) = edit(line) {
                out.push_str(&new);
                changed += 1;
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        fs::write(&p, out).expect("write slot");
    }
    changed
}

fn locked_sha(app: &Path) -> String {
    let lock: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(app.join("build/manifest.lock.json")).expect("lock"),
    )
    .expect("lock JSON");
    lock["sha256"].as_str().expect("sha256").to_owned()
}

fn run_ok(args: &[&str], dir: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_nopray"))
        .args(args)
        .arg(dir)
        .output()
        .expect("run nopray");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "nopray {args:?} failed:\n{text}");
    text
}

type Edit = Box<dyn Fn(&str) -> Option<String>>;

/// Symbolic fields must reproduce the hand-computed displacements byte for byte, survive
/// `annotate`, and audit clean.
#[test]
fn symbolic_fields_rebuild_byte_identical() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let cases: Vec<(&str, Edit)> = vec![
        (
            "nopray-doom-x86_64-cachyos",
            Box::new(|line: &str| {
                let (code, comment) = line.split_once('#')?;
                let name = line.split("#= call ").nth(1)?.trim();
                let hex: Vec<&str> = code.split_whitespace().collect();
                (hex.len() == 5 && hex[0] == "e8" && !comment.is_empty())
                    .then(|| format!("e8 {{rel32 @{name}}}  #= call {name}"))
            }),
        ),
        (
            "hello-aarch64-linux",
            Box::new(|line: &str| {
                if line.contains("#= adr x1, msg") {
                    Some("{a64.adr @msg : 01 00 00 10}  #= adr x1, msg".into())
                } else if line.contains("#= e_entry 0x400100") {
                    Some("{abs64 @_start}  #= e_entry 0x400100".into())
                } else {
                    None
                }
            }),
        ),
        (
            "hello-riscv64-linux",
            Box::new(|line: &str| {
                if line.contains("#= auipc a1,0x0") {
                    Some("{rv.pcrel_hi20 @msg : 97 05 00 00}  #= auipc a1,0x0".into())
                } else if line.contains("#= addi a1,a1,60") {
                    Some("{rv.pcrel_lo12_i @msg pc=-4 : 93 85 05 00}  #= addi a1,a1,60".into())
                } else {
                    None
                }
            }),
        ),
    ];
    for (i, (name, edit)) in cases.iter().enumerate() {
        let app = root.join(name);
        let dir = scratch_copy(&app, &format!("fields{i}"));
        let changed = rewrite_slots(&dir, edit);
        assert!(
            changed >= 2,
            "{name}: expected lines to symbolize, got {changed}"
        );
        let built = run_ok(&["build"], &dir);
        assert!(
            built.contains(&locked_sha(&app)),
            "{name}: symbolic fields changed the bytes: {built}"
        );
        run_ok(&["annotate"], &dir);
        run_ok(&["annotate", "--check"], &dir);
        let kept: usize = fs::read_dir(dir.join("slots"))
            .expect("slots")
            .map(|e| {
                fs::read_to_string(e.expect("entry").path())
                    .expect("slot")
                    .matches(" @")
                    .count()
            })
            .sum();
        assert!(kept >= changed, "{name}: annotate dropped field tokens");
        run_ok(&["audit", "--strict"], &dir);
        let _ = fs::remove_dir_all(&dir);
    }
}

/// A register typo in an authored word is an error, and the hint names the bit to fix.
#[test]
fn intent_mismatch_is_an_error_with_a_bit_hint() {
    let bin = env!("CARGO_BIN_EXE_nopray");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let cases = [
        // AArch64: bytes encode `adr x1, msg`; the author meant x9 (Rd bit 3).
        (
            "hello-aarch64-linux",
            "slots/0100_start.hex",
            "#= adr x1, msg",
            "#= adr x9, msg",
            "instruction bit 3",
        ),
        // x86-64: `e8 d9 fc ff ff` = call fill_rect; the intent is 4 bytes further on
        // (displacement byte 0xd9 vs 0xdd: bit 2).
        (
            "nopray-doom-x86_64-cachyos",
            "slots/3680_render_hud.hex",
            "#= call fill_rect",
            "#= call 0x403384",
            "intent +0x4",
        ),
    ];
    for (i, (name, file, from, to, hint)) in cases.iter().enumerate() {
        let dir = scratch_copy(&root.join(name), &format!("hint{i}"));
        let path = dir.join(file);
        let text = fs::read_to_string(&path).expect("slot file");
        let line = text
            .lines()
            .find(|l| l.contains(from))
            .unwrap_or_else(|| panic!("{name}: line `{from}` not found in {file}"));
        let edited = text.replacen(line, &line.replace(from, to), 1);
        fs::write(&path, edited).expect("write slot");
        let out = Command::new(bin)
            .arg("audit")
            .arg(&dir)
            .output()
            .expect("audit");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            !out.status.success() && text.contains("ERROR I001") && text.contains(hint),
            "{name}: expected an I001 error mentioning `{hint}`:\n{text}"
        );
        assert!(
            text.contains("1-bit neighbour"),
            "{name}: expected a 1-bit hint:\n{text}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn context_lists_what_a_change_must_retest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let doom = root.join("nopray-doom-x86_64-cachyos");
    let out = Command::new(env!("CARGO_BIN_EXE_nopray"))
        .args(["context"])
        .arg(&doom)
        .args(["fill_rect", "--json"])
        .output()
        .expect("context");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("context JSON");
    assert_eq!(v["slot"], "draw_primitives", "labels resolve to their slot");
    assert!(v["free"].as_u64().expect("free") > 0);
    let retest: Vec<&str> = v["retest"]
        .as_array()
        .expect("retest")
        .iter()
        .filter_map(|x| x.as_str())
        .collect();
    assert!(retest.contains(&"render_hud"), "{retest:?}");
}

#[test]
fn origins_are_totalled_and_reauthored_slots_need_evidence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let app = root.join("hello-aarch64-linux");
    let dir = scratch_copy(&app, "origin");
    let toml = fs::read_to_string(dir.join("nopray.toml")).expect("manifest");
    let mut origins = ["imported", "reauthored", "authored"].into_iter();
    let stamped: String = toml
        .lines()
        .map(|l| {
            if l.trim() == "[[slot]]" {
                format!(
                    "{l}\norigin = \"{}\"\n",
                    origins.next().expect("three slots")
                )
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    fs::write(dir.join("nopray.toml"), stamped).expect("write manifest");
    let out = Command::new(env!("CARGO_BIN_EXE_nopray"))
        .args(["audit", "--strict"])
        .arg(&dir)
        .output()
        .expect("audit");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success()
            && text.contains("P001")
            && text.contains("origin (used bytes): authored 18"),
        "{text}"
    );

    // `import` marks every slot of a mapped binary as imported.
    run_ok(&["build"], &dir);
    let built = dir.join("build/hello-aarch64");
    let manifest = app.join("nopray.toml");
    let imported = std::env::temp_dir().join(format!("nopray-test-{}-import", std::process::id()));
    let _ = fs::remove_dir_all(&imported);
    let out = Command::new(env!("CARGO_BIN_EXE_nopray"))
        .args(["import", "--binary"])
        .arg(&built)
        .arg("--manifest")
        .arg(&manifest)
        .arg("--out")
        .arg(&imported)
        .output()
        .expect("import");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let m = fs::read_to_string(imported.join("nopray.toml")).expect("imported manifest");
    assert_eq!(m.matches("origin = \"imported\"").count(), 3, "{m}");
    let audit = run_ok(&["audit"], &imported);
    assert!(audit.contains("hand-written 0.0%"), "{audit}");
    let _ = fs::remove_dir_all(&imported);
    let _ = fs::remove_dir_all(&dir);
}
