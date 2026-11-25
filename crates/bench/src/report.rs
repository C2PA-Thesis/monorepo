//! Turns the samples of a run into `summary.json`, `summary.csv`, `phases.csv`
//! and `report.md`. Reads only the run's directory, so it can run again later.

use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    run::{self, RunInfo, Sample, Status},
    stats::Stats,
    suite::{Case, Group, Op, Scenario},
};

/// HyperVerITAS, Table 3: HV[PST] at 2^19 pixels, 50% crop, on an Apple M3 with 36 GB.
const PAPER_CROP_PROVE_SECONDS: f64 = 16.1;
const PAPER_CROP_VERIFY_SECONDS: f64 = 3.1;
const PAPER_CROP_MEMORY_BYTES: f64 = 6.4e9;
const PAPER_CROP_PROOF_BYTES: f64 = 49.1e3;
/// ZKLP, Table 2: FP32 with Groth16, on an m6i.xlarge with 4 vCPUs (§5).
const PAPER_LOCATION_PROVE_SECONDS: f64 = 0.26;
/// ZKLP, §5: "verifying a Groth16 proof (≈ 1.54 ms)".
const PAPER_LOCATION_VERIFY_SECONDS: f64 = 0.00154;

#[derive(Debug, Serialize)]
pub struct PhaseStats {
    pub name: String,
    pub seconds: Stats,
}

/// One scenario, summarized.
#[derive(Debug, Serialize)]
pub struct Row {
    pub scenario: Scenario,
    pub failed: usize,
    /// Why the scenario took no samples, if it did not.
    pub skipped: Option<String>,
    pub first_error: Option<String>,
    /// The operation itself: the sum of its timed phases.
    pub operation_seconds: Option<Stats>,
    /// The whole process, with start-up and reading the inputs.
    pub process_seconds: Option<Stats>,
    /// User plus system time across every core.
    pub cpu_seconds: Option<Stats>,
    pub peak_rss_bytes: Option<Stats>,
    pub phases: Vec<PhaseStats>,
    pub metrics: BTreeMap<String, u64>,
}

impl Row {
    fn new(scenario: &Scenario, samples: &[Sample]) -> Self {
        let mine: Vec<&Sample> = samples
            .iter()
            .filter(|sample| sample.scenario == scenario.id)
            .collect();
        let ok: Vec<&Sample> = mine
            .iter()
            .copied()
            .filter(|sample| sample.status == Status::Ok)
            .collect();
        let error_of = |status: Status| {
            mine.iter()
                .rev()
                .find(|sample| sample.status == status)
                .and_then(|sample| sample.error.clone())
        };
        let column = |value: fn(&Sample) -> f64| -> Option<Stats> {
            Stats::of(&ok.iter().map(|sample| value(sample)).collect::<Vec<_>>())
        };
        let mut phase_names: Vec<&str> = Vec::new();
        for phase in ok.iter().flat_map(|sample| &sample.phases) {
            if !phase_names.contains(&phase.name.as_str()) {
                phase_names.push(&phase.name);
            }
        }
        let phases = phase_names
            .into_iter()
            .filter_map(|name| {
                let values: Vec<f64> = ok
                    .iter()
                    .flat_map(|sample| &sample.phases)
                    .filter(|phase| phase.name == name)
                    .map(|phase| phase.seconds)
                    .collect();
                Some(PhaseStats {
                    name: name.to_string(),
                    seconds: Stats::of(&values)?,
                })
            })
            .collect();
        let mut metric_values: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for (name, value) in ok.iter().flat_map(|sample| &sample.metrics) {
            metric_values.entry(name).or_default().push(*value as f64);
        }
        Self {
            scenario: scenario.clone(),
            failed: mine.iter().filter(|s| s.status == Status::Failed).count(),
            skipped: ok.is_empty().then(|| error_of(Status::Skipped)).flatten(),
            first_error: error_of(Status::Failed),
            operation_seconds: column(|s| s.phases.iter().map(|phase| phase.seconds).sum()),
            process_seconds: column(|s| s.process_seconds),
            cpu_seconds: column(|s| s.user_seconds + s.system_seconds)
                .filter(|_| !scenario.op.is_phone()),
            peak_rss_bytes: column(|s| s.peak_rss_bytes as f64),
            phases,
            metrics: metric_values
                .into_iter()
                .filter_map(|(name, values)| {
                    Some((name.to_string(), Stats::of(&values)?.median as u64))
                })
                .collect(),
        }
    }

    fn samples(&self) -> usize {
        self.operation_seconds.map_or(0, |stats| stats.count)
    }

    fn phase(&self, name: &str) -> Option<f64> {
        let phase = self.phases.iter().find(|phase| phase.name == name)?;
        Some(phase.seconds.median)
    }
}

pub fn write(dir: &Path) -> Result<()> {
    let info = run::read_info(dir)?;
    let samples = run::read_samples(dir)?;
    let rows: Vec<Row> = info
        .scenarios
        .iter()
        .map(|scenario| Row::new(scenario, &samples))
        .collect();
    let save = |name: &str, contents: &[u8]| {
        fs::write(dir.join(name), contents).with_context(|| format!("writing {name}"))
    };
    save("summary.json", &serde_json::to_vec_pretty(&rows)?)?;
    save("summary.csv", summary_csv(&rows).as_bytes())?;
    save("phases.csv", phases_csv(&rows).as_bytes())?;
    save("report.md", markdown(&info, &rows).as_bytes())?;
    println!("report: {}", dir.join("report.md").display());
    Ok(())
}

fn median(stats: Option<Stats>) -> String {
    stats.map_or(String::new(), |stats| stats.median.to_string())
}

fn summary_csv(rows: &[Row]) -> String {
    let mut csv = String::from(
        "id,op,image,crop,turn,resolution,threads,samples,failed,\
         median_seconds,p25_seconds,p75_seconds,min_seconds,max_seconds,mean_seconds,stddev_seconds,\
         process_median_seconds,cpu_median_seconds,peak_rss_median_bytes,proof_bytes,signed_bytes\n",
    );
    for row in rows {
        let Scenario {
            id,
            op,
            case,
            threads,
            ..
        } = &row.scenario;
        let time = row.operation_seconds;
        let of = |value: fn(&Stats) -> f64| {
            time.as_ref()
                .map_or(String::new(), |t| value(t).to_string())
        };
        let metric = |name: &str| row.metrics.get(name).map_or(String::new(), u64::to_string);
        let fields = [
            id.clone(),
            op.title().replace(',', ""),
            case.image.to_string(),
            case.crop.size().to_string(),
            case.turn.quarters().to_string(),
            case.resolution.to_string(),
            threads.map_or("all".to_string(), |count| count.to_string()),
            row.samples().to_string(),
            row.failed.to_string(),
            of(|t| t.median),
            of(|t| t.p25),
            of(|t| t.p75),
            of(|t| t.min),
            of(|t| t.max),
            of(|t| t.mean),
            of(|t| t.stddev),
            median(row.process_seconds),
            median(row.cpu_seconds),
            median(row.peak_rss_bytes),
            metric("proof_bytes"),
            metric("signed_bytes"),
        ];
        writeln!(csv, "{}", fields.join(",")).expect("writing to a String");
    }
    csv
}

fn phases_csv(rows: &[Row]) -> String {
    let mut csv = String::from("id,phase,samples,median_seconds,p25_seconds,p75_seconds\n");
    for row in rows {
        for PhaseStats { name, seconds } in &row.phases {
            writeln!(
                csv,
                "{},{},{},{},{},{}",
                row.scenario.id,
                name.replace(',', ""),
                seconds.count,
                seconds.median,
                seconds.p25,
                seconds.p75
            )
            .expect("writing to a String");
        }
    }
    csv
}

fn duration(seconds: f64) -> String {
    match seconds {
        s if s < 0.001 => format!("{:.0} µs", s * 1e6),
        s if s < 1.0 => format!("{:.1} ms", s * 1e3),
        s => format!("{s:.2} s"),
    }
}

fn bytes(value: f64) -> String {
    match value {
        v if v < 1e4 => format!("{v:.0} B"),
        v if v < 1e6 => format!("{:.1} KB", v / 1e3),
        v if v < 1e9 => format!("{:.1} MB", v / 1e6),
        v => format!("{:.2} GB", v / 1e9),
    }
}

fn shown(stats: Option<Stats>, format: fn(f64) -> String) -> String {
    stats.map_or("n/a".to_string(), |stats| format(stats.median))
}

/// What distinguishes a scenario from the others of `group`.
fn variant(group: Group, scenario: &Scenario) -> String {
    let case = &scenario.case;
    match group {
        Group::ImageSize => format!("{} ({} pixels)", case.image, case.image.pixels()),
        Group::CropSize => {
            let share = 100.0 * case.crop.size().pixels() as f64 / case.image.pixels() as f64;
            format!("{} ({share:.1}% of the original)", case.crop.size())
        }
        Group::Turn => format!("{} degrees", case.turn.degrees()),
        Group::Resolution => format!("resolution {}", case.resolution),
        Group::Threads => scenario
            .threads
            .map_or("all cores".to_string(), |count| count.to_string()),
        Group::Pipeline | Group::Baseline => "reference".to_string(),
        Group::Phone => case.image.to_string(),
    }
}

fn output(row: &Row) -> String {
    ["proof_bytes", "signed_bytes"]
        .iter()
        .find_map(|name| row.metrics.get(*name))
        .map_or(String::new(), |value| bytes(*value as f64))
}

fn spread(stats: Option<Stats>) -> String {
    stats.map_or(String::new(), |stats| {
        format!("{} to {}", duration(stats.p25), duration(stats.p75))
    })
}

fn group_table(report: &mut String, group: Group, rows: &[&Row]) {
    writeln!(report, "\n## {}\n", group.title()).expect("writing to a String");
    report.push_str(
        "| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |\n\
         | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for row in rows {
        let scenario = &row.scenario;
        if row.skipped.is_some() {
            writeln!(
                report,
                "| {} | {} | 0 | skipped, see Health | | | | | |",
                scenario.op.title(),
                variant(group, scenario)
            )
            .expect("writing to a String");
            continue;
        }
        writeln!(
            report,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            scenario.op.title(),
            variant(group, scenario),
            row.samples(),
            shown(row.operation_seconds, duration),
            spread(row.operation_seconds),
            shown(row.process_seconds, duration),
            shown(row.cpu_seconds, duration),
            shown(row.peak_rss_bytes, bytes),
            output(row),
        )
        .expect("writing to a String");
    }
}

fn phases_table(report: &mut String, rows: &[&Row]) {
    report.push_str(
        "\n## Where the time goes\n\n\
         | Operation | Phase | Median | Share |\n| --- | --- | ---: | ---: |\n",
    );
    for row in rows.iter().filter(|row| row.phases.len() > 1) {
        let total: f64 = row.phases.iter().map(|phase| phase.seconds.median).sum();
        for phase in &row.phases {
            writeln!(
                report,
                "| {} | {} | {} | {:.1}% |",
                row.scenario.op.title(),
                phase.name,
                duration(phase.seconds.median),
                100.0 * phase.seconds.median / total
            )
            .expect("writing to a String");
        }
    }
}

fn reference(rows: &[Row], op: Op) -> Option<&Row> {
    rows.iter().find(|row| {
        let scenario = &row.scenario;
        scenario.op == op && scenario.case == Case::reference() && scenario.threads.is_none()
    })
}

fn comparison(report: &mut String, rows: &[Row]) {
    let ours = |op| reference(rows, op);
    let cell =
        |value: Option<f64>, format: fn(f64) -> String| value.map_or("n/a".to_string(), format);
    let phase = |op, name: &str| ours(op).and_then(|row| row.phase(name));
    let memory = |op| {
        ours(op)
            .and_then(|row| row.peak_rss_bytes)
            .map(|stats| stats.median)
    };
    let metric = |op, name: &str| {
        ours(op)
            .and_then(|row| row.metrics.get(name))
            .map(|value| *value as f64)
    };
    let (crop, upstream_crop) = (Op::CropProve, Op::UpstreamCrop);
    let (location, upstream_location) = (Op::LocationProve, Op::UpstreamLocation);
    let lines = [
        (
            "Crop proof, prover",
            cell(phase(crop, "prove"), duration),
            cell(phase(upstream_crop, "prove"), duration),
            duration(PAPER_CROP_PROVE_SECONDS),
        ),
        (
            "Crop proof, verifier",
            cell(phase(Op::CropVerify, "verify"), duration),
            cell(phase(upstream_crop, "verify"), duration),
            duration(PAPER_CROP_VERIFY_SECONDS),
        ),
        (
            "Crop proof, peak memory",
            cell(memory(crop), bytes),
            cell(memory(upstream_crop), bytes),
            bytes(PAPER_CROP_MEMORY_BYTES),
        ),
        (
            "Crop proof, size",
            cell(metric(crop, "proof_bytes"), bytes),
            cell(metric(upstream_crop, "proof_bytes"), bytes),
            bytes(PAPER_CROP_PROOF_BYTES),
        ),
        (
            "Location proof, prover",
            cell(phase(location, "prove"), duration),
            cell(phase(upstream_location, "prove"), duration),
            duration(PAPER_LOCATION_PROVE_SECONDS),
        ),
        (
            "Location proof, verifier",
            cell(phase(Op::LocationVerify, "verify"), duration),
            cell(phase(upstream_location, "verify"), duration),
            duration(PAPER_LOCATION_VERIFY_SECONDS),
        ),
    ];
    report.push_str(
        "\n## Against upstream and the papers\n\n\
         | Measure | This repository | Upstream, this machine | Paper |\n\
         | --- | ---: | ---: | ---: |\n",
    );
    for (name, ours, upstream, paper) in lines {
        writeln!(report, "| {name} | {ours} | {upstream} | {paper} |")
            .expect("writing to a String");
    }
    report.push_str(
        "\nThe papers used other hardware: an Apple M3 with 36 GB for HyperVerITAS PST at 2^19 \
         pixels, an m6i.xlarge with 4 vCPUs for ZKLP. This repository's location prover is a \
         separate process that loads its keys each time, for proving and for verifying. \
         Upstream's times the proof alone, in a process that already holds them. How much of \
         the difference that accounts for is not measured.\n",
    );
}

fn header(report: &mut String, info: &RunInfo, rows: &[Row]) {
    let environment = &info.environment;
    let text = |value: &Option<String>| value.clone().unwrap_or_else(|| "unknown".to_string());
    let label = info
        .label
        .as_deref()
        .map_or(String::new(), |label| format!(" ({label})"));
    writeln!(report, "# Benchmark run {}{label}\n", info.id).expect("writing to a String");
    let lines = [
        ("Started", info.started_at.clone()),
        (
            "Commit",
            format!(
                "{} on {}",
                text(&environment.commit),
                text(&environment.branch)
            ),
        ),
        (
            "Uncommitted paths",
            environment.uncommitted.len().to_string(),
        ),
        (
            "Machine",
            format!(
                "{}, {} logical cores ({} performance), {}",
                text(&environment.chip),
                environment.logical_cores,
                text(&environment.performance_cores),
                bytes(environment.memory_bytes as f64)
            ),
        ),
        ("System", text(&environment.os)),
        ("Phone", text(&environment.phone)),
        (
            "Power",
            format!(
                "{}, power mode {}",
                text(&environment.power_source),
                text(&environment.power_mode)
            ),
        ),
        (
            "Tools",
            format!(
                "{}, {}, node {}",
                text(&environment.rustc),
                text(&environment.go),
                text(&environment.node)
            ),
        ),
    ];
    for (name, value) in lines {
        writeln!(report, "- **{name}:** {value}").expect("writing to a String");
    }

    let taken: usize = rows.iter().map(Row::samples).sum();
    let failed: usize = rows.iter().map(|row| row.failed).sum();
    let skipped = rows.iter().filter(|row| row.skipped.is_some()).count();
    let noisy: Vec<&str> = rows
        .iter()
        .filter(|row| row.operation_seconds.is_some_and(|stats| stats.is_noisy()))
        .map(|row| row.scenario.id.as_str())
        .collect();
    writeln!(
        report,
        "\n## Health\n\n{taken} samples kept, {failed} failed, {skipped} of {} scenarios skipped.",
        rows.len()
    )
    .expect("writing to a String");
    if !noisy.is_empty() {
        writeln!(
            report,
            "\nNoisy, the middle half spans over 10% of the median: {}.",
            noisy.join(", ")
        )
        .expect("writing to a String");
    }
    for row in rows.iter().filter(|row| row.skipped.is_some()) {
        let reason = row
            .skipped
            .as_deref()
            .unwrap_or("unknown")
            .replace('\n', " ");
        writeln!(report, "\nSkipped, {}: {reason}", row.scenario.id).expect("writing to a String");
    }
    for row in rows.iter().filter(|row| row.failed > 0) {
        let error = row
            .first_error
            .as_deref()
            .unwrap_or("unknown")
            .replace('\n', " ");
        writeln!(report, "\nFailed, {}: {error}", row.scenario.id).expect("writing to a String");
    }
}

fn artifacts(report: &mut String, info: &RunInfo) {
    report.push_str("\n## Files a prover or reader holds\n\n| File | Size |\n| --- | ---: |\n");
    for (name, size) in &info.environment.artifacts {
        writeln!(report, "| {name} | {} |", bytes(*size as f64)).expect("writing to a String");
    }
}

fn markdown(info: &RunInfo, rows: &[Row]) -> String {
    let mut report = String::new();
    header(&mut report, info, rows);
    report.push_str(
        "\nMedian is the operation alone. Middle half is the 25th to 75th percentile. Process \
         adds start-up and reading the inputs. CPU adds up all cores. Peak memory is the \
         process's largest resident set.\n",
    );
    for group in Group::ALL {
        let members: Vec<&Row> = rows
            .iter()
            .filter(|row| row.scenario.groups.contains(&group))
            .collect();
        if members.is_empty() {
            continue;
        }
        group_table(&mut report, group, &members);
        if group == Group::Pipeline {
            let reference: Vec<&Row> = members
                .iter()
                .copied()
                .filter(|row| row.scenario.op != Op::Attacks)
                .collect();
            phases_table(&mut report, &reference);
        }
    }
    if rows
        .iter()
        .any(|row| row.scenario.groups.contains(&Group::Baseline))
    {
        comparison(&mut report, rows);
    }
    artifacts(&mut report, info);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        suite::{scenarios, Depth},
        workload::Phase,
    };

    fn sample(scenario: &Scenario, status: Status, seconds: f64) -> Sample {
        Sample {
            scenario: scenario.id.clone(),
            round: 1,
            status,
            at: String::new(),
            process_seconds: seconds + 1.0,
            user_seconds: 1.0,
            system_seconds: 1.0,
            peak_rss_bytes: 1000,
            load_average: 1.5,
            phases: vec![Phase {
                name: "prove".to_string(),
                seconds,
            }],
            metrics: BTreeMap::from([("proof_bytes".to_string(), 196)]),
            error: None,
        }
    }

    #[test]
    fn warmups_and_failures_stay_out_of_the_statistics() {
        let scenario = &scenarios(&[Group::Turn], Depth::QUICK)[0];
        let mut failure = sample(scenario, Status::Failed, 900.0);
        failure.error = Some("boom".to_string());
        let samples = [
            sample(scenario, Status::Warmup, 900.0),
            failure,
            sample(scenario, Status::Ok, 2.0),
            sample(scenario, Status::Ok, 4.0),
        ];
        let row = Row::new(scenario, &samples);
        assert_eq!(row.samples(), 2);
        assert_eq!(row.failed, 1);
        assert_eq!(row.operation_seconds.unwrap().median, 3.0);
        assert_eq!(row.first_error.as_deref(), Some("boom"));
        assert_eq!(row.metrics["proof_bytes"], 196);
    }

    #[test]
    fn a_skip_is_forgotten_once_the_scenario_has_samples() {
        let scenario = &scenarios(&[Group::Turn], Depth::QUICK)[0];
        let mut skip = sample(scenario, Status::Skipped, 0.0);
        skip.error = Some("no nightly".to_string());
        assert_eq!(
            Row::new(scenario, std::slice::from_ref(&skip))
                .skipped
                .as_deref(),
            Some("no nightly")
        );
        let row = Row::new(scenario, &[skip, sample(scenario, Status::Ok, 2.0)]);
        assert_eq!(row.skipped, None);
    }

    #[test]
    fn sizes_and_times_use_readable_units() {
        assert_eq!(duration(0.0000005), "0 µs");
        assert_eq!(duration(0.0123), "12.3 ms");
        assert_eq!(duration(17.7), "17.70 s");
        assert_eq!(bytes(196.0), "196 B");
        assert_eq!(bytes(36692.0), "36.7 KB");
        assert_eq!(bytes(5.9e9), "5.90 GB");
    }

    #[test]
    fn the_csv_has_one_line_per_scenario_and_equal_columns() {
        let all = scenarios(&[Group::Turn], Depth::QUICK);
        let samples = [sample(&all[0], Status::Ok, 2.0)];
        let rows: Vec<Row> = all.iter().map(|each| Row::new(each, &samples)).collect();
        let csv = summary_csv(&rows);
        let columns: Vec<usize> = csv.lines().map(|line| line.split(',').count()).collect();
        assert_eq!(columns.len(), all.len() + 1);
        assert!(columns.iter().all(|count| *count == columns[0]));
    }
}
