use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{ensure, Context, Result};
use crop_proof::VerifierParams;
use serde::Serialize;

use crate::{
    capture,
    devices::{DeviceInfo, DeviceKind, Devices},
    location::{LocationTool, CIRCUIT_VERSION},
    manifest::Editor,
    receipt::{DeviceKey, TrustedKeys},
    tls::{self, TlsFiles},
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

    /// One PEM per device key a reader accepts.
    pub fn trusted_keys_dir(&self) -> PathBuf {
        self.home.join("trusted")
    }

    pub fn devices(&self) -> Devices {
        Devices::new(self.trusted_keys_dir())
    }

    /// The key and certificate of the TLS listener the app connects to.
    pub fn tls(&self) -> TlsFiles {
        TlsFiles::new(tls::dir(&self.home))
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
            trusted: TrustedKeys::load(&self.trusted_keys_dir())?,
            crop_params: VerifierParams::load(&self.crop_params())?,
            location: self.location_tool(),
        })
    }

    /// Prepares everything the demo needs from the repository at `source`,
    /// or with a `location_proof` built elsewhere, where there is no Go.
    /// Steps whose output already exists are skipped.
    pub fn setup(
        &self,
        source: &Path,
        location_proof: Option<&Path>,
        on: &mut dyn FnMut(&str, Step),
    ) -> Result<()> {
        let binary = self.home.join("bin/location-proof");
        match location_proof {
            Some(prebuilt) => step(on, "install location-proof", false, || {
                fs::create_dir_all(self.home.join("bin"))?;
                // Copy, then rename, so a running serve never sees half a binary.
                let partial = self.home.join("bin/location-proof.partial");
                fs::copy(prebuilt, &partial)
                    .with_context(|| format!("copying {}", prebuilt.display()))?;
                fs::rename(&partial, &binary)?;
                Ok(())
            })?,
            None => step(on, "build location-proof", false, || {
                run(Command::new("go")
                    .args(["build", "-o"])
                    .arg(&binary)
                    .arg(".")
                    .current_dir(source.join("location-proof")))
            })?,
        }

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
            DeviceKey::generate().save(&device)
        })?;
        let devices = self.devices();
        let device = self.device_key()?;
        let id = device.id()?;
        // A home set up before sidecars existed gets one, so the presenter
        // shows the key as the laptop's.
        step(
            on,
            "trust the device key",
            devices.is_trusted(&id) && devices.info(&id).is_some(),
            || {
                let info = DeviceInfo {
                    kind: DeviceKind::Laptop,
                    name: "this laptop".to_string(),
                    paired_at: capture::now()?,
                    attestation: None,
                };
                devices.add(&device.public(), &info).map(drop)
            },
        )?;
        // Also redone when the parameters are for an older circuit.
        let tool = self.location_tool();
        step(
            on,
            "Groth16 setup for location-proof",
            tool.is_set_up(),
            || {
                tool.setup()?;
                ensure!(
                    tool.is_set_up(),
                    "location-proof setup did not write params/location/version {CIRCUIT_VERSION}; it is older than this CLI"
                );
                Ok(())
            },
        )?;
        let tls = self.tls();
        step(on, "TLS key for the app", tls.exists(), || tls.create())?;
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
