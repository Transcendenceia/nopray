# Editions: how this book grows

NOPRAY is young, and this book is being written along the way. Each job done with it either
confirms a chapter or teaches something the chapter did not yet say. The book only improves
if that lesson is written down, in the right place, as a general rule.

## How to add what a job taught

After every NOPRAY job, before closing it, ask four questions.

| The job produced… | Goes into | Form |
|---|---|---|
| A mistake that cost time, or one a check caught | **Traps** of the chapter where the step lives | One bullet: the general rule, then the incident that taught it in a few words |
| A complete, verified case worth studying | **Worked examples** | The case, its numbers, and a link to its evidence file. Failures and losses qualify. |
| A new verified fact about a target, tool or technique | **State of knowledge** | Move it from *Open* to *Verified*, with the evidence link |
| A task that would train the next agent | **Problems** | Difficulty ★–★★★ and acceptance checks a reviewer can run |

Rules:

1. **Write the rule, not just the story.** "The audit.json differed only in an absolute path"
   is a story. "Generated files are evidence: regenerate and compare them before calling a diff
   foreign" is a rule.
2. **Cite evidence.** Every number links to the evidence file or benchmark that produced it.
   An unverified claim is marked UNVERIFIED or stays out.
3. **Keep losses.** A technique that lost, a port that did not converge or a gate left unmet
   teaches as much as a win (chapter 6, vocoder row).
4. **One home per fact.** The chapter holds the method. Finding codes and manifest fields stay
   in [MANIFEST.md](../references/MANIFEST.md). Measurements stay in
   [BENCHMARKS.md](../references/BENCHMARKS.md). Target status stays in
   [TARGETS.md](../references/TARGETS.md). Link to them; do not copy.
5. **A new chapter only for a new kind of work.** For example, a GPU target or a new carrier
   format. Otherwise extend an existing chapter.
6. **Tool changes come with their lesson.** When `nopray` gains a check or a command, the PR
   also updates the chapter step that the check protects, and adds a line below.

## Edition log

| Date | Edition | What it added |
|---|---|---|
| 24–26/11/2026 | DMCS-1.1 | Fixed-slot manifest, `#=` intents, runtime and authoring benchmarks, `C017` after `nopray-jb-r3`, the ban on encoder DSLs, DOOM with native PCM, bare-metal Qwen3-TTS |
| 27/11/2026 | AgujaESP | ESP32-S3 ROM probe on QEMU and the board, the PHY routine port, a live tailnet node from an imported flash set, ESP container audit (`E014`–`E016`), a slot edit over OTA |
| 27/11/2026 | Agent ergonomics | Symbolic fields, `I001` as an error with bit hints, `context`, per-slot `origin`/`evidence` and the hand-written share |
| 27/11/2026 | **The book** | The skill rewritten as nine chapters, each with principle, procedure, traps, worked examples, problems and state of knowledge, plus this growth protocol |

## Open questions the next editions should answer

- An all-authored image booting from flash on real hardware (chapter 9)
- Differential execution for ports (`nopray equiv`, [MANUAL.md](../references/MANUAL.md) §7),
  so that "equivalent" becomes a tool check (chapter 5)
- Thumb and Xtensa decoders with their field kinds (chapters 2 and 9)
- Vector kernels outside x86-64, and a method for compute-bound stages (chapter 6)
- A fuzzing harness shipped with the tool (chapter 8)
