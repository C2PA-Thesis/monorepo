//! One crop proof under a memory probe, for the memory hill-climb of the
//! prover (crates/crop-proof/HILLCLIMB.md). Unlike the `crop-prove` sample it
//! can verify the proof it made, and it prints the proof's SHA-256, so a
//! change that breaks the proof cannot pass as a memory win.
//!
//! The peak footprint is the kernel's lifetime maximum for the process, the
//! number `/usr/bin/time -l` prints as "peak memory footprint", read right
//! after the proof so the verification that follows does not count.
//!
//! With the `mem-profile` feature a counting allocator also reports live heap
//! bytes at each phase of the prover and over time.

use std::{fs, path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use crop_proof::{ProverParams, VerifierParams};
use provenance::capture::Capture;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{suite::Case, workload::Lab};

#[derive(Serialize)]
struct Report {
    load_seconds: f64,
    prove_seconds: f64,
    proof_bytes: usize,
    proof_sha256: String,
    /// Kernel lifetime maximum of the footprint, read after the proof.
    footprint_peak: u64,
    /// `None` when the proof was not checked.
    verified: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    phases: Vec<profile::Phase>,
    /// Largest number of live heap bytes, with `mem-profile`.
    #[serde(skip_serializing_if = "Option::is_none")]
    heap_peak: Option<usize>,
}

pub struct Options {
    pub verify: bool,
    pub trace: Option<PathBuf>,
    pub proof_out: Option<PathBuf>,
}

pub fn run(lab: &Lab, case: &Case, options: &Options) -> Result<()> {
    let capture = Capture::read(&lab.fixture(case))?;
    let published = crop_proof::edit(&capture.original, case.crop, case.turn)?;
    profile::start(options.trace.is_some());

    profile::mark("load parameters");
    let started = Instant::now();
    let params = ProverParams::load(&lab.workspace.crop_params())?;
    let load_seconds = started.elapsed().as_secs_f64();

    let started = Instant::now();
    let proof = crop_proof::prove(
        &params,
        &capture.original,
        &published,
        case.crop,
        case.turn,
        &capture.receipt.fingerprint,
    )?;
    let prove_seconds = started.elapsed().as_secs_f64();
    profile::mark("done");
    let footprint_peak = footprint().1;
    drop(params);

    let (phases, heap_peak) = profile::finish(options.trace.as_deref())?;
    if let Some(path) = &options.proof_out {
        fs::write(path, &proof).with_context(|| format!("writing {}", path.display()))?;
    }
    let verified = options
        .verify
        .then(|| {
            let params = VerifierParams::load(&lab.workspace.crop_params())?;
            Ok::<_, anyhow::Error>(
                crop_proof::verify(
                    &params,
                    &published,
                    case.crop,
                    case.turn,
                    &capture.receipt.fingerprint,
                    &proof,
                )
                .is_ok(),
            )
        })
        .transpose()?;
    println!(
        "{}",
        serde_json::to_string(&Report {
            load_seconds,
            prove_seconds,
            proof_bytes: proof.len(),
            proof_sha256: hex::encode(Sha256::digest(&proof)),
            footprint_peak,
            verified,
            phases,
            heap_peak,
        })?
    );
    anyhow::ensure!(verified != Some(false), "the proof does not verify");
    Ok(())
}

/// The footprint of this process now and its lifetime maximum, in bytes.
#[cfg(target_vendor = "apple")]
pub fn footprint() -> (u64, u64) {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: the buffer is a rusage_info_v4, which is what the flavor asks for.
    let status = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            info.as_mut_ptr().cast(),
        )
    };
    if status != 0 {
        return (0, 0);
    }
    // SAFETY: the call succeeded and filled the buffer.
    let info = unsafe { info.assume_init() };
    (info.ri_phys_footprint, info.ri_lifetime_max_phys_footprint)
}

#[cfg(not(target_vendor = "apple"))]
pub fn footprint() -> (u64, u64) {
    (0, 0)
}

#[cfg(not(feature = "mem-profile"))]
mod profile {
    use std::path::Path;

    use anyhow::Result;
    use serde::Serialize;

    #[derive(Serialize)]
    pub struct Phase {}

    pub fn start(_trace: bool) {}
    pub fn mark(_name: &'static str) {}
    pub fn finish(_trace: Option<&Path>) -> Result<(Vec<Phase>, Option<usize>)> {
        Ok((Vec::new(), None))
    }
}

#[cfg(feature = "mem-profile")]
mod profile {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        fmt::Write,
        fs,
        path::Path,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
            Mutex, OnceLock,
        },
        thread,
        time::{Duration, Instant},
    };

    use anyhow::{Context, Result};
    use serde::Serialize;

    use super::footprint;

    struct Counting;

    static LIVE: AtomicUsize = AtomicUsize::new(0);
    /// Largest LIVE since the last mark.
    static PHASE_PEAK: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);

    fn grew(bytes: usize) {
        let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
        PHASE_PEAK.fetch_max(live, Relaxed);
        PEAK.fetch_max(live, Relaxed);
    }

    // SAFETY: every call goes to the system allocator unchanged.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = System.alloc(layout);
            if !pointer.is_null() {
                grew(layout.size());
            }
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = System.alloc_zeroed(layout);
            if !pointer.is_null() {
                grew(layout.size());
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            System.dealloc(pointer, layout);
            LIVE.fetch_sub(layout.size(), Relaxed);
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let new = System.realloc(pointer, layout, new_size);
            if !new.is_null() {
                if new_size >= layout.size() {
                    grew(new_size - layout.size());
                } else {
                    LIVE.fetch_sub(layout.size() - new_size, Relaxed);
                }
            }
            new
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    #[derive(Clone, Serialize)]
    pub struct Phase {
        name: &'static str,
        seconds: f64,
        /// Live heap bytes when the phase started.
        heap_start: usize,
        /// Largest number of live heap bytes during the phase.
        heap_peak: usize,
        /// Footprint when the phase ended.
        footprint_end: u64,
        /// Largest footprint the sampler saw during the phase.
        footprint_peak: u64,
    }

    struct State {
        origin: Instant,
        marks: Vec<(&'static str, f64, usize)>,
        closed: Vec<Phase>,
        samples: Vec<(f64, usize, u64)>,
    }

    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    static SAMPLING: AtomicBool = AtomicBool::new(false);

    fn state() -> &'static Mutex<State> {
        STATE.get_or_init(|| {
            Mutex::new(State {
                origin: Instant::now(),
                marks: Vec::new(),
                closed: Vec::new(),
                samples: Vec::new(),
            })
        })
    }

    pub fn start(_trace: bool) {
        crop_proof::probe::on_mark(mark);
        state();
        SAMPLING.store(true, Relaxed);
        thread::spawn(|| {
            while SAMPLING.load(Relaxed) {
                let sample = {
                    let state = state().lock().expect("probe state");
                    (
                        state.origin.elapsed().as_secs_f64(),
                        LIVE.load(Relaxed),
                        footprint().0,
                    )
                };
                state().lock().expect("probe state").samples.push(sample);
                thread::sleep(Duration::from_millis(2));
            }
        });
    }

    pub fn mark(name: &'static str) {
        let live = LIVE.load(Relaxed);
        let phase_peak = PHASE_PEAK.swap(live, Relaxed);
        let footprint_end = footprint().0;
        let mut state = state().lock().expect("probe state");
        let now = state.origin.elapsed().as_secs_f64();
        if let Some(&(previous, started, heap_start)) = state.marks.last() {
            let footprint_peak = state
                .samples
                .iter()
                .rev()
                .take_while(|(at, ..)| *at >= started)
                .map(|&(_, _, footprint)| footprint)
                .max()
                .unwrap_or(0)
                .max(footprint_end);
            state.closed.push(Phase {
                name: previous,
                seconds: now - started,
                heap_start,
                heap_peak: phase_peak,
                footprint_end,
                footprint_peak,
            });
        }
        state.marks.push((name, now, live));
    }

    pub fn finish(trace: Option<&Path>) -> Result<(Vec<Phase>, Option<usize>)> {
        SAMPLING.store(false, Relaxed);
        let state = state().lock().expect("probe state");
        if let Some(path) = trace {
            let mut csv = String::from("seconds,heap_bytes,footprint_bytes,phase\n");
            let mut marks = state.marks.iter().peekable();
            let mut phase = "";
            for &(at, heap, footprint) in &state.samples {
                while let Some(&&(name, started, _)) = marks.peek() {
                    if started > at {
                        break;
                    }
                    phase = name;
                    marks.next();
                }
                writeln!(csv, "{at:.4},{heap},{footprint},{phase}").expect("writing to a string");
            }
            fs::write(path, csv).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok((state.closed.clone(), Some(PEAK.load(Relaxed))))
    }
}
