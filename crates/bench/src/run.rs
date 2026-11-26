//! Takes the samples of a run. Results reach the disk one sample at a time, so
//! an interrupted run loses at most the sample in progress and `--resume`
//! continues it. A failing sample never stops the others.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{ensure, Context, Result};
use provenance::Workspace;
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    baseline,
    environment::{logical_cores, Environment},
    measure::{self, Exit},
    phone, report,
    suite::{Case, Op, Scenario},
    workload::{Lab, Measured, Phase, Task},
};

const INFO: &str = "run.json";
const SAMPLES: &str = "samples.jsonl";
/// After this many failures in a row a scenario stops, so a broken operation
/// costs minutes, not a night of timeouts.
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
const ERROR_TAIL_BYTES: usize = 600;
/// A fixture is one full publish, which takes one crop proof.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Samples back to back heat the machine: crop proofs slowed by 8% in two
/// minutes and by more over a run, and rests of 30 s removed that.
const MAX_REST: Duration = Duration::from_secs(20);
const SETTLE_WAIT: Duration = Duration::from_secs(5 * 60);
const SETTLE_POLL: Duration = Duration::from_secs(15);

/// Everything needed to take, resume and report a run.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunInfo {
    pub id: String,
    pub label: Option<String>,
    pub started_at: String,
    pub timeout_seconds: u64,
    pub environment: Environment,
    pub scenarios: Vec<Scenario>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Round 0 of each scenario: it warms the file cache and is not reported.
    Warmup,
    Ok,
    Failed,
    /// The scenario could not start, for the reason in `error`.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub scenario: String,
    pub round: u32,
    pub status: Status,
    pub at: String,
    pub process_seconds: f64,
    pub user_seconds: f64,
    pub system_seconds: f64,
    pub peak_rss_bytes: u64,
    /// One-minute load average when the sample started.
    pub load_average: f64,
    pub phases: Vec<Phase>,
    pub metrics: BTreeMap<String, u64>,
    pub error: Option<String>,
}

impl Sample {
    fn empty(scenario: &Scenario, round: u32, status: Status, error: Option<String>) -> Self {
        Self {
            scenario: scenario.id.clone(),
            round,
            status,
            at: now(),
            process_seconds: 0.0,
            user_seconds: 0.0,
            system_seconds: 0.0,
            peak_rss_bytes: 0,
            load_average: load_average(),
            phases: Vec::new(),
            metrics: Default::default(),
            error,
        }
    }
}

pub fn now() -> String {
    OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("zero is a valid nanosecond")
        .format(&Rfc3339)
        .expect("a UTC time formats as RFC 3339")
}

fn load_average() -> f64 {
    let mut load = [0.0];
    // SAFETY: the buffer holds the one value asked for.
    unsafe { libc::getloadavg(load.as_mut_ptr(), 1) };
    load[0]
}

/// The last bytes of `text`, where a failing command says what went wrong.
pub fn tail(text: &str) -> String {
    let text = text.trim();
    let start = text.len().saturating_sub(ERROR_TAIL_BYTES);
    let start = (start..text.len())
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text[start..].to_string()
}

pub fn write_info(dir: &Path, info: &RunInfo) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join(INFO), serde_json::to_vec_pretty(info)?)
        .with_context(|| format!("writing {INFO}"))
}

pub fn read_info(dir: &Path) -> Result<RunInfo> {
    let path = dir.join(INFO);
    serde_json::from_slice(&fs::read(&path).with_context(|| format!("reading {}", path.display()))?)
        .with_context(|| format!("parsing {}", path.display()))
}

/// The samples on disk. A process killed mid-write leaves a partial last line;
/// that sample was not finished, so it is dropped and taken again.
pub fn read_samples(dir: &Path) -> Result<Vec<Sample>> {
    let path = dir.join(SAMPLES);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

struct Log {
    file: File,
}

impl Log {
    fn open(dir: &Path) -> Result<Self> {
        let path = dir.join(SAMPLES);
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        // An interrupted write has no newline, and the next line must not join it.
        if fs::read(&path)?.last().is_some_and(|byte| *byte != b'\n') {
            writeln!(file)?;
        }
        Ok(Self { file })
    }

    fn append(&mut self, sample: &Sample) -> Result<()> {
        writeln!(self.file, "{}", serde_json::to_string(sample)?)?;
        self.file.sync_data().context("flushing the sample")
    }
}

pub struct Paths {
    pub repository: PathBuf,
    pub results: PathBuf,
    pub work: PathBuf,
    pub home: PathBuf,
}

struct Engine<'a> {
    info: &'a RunInfo,
    paths: &'a Paths,
    lab: Lab,
    log: Log,
    /// Why a scenario takes no more samples.
    stopped: HashMap<String, String>,
    failures_in_a_row: HashMap<String, u32>,
    done: HashSet<(String, u32)>,
}

pub fn take(info: &RunInfo, paths: &Paths) -> Result<()> {
    fs::create_dir_all(&paths.work)
        .with_context(|| format!("creating {}", paths.work.display()))?;
    let done = read_samples(&paths.results)?
        .into_iter()
        .filter(|sample| sample.status != Status::Skipped)
        .map(|sample| (sample.scenario, sample.round))
        .collect();
    let mut engine = Engine {
        info,
        paths,
        lab: Lab {
            workspace: Workspace::new(&paths.home),
            work: paths.work.clone(),
        },
        log: Log::open(&paths.results)?,
        stopped: HashMap::new(),
        failures_in_a_row: HashMap::new(),
        done,
    };
    engine.prepare_fixtures()?;
    engine.prepare_baselines()?;
    engine.sample_all()?;
    engine.sample_phone()?;
    report::write(&paths.results)?;
    check_outcome(info, &read_samples(&paths.results)?)
}

/// A run that lost samples must not look like a clean one to a script or to CI.
fn check_outcome(info: &RunInfo, samples: &[Sample]) -> Result<()> {
    let failed = samples
        .iter()
        .filter(|sample| sample.status == Status::Failed)
        .count();
    let empty = info
        .scenarios
        .iter()
        .filter(|each| !each.op.is_optional())
        .filter(|each| {
            !samples
                .iter()
                .any(|sample| sample.scenario == each.id && sample.status == Status::Ok)
        })
        .count();
    ensure!(
        failed == 0 && empty == 0,
        "{failed} samples failed and {empty} scenarios have no samples; see the report"
    );
    Ok(())
}

impl Engine<'_> {
    fn laptop(&self) -> impl Iterator<Item = &Scenario> {
        self.info
            .scenarios
            .iter()
            .filter(|each| !each.op.is_phone())
    }

    fn skip(&mut self, scenario: &Scenario, reason: String) -> Result<()> {
        println!("  skipping {}: {reason}", scenario.id);
        self.log.append(&Sample::empty(
            scenario,
            0,
            Status::Skipped,
            Some(reason.clone()),
        ))?;
        self.stopped.insert(scenario.id.clone(), reason);
        Ok(())
    }

    /// A case that cannot be published skips its scenarios, not the run.
    fn prepare_fixtures(&mut self) -> Result<()> {
        let info = self.info;
        let mut cases: Vec<Case> = Vec::new();
        for scenario in info.scenarios.iter().filter(|each| !each.op.is_phone()) {
            if !cases.contains(&scenario.case) {
                cases.push(scenario.case);
            }
        }
        for (index, case) in cases.iter().enumerate() {
            println!(
                "preparing fixture {}/{}: {}",
                index + 1,
                cases.len(),
                case.key()
            );
            let task = Task {
                op: Op::Prepare,
                case: *case,
            };
            if let Err(error) = self.spawn(&task, None, PREPARE_TIMEOUT).and_then(finish) {
                let affected = |each: &&Scenario| each.case == *case && !each.op.is_phone();
                for scenario in info.scenarios.iter().filter(affected) {
                    self.skip(scenario, format!("preparing the fixture: {error:#}"))?;
                }
            }
        }
        Ok(())
    }

    fn prepare_baselines(&mut self) -> Result<()> {
        let info = self.info;
        for scenario in &info.scenarios {
            if self.stopped.contains_key(&scenario.id) {
                continue;
            }
            if let Err(error) = baseline::prepare(
                &self.lab,
                scenario.op,
                &scenario.case,
                &self.paths.repository,
            ) {
                self.skip(scenario, format!("{error:#}"))?;
            }
        }
        Ok(())
    }

    fn sample_all(&mut self) -> Result<()> {
        let info = self.info;
        let rounds = self.laptop().map(|each| each.samples).max().unwrap_or(0);
        let planned = self.planned();
        let started = Instant::now();
        let mut taken = 0;
        // Round by round, so a slow drift such as heat reaches every scenario
        // alike instead of whichever ran last.
        for round in 0..=rounds {
            for scenario in info.scenarios.iter().filter(|each| !each.op.is_phone()) {
                if round > scenario.samples
                    || self.stopped.contains_key(&scenario.id)
                    || self.done.contains(&(scenario.id.clone(), round))
                {
                    continue;
                }
                let sample = self.sample(scenario, round)?;
                taken += 1;
                print_progress(scenario, &sample, taken, planned, started.elapsed());
                self.log.append(&sample)?;
                self.track(scenario, &sample);
                thread::sleep(rest_after(&sample));
            }
        }
        Ok(())
    }

    /// The phone takes all the samples of a scenario in one test. A scenario
    /// is done when its last round is on disk; any other is measured again
    /// from round 0.
    fn sample_phone(&mut self) -> Result<()> {
        let info = self.info;
        let wanted: Vec<&Scenario> = info
            .scenarios
            .iter()
            .filter(|each| each.op.is_phone())
            .filter(|each| !self.done.contains(&(each.id.clone(), each.samples)))
            .collect();
        let Some(samples) = wanted.iter().map(|each| each.samples).max() else {
            return Ok(());
        };
        println!("measuring on the iPhone");
        let ops: Vec<Op> = wanted.iter().map(|each| each.op).collect();
        let location = self.lab.workspace.location_tool().params().to_path_buf();
        let crop = self.lab.workspace.crop_params();
        let parameters = phone::Parameters {
            location: &location,
            crop: &crop,
        };
        let (repository, work) = (&self.paths.repository, &self.paths.work);
        match phone::measure(repository, &parameters, work, &ops, samples) {
            Ok(batches) => {
                for batch in &batches {
                    for scenario in wanted.iter().filter(|each| batch.ops.contains(&each.op)) {
                        for sample in phone_samples(scenario, batch, &self.done) {
                            if let Some(error) = &sample.error {
                                println!("  {} round {}: {error}", scenario.id, sample.round);
                            }
                            self.log.append(&sample)?;
                        }
                    }
                }
            }
            Err(error) => {
                for scenario in wanted {
                    self.skip(scenario, format!("{error:#}"))?;
                }
            }
        }
        Ok(())
    }

    fn planned(&self) -> usize {
        self.laptop()
            .filter(|each| !self.stopped.contains_key(&each.id))
            .map(|each| {
                (0..=each.samples)
                    .filter(|round| !self.done.contains(&(each.id.clone(), *round)))
                    .count()
            })
            .sum()
    }

    fn track(&mut self, scenario: &Scenario, sample: &Sample) {
        let failures = self
            .failures_in_a_row
            .entry(scenario.id.clone())
            .or_default();
        match sample.status {
            Status::Failed => *failures += 1,
            _ => *failures = 0,
        }
        if *failures >= MAX_CONSECUTIVE_FAILURES {
            let reason = format!("{MAX_CONSECUTIVE_FAILURES} failures in a row");
            println!("  stopping {}: {reason}", scenario.id);
            self.stopped.insert(scenario.id.clone(), reason);
        }
    }

    fn sample(&self, scenario: &Scenario, round: u32) -> Result<Sample> {
        let task = Task {
            op: scenario.op,
            case: scenario.case,
        };
        let load = load_average();
        let timeout = Duration::from_secs(self.info.timeout_seconds);
        let usage = match self.spawn(&task, scenario.threads, timeout) {
            Ok(usage) => usage,
            Err(error) => {
                let reason = Some(format!("{error:#}"));
                return Ok(Sample::empty(scenario, round, Status::Failed, reason));
            }
        };
        let mut sample = Sample {
            load_average: load,
            process_seconds: usage.wall_seconds,
            user_seconds: usage.user_seconds,
            system_seconds: usage.system_seconds,
            peak_rss_bytes: usage.peak_rss_bytes,
            ..Sample::empty(scenario, round, Status::Failed, None)
        };
        match finish(usage) {
            Ok(measured) => {
                sample.phases = measured.phases;
                sample.metrics = measured.metrics;
                sample.status = match round {
                    0 => Status::Warmup,
                    _ => Status::Ok,
                };
            }
            Err(error) => sample.error = Some(format!("{error:#}")),
        }
        Ok(sample)
    }

    fn spawn(
        &self,
        task: &Task,
        threads: Option<usize>,
        timeout: Duration,
    ) -> Result<measure::Usage> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--home")
            .arg(&self.paths.home)
            .arg("sample")
            .arg("--work")
            .arg(&self.paths.work)
            .arg(serde_json::to_string(task)?);
        if let Some(limit) = threads {
            command
                .env("RAYON_NUM_THREADS", limit.to_string())
                .env("GOMAXPROCS", limit.to_string());
        }
        measure::run(&mut command, timeout)
    }
}

/// As long as the sample ran, so heavy samples cool down and fast ones cost nothing.
fn rest_after(sample: &Sample) -> Duration {
    Duration::from_secs_f64(sample.process_seconds).min(MAX_REST)
}

fn phone_sample(scenario: &Scenario, reading: &phone::Reading) -> Sample {
    let status = match reading.round {
        0 => Status::Warmup,
        _ => Status::Ok,
    };
    let mut metrics = BTreeMap::from([("thermal_state".to_string(), reading.thermal_state)]);
    if reading.bytes > 0 {
        metrics.insert("proof_bytes".to_string(), reading.bytes);
    }
    Sample {
        process_seconds: reading.seconds,
        peak_rss_bytes: reading.peak_footprint_bytes,
        phases: vec![Phase {
            name: scenario.op.title().to_string(),
            seconds: reading.seconds,
        }],
        metrics,
        ..Sample::empty(scenario, reading.round, status, None)
    }
}

/// What one phone test adds to the log for `scenario`: the rounds it
/// finished, without the rounds in `done`, which a resumed run measures
/// again. A test that stopped early adds the round it did not finish as a
/// failure, so the report shows it and the run does not pass for a clean
/// one. A test without any round skips the scenario, as a missing phone does.
fn phone_samples(
    scenario: &Scenario,
    batch: &phone::Batch,
    done: &HashSet<(String, u32)>,
) -> Vec<Sample> {
    let readings = batch.readings.iter().filter(|each| each.op == scenario.op);
    let rounds = readings.clone().map(|each| each.round + 1).max();
    let mut samples: Vec<Sample> = readings
        .filter(|each| !done.contains(&(scenario.id.clone(), each.round)))
        .map(|each| phone_sample(scenario, each))
        .collect();
    if let Some(failure) = &batch.failure {
        let (round, status) = match rounds {
            Some(next) => (next, Status::Failed),
            None => (0, Status::Skipped),
        };
        samples.push(Sample::empty(
            scenario,
            round,
            status,
            Some(failure.clone()),
        ));
    }
    samples
}

/// What the sample's process printed, or why it has nothing to report.
fn finish(usage: measure::Usage) -> Result<Measured> {
    match usage.outcome {
        Exit::Success => {
            let line = usage
                .stdout
                .lines()
                .last()
                .context("the sample printed nothing")?;
            serde_json::from_str(line).context("parsing the sample's output")
        }
        Exit::TimedOut => anyhow::bail!("timed out after {:.0} s", usage.wall_seconds),
        Exit::Failed(why) => anyhow::bail!("{why}: {}", tail(&usage.stderr)),
    }
}

fn print_progress(
    scenario: &Scenario,
    sample: &Sample,
    taken: usize,
    planned: usize,
    elapsed: Duration,
) {
    let left = elapsed.as_secs_f64() / taken as f64 * (planned - taken) as f64;
    let state = match sample.status {
        Status::Failed => format!("FAILED: {}", sample.error.as_deref().unwrap_or("unknown")),
        Status::Warmup => "warmup".to_string(),
        _ => String::new(),
    };
    println!(
        "[{taken}/{planned}, about {:.0} min left] {} round {}: {:.2} s {state}",
        left / 60.0,
        scenario.id,
        sample.round,
        sample.process_seconds,
    );
}

/// Waits for the build's own load to fade, so the first samples do not pay for it.
fn settle() {
    let limit = logical_cores() as f64 / 4.0;
    let started = Instant::now();
    while load_average() > limit && started.elapsed() < SETTLE_WAIT {
        println!(
            "waiting for a quiet machine: load average {:.1}",
            load_average()
        );
        thread::sleep(SETTLE_POLL);
    }
    if load_average() > limit {
        println!("warning: the machine is still busy; other work will distort the results");
    }
}

/// Warns about what would disturb the numbers before hours are spent on them.
pub fn check_machine(environment: &Environment) {
    settle();
    if environment.power_mode.as_deref() == Some("1") {
        println!("warning: Low Power Mode is on; proofs will be slower");
    }
    if let Some(source) = &environment.power_source {
        if !source.contains("AC") {
            println!("warning: running on {source}; connect power for stable results");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite::{scenarios, Depth, Group};

    fn sample(id: &str, round: u32) -> Sample {
        let scenario = &scenarios(&[Group::Turn], Depth::QUICK)[0];
        Sample {
            scenario: id.to_string(),
            ..Sample::empty(scenario, round, Status::Ok, None)
        }
    }

    #[test]
    fn appended_samples_read_back_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Log::open(dir.path()).unwrap();
        log.append(&sample("a", 1)).unwrap();
        log.append(&sample("b", 2)).unwrap();
        let read = read_samples(dir.path()).unwrap();
        // Each sample is stamped when it is built, so the two can differ.
        let expected: Vec<Sample> = [sample("a", 1), sample("b", 2)]
            .into_iter()
            .zip(&read)
            .map(|(mut each, got)| {
                each.at = got.at.clone();
                each.load_average = got.load_average;
                each
            })
            .collect();
        assert_eq!(read, expected);
    }

    #[test]
    fn a_partial_last_line_is_dropped_and_the_log_continues_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Log::open(dir.path()).unwrap();
        log.append(&sample("kept", 1)).unwrap();
        drop(log);
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.path().join(SAMPLES))
            .unwrap();
        write!(file, "{{\"scenario\":\"cut").unwrap();
        drop(file);

        let mut log = Log::open(dir.path()).unwrap();
        log.append(&sample("after", 2)).unwrap();
        let names: Vec<String> = read_samples(dir.path())
            .unwrap()
            .into_iter()
            .map(|each| each.scenario)
            .collect();
        assert_eq!(names, ["kept", "after"]);
    }

    fn crop_reading(round: u32) -> phone::Reading {
        phone::Reading {
            op: Op::PhoneCropProve,
            round,
            seconds: 40.0,
            bytes: 36692,
            peak_footprint_bytes: 1 << 30,
            thermal_state: 0,
        }
    }

    #[test]
    fn a_phone_test_that_stops_early_keeps_its_rounds_and_fails_the_next() {
        let all = scenarios(&[Group::Phone], Depth::QUICK);
        let crop = all.iter().find(|each| each.op == Op::PhoneCropProve);
        let (crop, capture) = (crop.unwrap(), &all[0]);
        let batch = phone::Batch {
            ops: &[Op::PhoneCropProve],
            readings: vec![crop_reading(0), crop_reading(1)],
            failure: Some("killed".to_string()),
        };
        let rounds = |samples: &[Sample]| -> Vec<(u32, Status)> {
            samples
                .iter()
                .map(|each| (each.round, each.status))
                .collect()
        };

        let samples = phone_samples(crop, &batch, &HashSet::new());
        assert_eq!(
            rounds(&samples),
            [(0, Status::Warmup), (1, Status::Ok), (2, Status::Failed)]
        );
        assert_eq!(samples[1].peak_rss_bytes, 1 << 30);
        assert_eq!(samples[2].error.as_deref(), Some("killed"));

        // A resumed run has rounds 0 and 1 on disk already.
        let done = HashSet::from([(crop.id.clone(), 0), (crop.id.clone(), 1)]);
        let complete = phone::Batch {
            readings: (0..=2).map(crop_reading).collect(),
            failure: None,
            ..batch
        };
        assert_eq!(
            rounds(&phone_samples(crop, &complete, &done)),
            [(2, Status::Ok)]
        );

        // A scenario without any round is skipped, with the reason.
        let skipped = phone_samples(capture, &batch, &HashSet::new());
        assert_eq!(rounds(&skipped), [(0, Status::Skipped)]);
        assert_eq!(skipped[0].error.as_deref(), Some("killed"));
    }

    #[test]
    fn rest_follows_the_sample_up_to_a_limit() {
        let mut fast = sample("a", 1);
        fast.process_seconds = 0.5;
        assert_eq!(rest_after(&fast), Duration::from_millis(500));
        let mut slow = sample("a", 1);
        slow.process_seconds = 300.0;
        assert_eq!(rest_after(&slow), MAX_REST);
    }

    #[test]
    fn no_samples_file_means_no_samples() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_samples(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn tail_keeps_the_end_and_respects_characters() {
        let text = "é".repeat(ERROR_TAIL_BYTES);
        let kept = tail(&text);
        assert!(kept.len() <= ERROR_TAIL_BYTES);
        assert!(kept.ends_with('é'));
        assert_eq!(tail("  short\n"), "short");
    }
}
