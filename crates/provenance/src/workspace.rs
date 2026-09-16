use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{ensure, Context, Result};
use crop_proof::VerifierParams;
use serde::Serialize;

use crate::{
    capture::{self, DeviceKey},
    location::LocationTool,
    manifest::Editor,
    verify::Verifier,
};

/// C2PA SDK sample certificate, key and photo at the c2patool release the
/// C2P-19 round trip used. Public test material, so it is downloaded rather
/// than committed.
const SAMPLES: &str =
    "https://raw.githubusercontent.com/contentauth/c2pa-rs/c2patool-v0.27.16/cli/sample";

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Running,
    Done,
    AlreadyPresent,
}

/// Local state: the built location prover, proof parameters, keys and samples.
pub struct Workspace {
    home: PathBuf,
}

impl Workspace {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        // Setup runs tools from other directories, so relative paths would move.
        Self {
            home: std::path::absolute(&home).unwrap_or(home),
        }
    }

    pub fn sample_photo(&self) -> PathBuf {
        self.home.join("sample.jpg")
    }

    pub fn location_tool(&self) -> LocationTool {
        LocationTool::new(
            self.home.join("bin/location-proof"),
            self.home.join("params/location"),
        )
    }

    pub fn crop_params(&self) -> PathBuf {
        self.home.join("params/crop")
    }

    pub fn device_public_key(&self) -> PathBuf {
        self.home.join("device.pub.pem")
    }

    pub fn device_key(&self) -> Result<DeviceKey> {
        DeviceKey::load(&self.home.join("device.pem")).context("run `provenance setup` first")
    }

    pub fn editor(&self) -> Result<Editor> {
        Editor::load(
            &self.home.join("c2pa/es256_certs.pem"),
            &self.home.join("c2pa/es256_private.key"),
        )
    }

    pub fn verifier(&self) -> Result<Verifier> {
        Ok(Verifier {
            device: capture::load_public_key(&self.device_public_key())?,
            crop_params: VerifierParams::load(&self.crop_params())?,
            location: self.location_tool(),
        })
    }

    /// Prepares everything the demo needs from the repository at `source`.
    /// Steps whose output already exists are skipped.
    pub fn setup(&self, source: &Path, on: &mut dyn FnMut(&str, Step)) -> Result<()> {
        step(on, "build location-proof", false, || {
            run(Command::new("go")
                .args(["build", "-o"])
                .arg(self.home.join("bin/location-proof"))
                .arg(".")
                .current_dir(source.join("location-proof")))
        })?;

        for (file, sample) in [
            ("c2pa/es256_certs.pem", "es256_certs.pem"),
            ("c2pa/es256_private.key", "es256_private.key"),
            ("sample.jpg", "image.jpg"),
        ] {
            let path = self.home.join(file);
            step(
                on,
                &format!("download sample {sample}"),
                path.exists(),
                || {
                    fs::create_dir_all(path.parent().expect("sample paths have a parent"))?;
                    run(Command::new("curl")
                        .args(["-fsSL", "-o"])
                        .arg(&path)
                        .arg(format!("{SAMPLES}/{sample}")))
                },
            )?;
        }

        let device = self.home.join("device.pem");
        step(on, "generate the device key", device.exists(), || {
            DeviceKey::generate().save(&device, &self.device_public_key())
        })?;
        let tool = self.location_tool();
        step(
            on,
            "Groth16 setup for location-proof",
            tool.is_set_up(),
            || tool.setup(),
        )?;
        let crop_params = self.crop_params();
        step(
            on,
            "PST setup for crop-proof",
            crop_params.join("metadata.json").exists(),
            || crop_proof::setup(&crop_params),
        )
    }
}

fn step(
    on: &mut dyn FnMut(&str, Step),
    name: &str,
    already_present: bool,
    work: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if already_present {
        on(name, Step::AlreadyPresent);
        return Ok(());
    }
    on(name, Step::Running);
    work().with_context(|| format!("{name} failed"))?;
    on(name, Step::Done);
    Ok(())
}

fn run(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .output()
        .with_context(|| format!("running {program}"))?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}
