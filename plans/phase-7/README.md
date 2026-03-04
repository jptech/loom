# Phase 7: Ecosystem

**Prerequisites:** Phase 6 complete
**Goal:** Loom works beyond a single team's monorepo. Vendor coverage is comprehensive. Community can build on Loom.

## Spec Reference
`system_plan.md` §15 Phase 7, §3.5.2 (DependencySource trait — registry interface)

---

## Tasks

### Task 01: Package Registry Design and Implementation

**The resolution interface was designed for this from Day 1** (spec §3.5.2):
```rust
trait DependencySource {
    async fn resolve(&self, name: &str, constraint: &VersionReq) -> Result<Option<ResolvedDependency>>;
    async fn list_versions(&self, name: &str) -> Result<Vec<Version>>;
}
```

The `WorkspaceDependencySource` from Phase 1 implements this trait. A `RegistryDependencySource` implements the same interface against a remote registry API.

**Registry spec decisions:**
- Package format: `.tar.gz` containing `component.toml` + source files (no `.build/`, no generated files)
- Registry protocol: REST API (similar to crates.io or npm registry)
- Authentication: API token
- Namespace enforcement: `org/name` format required

**Lockfile additions for registry:**
```toml
[[package]]
name = "acmecorp/axi_common"
version = "1.3.0"
source = "registry:https://registry.loom-fpga.dev"
checksum = "sha256:abc123..."  # of the package tarball
```

**CLI commands:**
```
loom publish           Publish current component to registry
loom search <query>    Search registry for components
loom install <pkg>     Add registry dependency to workspace
```

### Task 02: Lattice Radiant Backend

Similar structure to Quartus backend. Key differences:
- Device families: iCE40 UltraPlus, CrossLink-NX, CertusPro-NX
- Scripting: Radiant Tcl
- Constraint format: `.lpf` (Logic Preference File), `.pdc`
- IP: Radiant IP catalog (`radiant_ip` generator plugin)

```
crates/loom-radiant/
├── src/
│   ├── lib.rs
│   ├── tcl_gen.rs
│   ├── executor.rs
│   └── env_check.rs
```

### Task 03: Additional Simulator Plugins

**Questa:**
```
crates/loom-questa/
```
Full capabilities: SystemVerilog, VHDL, UVM, fork/join, coverage. UCDB coverage format.

**VCS (Synopsys):**
```
crates/loom-vcs/
```
Full capabilities: SystemVerilog, VHDL, UVM. VDB coverage format.

**Xcelium (Cadence):**
```
crates/loom-xcelium/
```

**Icarus Verilog:**
```
crates/loom-icarus/
```
Limited: Verilog, basic SV, no UVM. Free and open-source. Good for smoke tests.

### Task 04: quartus_qsys Generator

`vivado_bd` equivalent for Quartus. Manages Platform Designer `.qsys` files.

```toml
[[generators]]
name = "pcie_subsystem"
plugin = "quartus_qsys"
[generators.config]
qsys_file = "ip/pcie_subsystem.qsys"
```

Regenerates from canonical `.qsys` XML, avoiding Quartus GUI state pollution.

### Task 05: Documentation, Examples, and Community

**Documentation site:** Generate from rustdoc + Python SDK docstrings. Include:
- Getting started tutorial (5-minute hello FPGA)
- Component authoring guide
- Platform definition guide
- Backend plugin authoring guide
- CI integration examples (GitHub Actions, GitLab CI)

**Example repositories:**
- `loom-examples/simple-ice40`: iCEBreaker board, yosys/nextpnr
- `loom-examples/vivado-ip`: ZCU104 with IP cores
- `loom-examples/multi-board`: Same design targeting ZCU104 + KCU116

**Community plugin guide:** How to publish a backend plugin:
1. Create Python package with `loom.plugins` entry point
2. Implement `BackendPlugin` or `GeneratorPlugin` ABC
3. Publish to PyPI: `pip install loom-mybackend-backend`
4. List in Loom plugin registry

**`loom env dockerfile`:**
```
loom env dockerfile > Dockerfile
```
Generates a Dockerfile with:
- Vivado/Quartus installation scripts (or Nix-based)
- Correct tool version
- Loom installation
- CI-ready entrypoint
