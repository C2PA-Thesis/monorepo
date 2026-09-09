import { useEffect, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowRight,
  ArrowUpRight,
  Check,
  ChevronDown,
  CircleHelp,
  Download,
  Eye,
  Focus,
  ImagePlus,
  LoaderCircle,
  Plus,
  ScanLine,
  ShieldCheck,
  Square,
  Upload,
  X,
} from "lucide-react";
import { useLab } from "./useLab";
import { LocationPanel } from "./LocationPanel";
import { MatrixView } from "./MatrixView";
import { EvidenceView, TamperView } from "./EvidenceView";
import { asset, busy, bytes, DEFAULT_LOCATION, seconds, STAGES } from "./types";
import type { Location, Region } from "./types";

const tabs = [
  ["experiment", "01", "Experiment"],
  ["matrix", "02", "Inside the pixels"],
  ["evidence", "03", "Evidence"],
  ["tamper", "04", "Try to break it"],
];

function Elapsed({ started }: { started: string | null }) {
  const [time, setTime] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setTime(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return (
    <>
      {started
        ? seconds(Math.max(0, (time - Date.parse(started)) / 1000))
        : "Waiting"}
    </>
  );
}

export default function App() {
  const [mode, setMode] = useState<"capture" | "reader">("capture");
  const lab = useLab(mode === "reader");
  const [tab, setTab] = useState("experiment");
  const [location, setLocation] = useState<Location>(DEFAULT_LOCATION);
  const [region, setRegion] = useState<Region | null>(null);
  const [imageMode, setImageMode] = useState("crop");
  const [reveal, setReveal] = useState(50);
  const [dragging, setDragging] = useState(false);
  const [stageIndex, setStageIndex] = useState<number | null>(null);
  const [showAbout, setShowAbout] = useState(false);
  const uploadInput = useRef<HTMLInputElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const { run, reader } = lab;
  const active = busy(run?.status);
  const frozen = !!run && run.status !== "draft";
  const viewingReader = mode === "reader";
  const finished = run?.status === "complete";
  const selectedStage =
    stageIndex === null
      ? run?.stages.find((s) => s.status === "running")
      : run?.stages[stageIndex];
  const latest = [...lab.events].reverse().find((event) => event.message);
  const selectedEvents = lab.events
    .filter(
      (event) =>
        event.message && (!selectedStage || event.stage === selectedStage.id),
    )
    .slice(-8);

  useEffect(() => {
    if (run?.config)
      setLocation({
        latitude: run.config.latitude,
        longitude: run.config.longitude,
        resolution: run.config.resolution,
      });
  }, [run?.id, run?.config]);
  useEffect(() => {
    if (showAbout) dialog.current?.showModal();
    else dialog.current?.close();
  }, [showAbout]);

  const upload = (file?: File) => {
    if (file && file.size > 20 * 1024 * 1024) {
      uploadInput.current?.setCustomValidity(
        "Choose an image smaller than 20 MB.",
      );
      uploadInput.current?.reportValidity();
      return;
    }
    setMode("capture");
    setTab("experiment");
    setStageIndex(null);
    void lab.upload(file);
  };
  const newExperiment = () => uploadInput.current?.click();

  return (
    <div className="app-shell">
      <header className="topbar">
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setTab("experiment");
          }}
          aria-label="Provenance Lab home"
        >
          <span className="brand-mark">
            <i />
            <i />
            <i />
            <i />
          </span>
          <span>
            PROVENANCE
            <br />
            <b>LAB</b>
          </span>
        </a>
        <div className="topbar-center">
          <span className="tiny-cross">+</span> A PHOTOGRAPH, WITH EVIDENCE{" "}
          <span className="tiny-cross">+</span>
        </div>
        <div className="topbar-actions">
          <span className="system-status">
            <i className={lab.ready ? "live-dot" : "live-dot offline"} />
            {lab.ready ? "LOCAL ENGINE READY" : "ENGINE UNAVAILABLE"}
          </span>
          <button
            aria-label="About the lab"
            className="about-button"
            onClick={() => setShowAbout(true)}
          >
            <CircleHelp size={17} />
            <span>About the lab</span>
          </button>
        </div>
      </header>
      <main>
        <section className="hero">
          <div className="hero-copy">
            <span className="eyebrow">
              <span className="orange-square" /> EXPERIMENT NO.{" "}
              {run ? run.id.slice(0, 6).toUpperCase() : "001"} / C2PA THESIS
            </span>
            <h1>
              From pixels
              <br />
              to <span>proof.</span>
              <span className="title-star">✳</span>
            </h1>
            <p>
              One photograph. Two proofs. A story you can verify.
              <br />
              Follow the evidence from capture to publication.
            </p>
          </div>
          <div className="hero-graphic" aria-hidden="true">
            <div className="orbit orbit-one" />
            <div className="orbit orbit-two" />
            <div className="orbit orbit-three" />
            <div className="orbital-axis" />
            <div className="orbital-axis second" />
            <div className="orbital-core">
              <ScanLine size={38} strokeWidth={0.9} />
            </div>
            <span className="orbit-label top">CAPTURE</span>
            <span className="orbit-label bottom">VERIFY</span>
            <i className="orbit-point p1" />
            <i className="orbit-point p2" />
            <span className="graphic-coordinate">
              3 PROTOCOLS
              <br />1 CAPTURE
            </span>
          </div>
          <div className="hero-aside">
            <span className="micro">RESEARCH PROTOTYPE / 01</span>
            <div>
              <span>HyperVerITAS</span>
              <span>ZKLP</span>
              <span>C2PA</span>
            </div>
            <button
              onClick={() =>
                document
                  .getElementById("workspace")
                  ?.scrollIntoView({ behavior: "smooth" })
              }
              aria-label="Go to experiment"
            >
              <ArrowDown size={24} strokeWidth={1} />
            </button>
          </div>
        </section>

        <nav
          className="workspace-tabs"
          id="workspace"
          aria-label="Lab sections"
        >
          {tabs.map(([id, number, label]) => (
            <button
              key={id}
              className={tab === id ? "active" : ""}
              aria-current={tab === id ? "page" : undefined}
              onClick={() => setTab(id)}
            >
              <span>{number}</span>
              {label}
              <ArrowUpRight size={16} />
            </button>
          ))}
        </nav>
        <div className="workspace-toolbar">
          <div className="view-toggle" aria-label="Evidence perspective">
            <button
              aria-pressed={!viewingReader}
              className={!viewingReader ? "selected" : ""}
              onClick={() => setMode("capture")}
            >
              <Focus size={14} /> Capture view
            </button>
            <button
              aria-pressed={viewingReader}
              className={viewingReader ? "selected" : ""}
              onClick={() => setMode("reader")}
            >
              <Eye size={14} /> Reader view
            </button>
          </div>
          <div className="experiment-controls">
            {lab.history.length > 0 && (
              <label className="history-select">
                <span className="sr-only">Saved experiment</span>
                <select
                  aria-label="Saved experiment"
                  value={run?.id || ""}
                  onChange={(e) => {
                    const item = lab.history.find(
                      (value) => value.id === e.target.value,
                    );
                    if (item) {
                      lab.select(item);
                      setStageIndex(null);
                    }
                  }}
                >
                  <option value="" disabled>
                    Saved experiments
                  </option>
                  {lab.history.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name.slice(0, 30)} · {item.status}
                    </option>
                  ))}
                </select>
                <ChevronDown size={13} />
              </label>
            )}
            <button
              className="text-button"
              onClick={newExperiment}
              disabled={lab.working}
            >
              <Plus size={15} /> New experiment
            </button>
          </div>
        </div>
        <input
          className="sr-only"
          type="file"
          accept="image/png,image/jpeg,image/webp"
          ref={uploadInput}
          aria-label="Upload an image"
          onChange={(e) => {
            uploadInput.current?.setCustomValidity("");
            if (e.target.files?.[0]) upload(e.target.files[0]);
            e.target.value = "";
          }}
        />
        {lab.error && (
          <div className="error-banner" role="alert">
            <X size={17} />
            <span>{lab.error}</span>
          </div>
        )}
        {!lab.connected && run && (
          <div className="notice" role="status">
            Reconnecting to live progress. Your proof job continues on this
            computer.
          </div>
        )}
        {viewingReader && (
          <div className="reader-notice">
            <Eye size={15} />
            <span>
              Reader perspective: the published crop, public region, and
              evidence. The original image and exact capture point are hidden.
            </span>
          </div>
        )}

        {tab === "experiment" && (
          <>
            <div className="experiment-grid">
              <section className="image-panel">
                <div className="panel-heading">
                  <span>
                    <ScanLine size={15} />{" "}
                    {viewingReader ? "Published photograph" : "Your photograph"}
                  </span>
                  <span className="micro">
                    01 / {viewingReader ? "PUBLIC" : "CAPTURE"}
                  </span>
                </div>
                {run && (!viewingReader || reader?.published) ? (
                  <>
                    <div
                      className={`image-stage ${imageMode === "original" || viewingReader ? "plain-image" : ""}`}
                    >
                      <div className="image-grid-lines" />
                      <div className="image-frame">
                        <img
                          src={asset(
                            run.id,
                            viewingReader ? "signed.png" : "prepared.png",
                          )}
                          alt={
                            viewingReader
                              ? "Published crop with embedded C2PA evidence"
                              : "Normalized original photograph"
                          }
                          draggable="false"
                        />
                        {!viewingReader && imageMode === "crop" && (
                          <>
                            <div className="crop-removed">
                              <span>
                                REMOVED
                                <br />
                                <b>50%</b>
                              </span>
                            </div>
                            <div className="crop-kept">
                              <span>KEEP / 512 × 512</span>
                            </div>
                            <div className="crop-divider" />
                          </>
                        )}
                        {!viewingReader && imageMode === "compare" && (
                          <>
                            <div
                              className="comparison-after"
                              style={{ clipPath: `inset(0 0 0 ${reveal}%)` }}
                            >
                              <img
                                src={asset(run.id, "crop.png")}
                                alt="Actual fixed left crop preview"
                              />
                            </div>
                            <span className="comparison-label before">
                              ORIGINAL
                            </span>
                            <span className="comparison-label after">
                              AFTER LEFT CROP
                            </span>
                            <div
                              className="comparison-line"
                              style={{ left: `${reveal}%` }}
                            >
                              <span>↔</span>
                            </div>
                          </>
                        )}
                        <i className="corner top-left" />
                        <i className="corner top-right" />
                        <i className="corner bottom-left" />
                        <i className="corner bottom-right" />
                      </div>
                      <span className="image-stage-label">
                        {viewingReader
                          ? "PUBLISHED / 512 × 512 / PNG"
                          : "NORMALIZED ORIGINAL / 1024 × 512 / RGB"}
                      </span>
                    </div>
                    {!viewingReader && (
                      <div className="image-tools">
                        <div className="segmented">
                          {["original", "crop", "compare"].map((value) => (
                            <button
                              key={value}
                              aria-pressed={imageMode === value}
                              className={imageMode === value ? "selected" : ""}
                              onClick={() => setImageMode(value)}
                            >
                              {value === "crop"
                                ? "Apply left crop"
                                : value === "compare"
                                  ? "Compare"
                                  : "Original"}
                            </button>
                          ))}
                        </div>
                        {imageMode === "compare" ? (
                          <input
                            type="range"
                            aria-label="Image comparison position"
                            min="0"
                            max="100"
                            value={reveal}
                            onChange={(e) => setReveal(+e.target.value)}
                          />
                        ) : (
                          <span className="micro">
                            FIXED LEFT HALF · LOSSLESS PNG
                          </span>
                        )}
                      </div>
                    )}
                    <div className="file-info">
                      <div>
                        <ImagePlus size={17} />
                        <span>
                          <b>{viewingReader ? "signed.png" : run.name}</b>
                          <small>
                            {viewingReader
                              ? `${bytes(reader?.sizes.signed_png)} · embedded provenance`
                              : `${run.source.width} × ${run.source.height} ${run.source.format} → 1024 × 512 RGB`}
                          </small>
                        </span>
                      </div>
                      {viewingReader ? (
                        <a
                          href={asset(run.id, "signed.png", true)}
                          aria-label="Download signed image"
                        >
                          <Download size={17} />
                        </a>
                      ) : (
                        <button onClick={newExperiment} disabled={lab.working}>
                          Replace <ArrowUpRight size={13} />
                        </button>
                      )}
                    </div>
                    <div className="capture-explanation">
                      <span className="tiny-cross">+</span>
                      <p>
                        {viewingReader
                          ? "This is the actual signed PNG. Its pixels, receipt, and proofs are checked together by the reader."
                          : "Your upload becomes the simulated capture. The supported edit keeps its left half. Changing the photograph or location after capture requires a new experiment."}
                      </p>
                    </div>
                  </>
                ) : viewingReader ? (
                  <div className="upload-zone">
                    <Eye size={35} strokeWidth={1} />
                    <h2>Nothing published yet.</h2>
                    <p>
                      Complete a capture and proof run to see exactly what a
                      reader receives.
                    </p>
                    <button
                      className="outline-button"
                      onClick={() => setMode("capture")}
                    >
                      Return to capture <ArrowRight size={15} />
                    </button>
                  </div>
                ) : (
                  <div
                    className={`upload-zone ${dragging ? "dragging" : ""}`}
                    onDragOver={(e) => {
                      e.preventDefault();
                      setDragging(true);
                    }}
                    onDragLeave={() => setDragging(false)}
                    onDrop={(e) => {
                      e.preventDefault();
                      setDragging(false);
                      if (e.dataTransfer.files[0])
                        upload(e.dataTransfer.files[0]);
                    }}
                  >
                    <div className="upload-crosshair">
                      <ImagePlus size={31} strokeWidth={1} />
                    </div>
                    <span className="micro">START WITH A PHOTOGRAPH</span>
                    <h2>
                      Every image
                      <br />
                      has a beginning.
                    </h2>
                    <p>
                      Drop yours here, or explore with our sample.
                      <br />
                      PNG, JPEG or WebP · up to 20 MB
                    </p>
                    <div>
                      <button
                        className="dark-button"
                        disabled={lab.working}
                        onClick={newExperiment}
                      >
                        {lab.working ? (
                          <LoaderCircle className="spin" size={15} />
                        ) : (
                          <Upload size={15} />
                        )}{" "}
                        Upload image
                      </button>
                      <button
                        className="text-button"
                        disabled={lab.working}
                        onClick={() => upload()}
                      >
                        Use sample <ArrowUpRight size={14} />
                      </button>
                    </div>
                    <span className="upload-footnote">
                      PROCESSED BY THE LOCAL ENGINE ON YOUR COMPUTER
                    </span>
                  </div>
                )}
              </section>
              <LocationPanel
                location={
                  viewingReader && run?.config
                    ? {
                        ...location,
                        latitude: run.config.region.center[0],
                        longitude: run.config.region.center[1],
                        cell: run.config.region.cell,
                      }
                    : location
                }
                onChange={setLocation}
                onRegion={setRegion}
                locked={frozen}
                reader={viewingReader}
              />
            </div>
            <div className="run-command">
              <div>
                <span className="eyebrow">
                  {active
                    ? "REAL COMPUTATION IN PROGRESS"
                    : finished
                      ? "EXPERIMENT COMPLETE"
                      : "READY WHEN YOU ARE"}
                </span>
                <h3>
                  {active
                    ? run?.activity === "pipeline"
                      ? "Following the evidence."
                      : "Checking the published evidence."
                    : finished
                      ? "The evidence checks out."
                      : "Make this photograph verifiable."}
                </h3>
                <p>
                  {active
                    ? "You can explore the lab while the engine works. Progress reflects actual stage events."
                    : finished
                      ? "Inspect the proofs, download the signed PNG, or try changing the evidence."
                      : "Capture → crop → two proofs → a signed PNG → independent verification."}
                </p>
              </div>
              <div className="run-action">
                {active ? (
                  <>
                    <span className="elapsed">
                      <LoaderCircle size={16} className="spin" />
                      <Elapsed started={run?.started_at || null} />
                    </span>
                    <button
                      className="outline-button"
                      onClick={() => void lab.cancel()}
                    >
                      <Square size={12} /> Stop run
                    </button>
                  </>
                ) : finished ? (
                  <button
                    className="orange-button"
                    onClick={() => setTab("evidence")}
                  >
                    Inspect the evidence <ArrowUpRight size={20} />
                  </button>
                ) : run?.status === "failed" || run?.status === "cancelled" ? (
                  <button className="orange-button" onClick={() => upload()}>
                    Start a new sample <ArrowUpRight size={20} />
                  </button>
                ) : (
                  <button
                    className="orange-button"
                    disabled={
                      !run ||
                      !region?.supported ||
                      !region?.contains_point ||
                      lab.working ||
                      !lab.ready ||
                      viewingReader
                    }
                    onClick={() => void lab.start(location)}
                  >
                    {lab.working ? (
                      <LoaderCircle size={17} className="spin" />
                    ) : (
                      <ShieldCheck size={18} />
                    )}{" "}
                    Generate real proofs <ArrowUpRight size={20} />
                  </button>
                )}
                <span className="micro">PST + GROTH16 · RUNS LOCALLY</span>
              </div>
            </div>
          </>
        )}

        {tab === "matrix" && <MatrixView run={run} reader={viewingReader} />}
        {tab === "evidence" && (
          <EvidenceView
            run={run}
            reader={reader}
            onVerify={() => void lab.action("verify")}
            working={lab.working}
          />
        )}
        {tab === "tamper" && (
          <TamperView
            run={run}
            reader={reader}
            onAttack={(name) => void lab.action(name)}
            working={lab.working}
          />
        )}

        <section className="pipeline-section">
          <div className="pipeline-heading">
            <span className="eyebrow">
              <span className="orange-square" /> THE PIPELINE
            </span>
            <span className="micro">
              {run?.stages.filter((stage) => stage.status === "complete")
                .length || 0}{" "}
              / 8 STAGES COMPLETE
            </span>
          </div>
          <div className="pipeline-track">
            {STAGES.map((title, index) => {
              const stage = run?.stages[index];
              return (
                <button
                  key={title}
                  className={`pipeline-step ${stage?.status || ""} ${stageIndex === index ? "inspected" : ""}`}
                  onClick={() => setStageIndex(index)}
                >
                  <span className="step-top">
                    <span>
                      {stage?.status === "complete" ? (
                        <Check size={13} />
                      ) : stage?.status === "running" ? (
                        <LoaderCircle size={13} className="spin" />
                      ) : (
                        String(index + 1).padStart(2, "0")
                      )}
                    </span>
                    <ArrowRight size={13} />
                  </span>
                  <b>{title}</b>
                  <small>
                    {stage?.seconds != null
                      ? seconds(stage.seconds)
                      : stage?.status === "running"
                        ? "In progress"
                        : stage?.status === "failed"
                          ? "Failed"
                          : stage?.status === "cancelled"
                            ? "Stopped"
                            : "Awaiting run"}
                  </small>
                </button>
              );
            })}
          </div>
          <div className="event-console">
            <div className="event-console-heading">
              <span>
                <span className={active ? "live-dot" : "idle-dot"} />
                {selectedStage?.title || "Live activity"}
              </span>
              {run && selectedStage && !viewingReader && (
                <a
                  href={`/api/runs/${run.id}/logs/${selectedStage.id}`}
                  target="_blank"
                  rel="noreferrer"
                >
                  Full log <ArrowUpRight size={12} />
                </a>
              )}
            </div>
            {selectedStage && (
              <p className="stage-description">{selectedStage.description}</p>
            )}
            {selectedEvents.length ? (
              <ol>
                {selectedEvents.map((event) => (
                  <li key={event.id}>
                    <time>
                      {new Date(event.time).toLocaleTimeString([], {
                        hour12: false,
                      })}
                    </time>
                    <span>
                      {event.message?.replaceAll(
                        /\/Users\/[^ ]+/g,
                        "[local artifact]",
                      )}
                    </span>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="console-empty">
                {viewingReader
                  ? "Reader view shows public stage results. Detailed capture logs stay in capture view."
                  : latest?.message ||
                    "Real events will appear here when you start an experiment. Select any stage to inspect it."}
              </p>
            )}
            {active && (
              <p className="console-live">
                {run?.status === "queued" ? (
                  "Waiting for the local prover…"
                ) : (
                  <>
                    <span className="pulse-square" /> Engine running. Elapsed{" "}
                    <Elapsed started={run?.started_at || null} />. No estimated
                    proof percentage.
                  </>
                )}
              </p>
            )}
          </div>
        </section>
        {run?.error && (
          <div className="error-banner" role="alert">
            <X size={16} />
            <span>{run.error}</span>
          </div>
        )}
        <section className="research-strip">
          <span className="micro">KNOW WHAT THE DEMO MEANS</span>
          <p>
            Simulated capture. Real proof commands. An evolving research
            prototype.
          </p>
          <button className="text-button" onClick={() => setShowAbout(true)}>
            Read the boundaries <ArrowUpRight size={15} />
          </button>
        </section>
      </main>
      <footer>
        <span>PROVENANCE LAB</span>
        <span>HYPERVERITAS × ZKLP × C2PA</span>
        <span>
          BUILT TO BE EXPLORED <span className="orange-square" />
        </span>
      </footer>
      <dialog
        ref={dialog}
        onClose={() => setShowAbout(false)}
        className="about-dialog"
      >
        <button
          className="dialog-close"
          aria-label="Close about the lab"
          onClick={() => setShowAbout(false)}
        >
          <X />
        </button>
        <span className="eyebrow">C2PA THESIS / RESEARCH PROTOTYPE</span>
        <h2>
          Follow the evidence.
          <br />
          Understand its limits.
        </h2>
        <p>
          The thesis explores how a reader can verify an edited photograph and a
          regional location claim, while keeping original pixels and exact
          coordinates private.
        </p>
        <h3>What runs for real</h3>
        <p>
          A simulated device signs the image fingerprint and salted location
          envelope together. HyperVerITAS generates a PST crop proof; ZKLP
          generates a Groth16 location proof. C2PA embeds and signs the evidence
          in the published PNG.
        </p>
        <h3>What is supported today</h3>
        <p>
          1024 × 512 RGB capture, followed by a fixed 512 × 512 left crop. The
          location is chosen by you, not read from GPS or authenticated
          hardware. Input images are resized; the upload’s original aspect ratio
          may change.
        </p>
        <h3>Location restrictions</h3>
        <p>
          The current ZKLP code rejects H3 pentagons, cells spanning multiple
          icosahedron faces, and centers with inconsistent 32-bit mappings.
          Separately, its trigonometric hint outputs are not fully constrained
          to secret coordinates. These passing checks demonstrate the intended
          prover flow, not security against a malicious prover.
        </p>
        <h3>Privacy is still research work</h3>
        <p>
          The public fingerprint permits candidate-image matching. The image
          proof also exposes unmasked polynomial evaluations. The original file
          and raw coordinates are not attached to the published PNG, but
          complete zero knowledge has not been established. Reader view is an
          educational perspective inside this local lab.
        </p>
        <h3>Local by design</h3>
        <p>
          Images and proof work are handled by the local Python engine. Map
          tiles are requested from OpenStreetMap. Demo signing keys and proof
          parameters are for research, not production identities.
        </p>
        <button className="dark-button" onClick={() => setShowAbout(false)}>
          Back to the experiment <ArrowUpRight size={17} />
        </button>
      </dialog>
    </div>
  );
}
