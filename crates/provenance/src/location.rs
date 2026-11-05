use std::{
    fs,
    io::Write,
    ops::RangeInclusive,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::capture::{Coordinate, Secrets};

/// H3 resolutions a capture may claim, everywhere: the API, publishing and
/// the reader. At 4 a cell averages about 1,770 km², at 9 about 0.1 km² (H3
/// cell statistics). Coarser says little about the place; finer is close to
/// the accuracy a phone reports for its fix and narrows the place more than a
/// published photo should.
pub const RESOLUTIONS: RangeInclusive<u8> = 4..=9;

/// What `location-proof setup` writes to `version` in the parameters
/// directory. Parameters without it are for the circuit that took its
/// trigonometric hints unsealed, and are set up again.
pub const CIRCUIT_VERSION: &str = "sealed-hints-v1";

/// The parameter files the app downloads to prove on the phone.
pub const PROVER_FILES: [&str; 2] = ["proving.key", "circuit.r1cs"];

/// Refuses a resolution outside [`RESOLUTIONS`].
pub fn check_resolution(resolution: u8) -> Result<()> {
    ensure!(
        RESOLUTIONS.contains(&resolution),
        "resolution {resolution} is outside the allowed range {} to {}",
        RESOLUTIONS.start(),
        RESOLUTIONS.end()
    );
    Ok(())
}

/// The resolution of an H3 cell index, read from its bits.
pub fn cell_resolution(cell: &str) -> Result<u8> {
    let index = u64::from_str_radix(cell, 16)
        .ok()
        .filter(|_| cell.len() == 15 && cell.chars().all(|c| c.is_ascii_hexdigit()))
        .with_context(|| format!("{cell} is not an H3 cell index"))?;
    ensure!((index >> 59) & 0xf == 1, "{cell} is not an H3 cell index");
    Ok(((index >> 52) & 0xf) as u8)
}

/// Refuses a cell whose resolution is outside [`RESOLUTIONS`].
pub fn check_cell(cell: &str) -> Result<()> {
    let resolution = cell_resolution(cell)?;
    check_resolution(resolution).with_context(|| format!("cell {cell}"))
}

/// The Go `location-proof` binary and its Groth16 parameters.
pub struct LocationTool {
    binary: PathBuf,
    params: PathBuf,
}

/// The public tuple the circuit checks a coordinate against.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub cell: String,
    pub resolution: u8,
    pub face: u8,
    pub i: i64,
    pub j: i64,
    pub k: i64,
}

/// A cell's corners and area, for drawing it on a map.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outline {
    pub cell: String,
    pub resolution: u8,
    pub area_km2: f64,
    /// Latitude, longitude pairs in degrees.
    pub boundary: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationProof {
    pub envelope: String,
    /// Base64 BN254 Groth16 proof.
    pub proof: String,
}

#[derive(Deserialize)]
struct Commitment {
    envelope: String,
}

impl LocationTool {
    pub fn new(binary: PathBuf, params: PathBuf) -> Self {
        Self { binary, params }
    }

    /// Whether the parameters exist and are for the current circuit.
    pub fn is_set_up(&self) -> bool {
        self.params.join("verifying.key").exists()
            && fs::read_to_string(self.params.join("version"))
                .is_ok_and(|version| version.trim() == CIRCUIT_VERSION)
    }

    pub fn params(&self) -> &Path {
        &self.params
    }

    pub fn setup(&self) -> Result<()> {
        self.run(self.with_params("setup"), None).map(drop)
    }

    /// Fails for cells the circuit cannot represent.
    pub fn region(&self, cell: &str) -> Result<Region> {
        let mut command = self.command("region");
        command.args(["--cell", cell]);
        self.json(command, None)
    }

    /// The cell at `resolution` containing `coordinate`, as the circuit maps
    /// it. Fails for cells the circuit cannot represent and for resolutions
    /// outside [`RESOLUTIONS`].
    pub fn cell(&self, coordinate: Coordinate, resolution: u8) -> Result<Region> {
        check_resolution(resolution)?;
        let mut command = self.command("cell");
        command.args(["--resolution", &resolution.to_string()]);
        self.json(command, Some(serde_json::to_vec(&coordinate)?))
    }

    pub fn outline(&self, cell: &str) -> Result<Outline> {
        let mut command = self.command("outline");
        command.args(["--cell", cell]);
        self.json(command, None)
    }

    pub fn commit(&self, secrets: &Secrets) -> Result<String> {
        let input = serde_json::to_vec(secrets)?;
        Ok(self
            .json::<Commitment>(self.command("commit"), Some(input))?
            .envelope)
    }

    pub fn prove(&self, secrets: &Secrets, cell: &str) -> Result<LocationProof> {
        let mut command = self.with_params("prove");
        command.args(["--cell", cell]);
        self.json(command, Some(serde_json::to_vec(secrets)?))
    }

    pub fn verify(&self, cell: &str, envelope: &str, proof: &str) -> Result<()> {
        let mut command = self.with_params("verify");
        command.args(["--cell", cell, "--envelope", envelope, "--proof", proof]);
        self.run(command, None).map(drop)
    }

    fn command(&self, name: &str) -> Command {
        let mut command = Command::new(&self.binary);
        command.arg(name);
        command
    }

    fn with_params(&self, name: &str) -> Command {
        let mut command = self.command(name);
        command.arg("--params").arg(&self.params);
        command
    }

    fn json<T: DeserializeOwned>(&self, command: Command, input: Option<Vec<u8>>) -> Result<T> {
        let stdout = self.run(command, input)?;
        serde_json::from_slice(&stdout).context("location-proof printed unexpected output")
    }

    /// Runs the tool with `input` on stdin, so secrets stay out of the process list.
    fn run(&self, mut command: Command, input: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "running {}; run `provenance setup` first",
                    self.binary.display()
                )
            })?;
        child
            .stdin
            .take()
            .context("location-proof stdin")?
            .write_all(&input.unwrap_or_default())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            bail!("{}", message.trim().trim_start_matches("location-proof: "));
        }
        Ok(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_resolution_from_the_index() {
        assert_eq!(cell_resolution("87c2e3020ffffff").unwrap(), 7);
        assert_eq!(cell_resolution("872f5a32dffffff").unwrap(), 7);
        assert_eq!(cell_resolution("8a2a1072b59ffff").unwrap(), 10);
        assert_eq!(cell_resolution("8428309ffffffff").unwrap(), 4);
        assert!(cell_resolution("not a cell").is_err());
        assert!(cell_resolution("87c2e3020fffff").is_err());
        // Mode bits that are not a cell's.
        assert!(cell_resolution("1172e3020ffffff").is_err());
    }

    #[test]
    fn parameters_without_the_current_version_are_not_set_up() {
        let dir = tempfile::tempdir().unwrap();
        let tool = LocationTool::new(PathBuf::from("location-proof"), dir.path().to_path_buf());
        fs::write(dir.path().join("verifying.key"), b"").unwrap();
        assert!(!tool.is_set_up(), "parameters from before the version file");
        fs::write(dir.path().join("version"), b"unsealed\n").unwrap();
        assert!(!tool.is_set_up(), "parameters for another circuit");
        fs::write(dir.path().join("version"), format!("{CIRCUIT_VERSION}\n")).unwrap();
        assert!(tool.is_set_up());
    }

    #[test]
    fn allows_four_to_nine() {
        assert!(check_resolution(3).is_err());
        assert!(check_resolution(4).is_ok());
        assert!(check_resolution(9).is_ok());
        let error = check_cell("8a2a1072b59ffff").unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "cell 8a2a1072b59ffff: resolution 10 is outside the allowed range 4 to 9"
        );
    }
}
