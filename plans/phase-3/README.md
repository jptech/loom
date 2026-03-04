# Phase 3: Platforms, Variants, Profiles, OOC Synthesis

**Prerequisites:** Phase 2 complete
**Goal:** A single project targets multiple boards. Components have vendor-specific variants. Profile dimensions enable combinatorial builds.

## Spec Reference
`system_plan.md` §4 (Platform Model), §3.4 (Component Variants), §5.2 (Build Profiles), §7.2.1 (OOC Synthesis)

---

## Tasks

### Task 01: Platform Manifest Parsing
**Files:** `crates/loom-core/src/manifest/platform.rs`, `crates/loom-core/src/resolve/platform.rs`

Add `platform.toml` parsing (spec §4.1):
```rust
#[derive(Deserialize)]
pub struct PlatformManifest {
    pub platform: PlatformMeta,
    pub clocks: HashMap<String, ClockDef>,
    pub constraints: PlatformConstraints,
    pub params: HashMap<String, toml::Value>,
    pub variant_defaults: Option<VariantDefaults>,
    pub tool: Option<PlatformToolSpec>,
}
```

**Virtual platforms** (`virtual = true`): no part required, disallow `loom build` (only `loom sim`).

**Platform discovery:** Same glob mechanism as components. `classify_member()` already checks for `platform.toml`.

**Platform resolution:** When `project.toml` has `platform = "zcu104"`:
1. Find `platform.toml` with `[platform].name == "zcu104"` in workspace
2. Merge: part, backend, constraints, clocks, params, variant_defaults into resolved project

**Parameter substitution:** `${platform.clocks.sys_clk.frequency_mhz}` in generator config properties, defines, etc. Implement via a `ParameterResolver` that takes dot-notation paths and looks them up in the platform context.

### Task 02: Component Variants
**Files:** `crates/loom-core/src/manifest/component.rs` (extend), `crates/loom-core/src/resolve/variant.rs`

Add variant parsing (spec §3.4):
```rust
#[derive(Deserialize)]
pub struct ComponentVariant {
    pub description: Option<String>,
    pub tags: Vec<String>,  // e.g., ["vendor:xilinx"]
    pub filesets: VariantFilesetOverride,
    pub generators: Vec<GeneratorDecl>,
}

#[derive(Deserialize)]
pub struct VariantFilesetOverride {
    pub synth: Option<VariantFileset>,
    pub sim: Option<VariantFileset>,
}

#[derive(Deserialize)]
pub struct VariantFileset {
    pub add_files: Vec<PathBuf>,
    pub remove_files: Vec<PathBuf>,
    pub add_constraints: Vec<PathBuf>,
}
```

**Variant resolution priority** (spec §3.4):
1. Project explicit: `dependency = { version = ..., variant = "xilinx" }`
2. Profile override
3. Platform `variant_defaults.tags`: if platform has `tags = ["vendor:xilinx"]`, auto-select variant with matching tag
4. No variant (base fileset)

**Conflict detection:** If multiple platform tags match different variants → error.

### Task 03: Build Profiles
**Files:** `crates/loom-core/src/manifest/project.rs` (extend), `crates/loom-core/src/resolve/profile.rs`

Add simple profiles and profile dimensions (spec §5.2.1, §5.2.2).

**Simple profiles:**
```rust
#[derive(Deserialize)]
pub struct ProfileOverlay {
    pub description: Option<String>,
    pub platform: Option<String>,
    pub params: HashMap<String, toml::Value>,
    pub filesets: Option<ProfileFilesetOverride>,
    pub build: Option<BuildConfig>,
}
```

**Dimensional profiles:**
```rust
#[derive(Deserialize)]
pub struct ProfileDimension {
    pub description: Option<String>,
    pub default: String,
    pub choices: HashMap<String, ProfileOverlay>,
}
```

**CLI:** `--profile kcu116_port`, `--profile board=kcu116,tier=reduced`, `--profile-all`

**Build directory naming:** `kcu116.reduced.off/` for dimensional combination.

**Overlay application:** In order dimensions are declared. `platform` replaces, `params` merges (later wins), `add_files` appends, `build` merges.

### Task 04: OOC Synthesis
**Files:** `crates/loom-vivado/src/ooc.rs`, `crates/loom-core/src/build/pipeline.rs` (extend)

**Spec §7.2.1.** When a component has `[synth] ooc = true`:
1. Add OOC build nodes to Phase 5 DAG
2. Each OOC node: `synth_design -mode out_of_context -top <ooc_top> -part <part>`
3. Produces `.build/<project>/ooc/<component>/post_ooc_synth.dcp`
4. Top-level synthesis loads these checkpoints: `read_checkpoint {path/to/post_ooc_synth.dcp}`

**OOC cache key:** component fileset hash + transitive dep hashes + part + tool version.

**Parallel OOC:** Independent OOC builds run in parallel (up to `-j N`).

**Graceful degradation:** If backend doesn't support OOC (yosys), emit info diagnostic and synthesize in-context.

### Task 05: Scaffolding Commands
**Files:** `crates/loom-cli/src/commands/new.rs`

`loom new component <path>`, `loom new project <path>`, `loom new platform <path>`

Generate starter `component.toml`, `project.toml`, `platform.toml` with comments and example values. Prompt for name (or use last path component). Create `rtl/`, `tb/`, `constraints/` directories.

### Task 06: Platform in Validate + Build
Extend `validate_pre_build()` to check platform consistency:
- Platform `part` matches backend expectation
- Clock frequencies referenced in templates have platform definitions
- Profile exclusion rules checked

Extend `generate_tcl()` to:
- Use platform part (not `[target].part` directly)
- Include platform constraint files in correct order
- Substitute `${platform.*}` parameters

---

## Key Data Flow Changes (Phase 3)

```
workspace.toml
    members: ["lib/*", "projects/*", "platforms/*"]
                                              ↓
project.toml: platform = "zcu104"
                    ↓ lookup by name
              platform.toml (part, clocks, params, constraints, variant_defaults)
                    ↓ merge
              ResolvedProject (has platform: ResolvedPlatform)
                    ↓
              parameter substitution in generators + defines + templates
                    ↓
              variant selection based on platform tags
                    ↓
              AssembledFilesets (includes platform constraints)
```

## Test Fixtures Needed

`tests/fixtures/multi_platform/`: same project targeting `zcu104` and `kcu116` platforms via profiles. Validates that switching platform changes part, constraints, and variant selections.
