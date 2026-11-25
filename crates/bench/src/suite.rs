//! What gets measured. Every sweep changes one input and holds the others at
//! the reference case, so a difference between two rows has one cause.

use crop_proof::{Rect, Size, Turn};
use serde::{Deserialize, Serialize};

/// The paper's smallest size, and the largest original the crop proof takes.
const REFERENCE_IMAGE: Size = Size {
    width: 1024,
    height: 512,
};
/// The original of a 4:3 photo, as the app renders it.
const APP_IMAGE: Size = Size {
    width: 836,
    height: 627,
};
const REFERENCE_RESOLUTION: u8 = 7;

/// The inputs of one measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub image: Size,
    pub crop: Rect,
    pub turn: Turn,
    pub resolution: u8,
}

impl Case {
    pub fn reference() -> Case {
        Case::half_of(REFERENCE_IMAGE)
    }

    fn half_of(image: Size) -> Case {
        Case {
            image,
            crop: Rect::left_half(image),
            turn: Turn::NONE,
            resolution: REFERENCE_RESOLUTION,
        }
    }

    /// Names the case's fixture directory.
    pub fn key(&self) -> String {
        let Case {
            image,
            crop,
            turn,
            resolution,
        } = self;
        format!(
            "{image}-crop{}-turn{}-res{resolution}",
            crop.size(),
            turn.quarters()
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    /// Capture to verified file, and each operation alone, at the reference case.
    Pipeline,
    /// Proof cost as the original grows.
    ImageSize,
    /// Proof cost as the published rectangle grows.
    CropSize,
    /// Proof cost for each quarter turn.
    Turn,
    /// Location proof cost for each H3 resolution the pipeline allows.
    Resolution,
    /// Proof cost with fewer cores.
    Threads,
    /// The upstream code on this machine, and the fingerprint as WebAssembly.
    Baseline,
    /// What the app does at capture, on a paired iPhone.
    Phone,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::Pipeline,
        Group::ImageSize,
        Group::CropSize,
        Group::Turn,
        Group::Resolution,
        Group::Threads,
        Group::Baseline,
        Group::Phone,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Pipeline => "Pipeline at the reference case",
            Group::ImageSize => "Image size sweep",
            Group::CropSize => "Crop size sweep",
            Group::Turn => "Turn sweep",
            Group::Resolution => "H3 resolution sweep",
            Group::Threads => "Thread sweep",
            Group::Baseline => "Baselines",
            Group::Phone => "On the iPhone",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    /// Everything a sample of this case reads. Not measured.
    Prepare,
    Pipeline,
    Fingerprint,
    Capture,
    LocationProve,
    LocationVerify,
    CropProve,
    CropVerify,
    Sign,
    Read,
    Attacks,
    UpstreamCrop,
    UpstreamLocation,
    WasmFingerprint,
    PhoneFingerprint,
    PhoneEnvelope,
    PhoneLocationProve,
}

impl Op {
    /// Measured by the phone itself, not by a process on this machine.
    pub fn is_phone(self) -> bool {
        matches!(
            self,
            Op::PhoneFingerprint | Op::PhoneEnvelope | Op::PhoneLocationProve
        )
    }

    /// Needs something this machine may lack, so a skip is not a failure.
    pub fn is_optional(self) -> bool {
        self.is_phone()
            || matches!(
                self,
                Op::UpstreamCrop | Op::UpstreamLocation | Op::WasmFingerprint
            )
    }

    pub fn title(self) -> &'static str {
        match self {
            Op::Prepare => "prepare",
            Op::Pipeline => "capture to verified file",
            Op::Fingerprint => "fingerprint",
            Op::Capture => "capture",
            Op::LocationProve => "location proof",
            Op::LocationVerify => "location verify",
            Op::CropProve => "crop proof",
            Op::CropVerify => "crop verify",
            Op::Sign => "C2PA sign",
            Op::Read => "reader verification",
            Op::Attacks => "all attacks",
            Op::UpstreamCrop => "upstream HyperVerITAS PST",
            Op::UpstreamLocation => "upstream ZKLP Groth16",
            Op::WasmFingerprint => "fingerprint, WebAssembly",
            Op::PhoneFingerprint => "fingerprint",
            Op::PhoneEnvelope => "envelope",
            Op::PhoneLocationProve => "location proof",
        }
    }

    fn slug(self) -> String {
        let quoted = serde_json::to_string(&self).expect("an Op serializes");
        quoted.trim_matches('"').to_string()
    }
}

/// One thing to measure, listed under every group it belongs to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub op: Op,
    pub case: Case,
    /// Thread limit for the proof libraries. All cores when absent.
    pub threads: Option<usize>,
    pub samples: u32,
    pub groups: Vec<Group>,
}

/// How many samples each kind of scenario takes.
#[derive(Clone, Copy, Debug)]
pub struct Depth {
    /// Reference operations: the numbers the thesis quotes.
    pub reference: u32,
    /// Sweep points: enough to see a trend.
    pub sweep: u32,
    /// Operations under a second, where more samples cost almost nothing.
    pub fast: u32,
}

impl Depth {
    pub const FULL: Depth = Depth {
        reference: 10,
        sweep: 5,
        fast: 30,
    };
    pub const QUICK: Depth = Depth {
        reference: 2,
        sweep: 2,
        fast: 2,
    };

    pub fn uniform(samples: u32) -> Depth {
        Depth {
            reference: samples,
            sweep: samples,
            fast: samples,
        }
    }
}

struct Builder {
    depth: Depth,
    scenarios: Vec<Scenario>,
}

impl Builder {
    /// Adds a scenario, or adds `group` to the same measurement listed earlier.
    fn add(&mut self, group: Group, op: Op, case: Case, threads: Option<usize>, samples: u32) {
        let limit = threads.map_or(String::new(), |count| format!("-threads{count}"));
        let id = format!("{}-{}{limit}", op.slug(), case.key());
        match self.scenarios.iter_mut().find(|each| each.id == id) {
            Some(known) => {
                known.groups.push(group);
                known.samples = known.samples.max(samples);
            }
            None => self.scenarios.push(Scenario {
                id,
                op,
                case,
                threads,
                samples,
                groups: vec![group],
            }),
        }
    }

    fn pipeline(&mut self) {
        let Depth {
            reference, fast, ..
        } = self.depth;
        let case = Case::reference();
        for (op, samples) in [
            (Op::Pipeline, reference),
            (Op::Fingerprint, fast),
            (Op::Capture, fast),
            (Op::LocationProve, fast),
            (Op::LocationVerify, fast),
            (Op::CropProve, reference),
            (Op::CropVerify, reference),
            (Op::Sign, fast),
            (Op::Read, reference),
            (Op::Attacks, reference),
        ] {
            self.add(Group::Pipeline, op, case, None, samples);
        }
    }

    fn crop_operations(&mut self, group: Group, case: Case) {
        for op in [Op::CropProve, Op::CropVerify] {
            self.add(group, op, case, None, self.depth.sweep);
        }
    }

    fn image_size(&mut self) {
        let powers = [15, 16, 17, 18].map(|power| Size {
            width: 1 << (power / 2 + power % 2),
            height: 1 << (power / 2),
        });
        for image in powers.into_iter().chain([REFERENCE_IMAGE, APP_IMAGE]) {
            let case = Case::half_of(image);
            self.add(
                Group::ImageSize,
                Op::Fingerprint,
                case,
                None,
                self.depth.sweep,
            );
            self.crop_operations(Group::ImageSize, case);
        }
    }

    fn crop_size(&mut self) {
        for (width, height) in [(64, 64), (256, 256), (512, 512), (1024, 512)] {
            let crop = Rect {
                x: 0,
                y: 0,
                width,
                height,
            };
            let case = Case {
                crop,
                ..Case::reference()
            };
            self.crop_operations(Group::CropSize, case);
        }
    }

    fn turn(&mut self) {
        for quarters in 0..4 {
            let turn = Turn::new(quarters).expect("0 to 3 quarter turns are valid");
            let case = Case {
                turn,
                ..Case::reference()
            };
            self.crop_operations(Group::Turn, case);
        }
    }

    fn resolution(&mut self) {
        for resolution in provenance::location::RESOLUTIONS {
            let case = Case {
                resolution,
                ..Case::reference()
            };
            for op in [Op::LocationProve, Op::LocationVerify] {
                self.add(Group::Resolution, op, case, None, self.depth.fast);
            }
        }
    }

    fn threads(&mut self) {
        let case = Case::reference();
        for limit in [1, 2, 4, 8] {
            let threads = Some(limit);
            self.add(
                Group::Threads,
                Op::CropProve,
                case,
                threads,
                self.depth.sweep,
            );
            self.add(
                Group::Threads,
                Op::LocationProve,
                case,
                threads,
                self.depth.sweep,
            );
        }
        self.add(Group::Threads, Op::CropProve, case, None, self.depth.sweep);
        self.add(
            Group::Threads,
            Op::LocationProve,
            case,
            None,
            self.depth.sweep,
        );
    }

    fn baseline(&mut self) {
        let case = Case::reference();
        let Depth {
            reference, fast, ..
        } = self.depth;
        self.add(Group::Baseline, Op::UpstreamCrop, case, None, reference);
        self.add(Group::Baseline, Op::UpstreamLocation, case, None, reference);
        self.add(Group::Baseline, Op::WasmFingerprint, case, None, fast);
    }

    fn phone(&mut self) {
        let case = Case::half_of(APP_IMAGE);
        for op in [
            Op::PhoneFingerprint,
            Op::PhoneEnvelope,
            Op::PhoneLocationProve,
        ] {
            self.add(Group::Phone, op, case, None, self.depth.reference);
        }
    }
}

/// The scenarios of `groups`, in the order they first get measured.
pub fn scenarios(groups: &[Group], depth: Depth) -> Vec<Scenario> {
    let mut builder = Builder {
        depth,
        scenarios: Vec::new(),
    };
    for group in Group::ALL.into_iter().filter(|each| groups.contains(each)) {
        match group {
            Group::Pipeline => builder.pipeline(),
            Group::ImageSize => builder.image_size(),
            Group::CropSize => builder.crop_size(),
            Group::Turn => builder.turn(),
            Group::Resolution => builder.resolution(),
            Group::Threads => builder.threads(),
            Group::Baseline => builder.baseline(),
            Group::Phone => builder.phone(),
        }
    }
    builder.scenarios
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn scenario_ids_are_unique() {
        let all = scenarios(&Group::ALL, Depth::FULL);
        let ids: HashSet<&str> = all.iter().map(|each| each.id.as_str()).collect();
        assert_eq!(ids.len(), all.len());
    }

    #[test]
    fn a_measurement_shared_by_sweeps_runs_once_with_the_most_samples() {
        let all = scenarios(&Group::ALL, Depth::FULL);
        let reference = all
            .iter()
            .find(|each| {
                each.op == Op::CropProve && each.case == Case::reference() && each.threads.is_none()
            })
            .unwrap();
        assert_eq!(reference.samples, Depth::FULL.reference);
        assert_eq!(
            reference.groups,
            [
                Group::Pipeline,
                Group::ImageSize,
                Group::CropSize,
                Group::Turn,
                Group::Threads
            ]
        );
    }

    #[test]
    fn every_case_is_a_valid_statement() {
        for scenario in scenarios(&Group::ALL, Depth::FULL) {
            let Case { image, crop, .. } = scenario.case;
            image.check().unwrap();
            crop.check(image).unwrap();
        }
    }

    #[test]
    fn only_selected_groups_are_built() {
        let some = scenarios(&[Group::Turn], Depth::QUICK);
        assert!(some.iter().all(|each| each.groups == [Group::Turn]));
        assert_eq!(some.len(), 8);
    }
}
