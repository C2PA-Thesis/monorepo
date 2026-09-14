use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::capture::Secrets;

/// The Go `location-proof` binary and its Groth16 parameters.
pub struct LocationTool {
    binary: PathBuf,
    params: PathBuf,
}

/// The public tuple the circuit checks a coordinate against.
#[derive(Debug, Serialize, Deserialize)]
pub struct Region {
    pub cell: String,
    pub resolution: u8,
    pub face: u8,
    pub i: i64,
    pub j: i64,
    pub k: i64,
}

#[derive(Debug, Deserialize)]
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

    pub fn is_set_up(&self) -> bool {
        self.params.join("verifying.key").exists()
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

    pub fn commit(&self, secrets: &Secrets) -> Result<String> {
        Ok(self
            .json::<Commitment>(self.command("commit"), Some(secrets))?
            .envelope)
    }

    pub fn prove(&self, secrets: &Secrets, cell: &str) -> Result<LocationProof> {
        let mut command = self.with_params("prove");
        command.args(["--cell", cell]);
        self.json(command, Some(secrets))
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

    fn json<T: DeserializeOwned>(&self, command: Command, secrets: Option<&Secrets>) -> Result<T> {
        let stdout = self.run(command, secrets)?;
        serde_json::from_slice(&stdout).context("location-proof printed unexpected output")
    }

    fn run(&self, mut command: Command, secrets: Option<&Secrets>) -> Result<Vec<u8>> {
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
        let input = secrets
            .map(serde_json::to_vec)
            .transpose()?
            .unwrap_or_default();
        child
            .stdin
            .take()
            .context("location-proof stdin")?
            .write_all(&input)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            bail!("{}", message.trim().trim_start_matches("location-proof: "));
        }
        Ok(output.stdout)
    }
}
