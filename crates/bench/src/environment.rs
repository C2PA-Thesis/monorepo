//! What a result depends on besides the code: the machine, its state and the tools.

use std::{collections::BTreeMap, fs, path::Path, process::Command, thread};

use provenance::Workspace;
use serde::{Deserialize, Serialize};

use crate::phone;

#[derive(Debug, Serialize, Deserialize)]
pub struct Environment {
    pub commit: Option<String>,
    pub branch: Option<String>,
    /// `git status --porcelain`: anything here is not in `commit`.
    pub uncommitted: Vec<String>,
    pub os: Option<String>,
    pub chip: Option<String>,
    pub memory_bytes: u64,
    pub logical_cores: usize,
    pub performance_cores: Option<String>,
    pub power_source: Option<String>,
    /// `powermode` of `pmset`: 0 automatic, 1 low power, 2 high power.
    pub power_mode: Option<String>,
    pub phone: Option<String>,
    pub rustc: Option<String>,
    pub go: Option<String>,
    pub node: Option<String>,
    /// Sizes of the files a prover or a reader must hold, in bytes.
    pub artifacts: BTreeMap<String, u64>,
}

impl Environment {
    pub fn capture(repository: &Path, workspace: &Workspace) -> Self {
        let git = |args: &[&str]| {
            let mut command = vec!["git", "-C"];
            let root = repository.to_string_lossy();
            command.push(&root);
            command.extend(args);
            output(&command)
        };
        Self {
            commit: git(&["rev-parse", "HEAD"]),
            branch: git(&["rev-parse", "--abbrev-ref", "HEAD"]),
            uncommitted: git(&["status", "--porcelain"])
                .map(|text| text.lines().map(str::to_string).collect())
                .unwrap_or_default(),
            os: output(&["sw_vers", "-productVersion"])
                .map(|version| format!("macOS {version}"))
                .or_else(|| output(&["uname", "-sr"])),
            chip: output(&["sysctl", "-n", "machdep.cpu.brand_string"]).or_else(cpu_model),
            memory_bytes: memory_bytes(),
            logical_cores: logical_cores(),
            performance_cores: output(&["sysctl", "-n", "hw.perflevel0.logicalcpu"]),
            power_source: output(&["pmset", "-g", "ps"]).and_then(|text| {
                let line = text.lines().next()?;
                Some(
                    line.trim_start_matches("Now drawing from ")
                        .replace('\'', ""),
                )
            }),
            power_mode: output(&["pmset", "-g"]).and_then(|text| {
                let line = text.lines().find(|line| line.contains("powermode"))?;
                line.split_whitespace().last().map(str::to_string)
            }),
            phone: phone::describe(),
            rustc: output(&["rustc", "--version"]),
            go: output(&["go", "version"]),
            node: output(&["node", "--version"]),
            artifacts: artifacts(workspace),
        }
    }
}

pub fn logical_cores() -> usize {
    thread::available_parallelism().map_or(1, usize::from)
}

fn output(command: &[&str]) -> Option<String> {
    let (program, args) = command.split_first()?;
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    output.status.success().then_some(text)
}

fn cpu_model() -> Option<String> {
    let info = fs::read_to_string("/proc/cpuinfo").ok()?;
    let line = info.lines().find(|line| line.starts_with("model name"))?;
    Some(line.split_once(':')?.1.trim().to_string())
}

fn memory_bytes() -> u64 {
    // SAFETY: sysconf takes an integer and reads no memory.
    let (pages, page) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    pages.max(0) as u64 * page.max(0) as u64
}

fn artifacts(workspace: &Workspace) -> BTreeMap<String, u64> {
    let crop = workspace.crop_params();
    let tool = workspace.location_tool();
    let location = tool.params();
    [
        ("crop prover parameters", crop.join("prover.bin")),
        ("crop verifier parameters", crop.join("verifier.bin")),
        ("location proving key", location.join("proving.key")),
        ("location verifying key", location.join("verifying.key")),
        ("location circuit", location.join("circuit.r1cs")),
    ]
    .into_iter()
    .filter_map(|(name, path)| Some((name.to_string(), fs::metadata(path).ok()?.len())))
    .collect()
}
