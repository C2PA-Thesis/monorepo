import { useEffect, useState } from "react";
import { ArrowDown, ArrowRight, Grid3X3, RefreshCw } from "lucide-react";
import { api } from "./useLab";
import { asset } from "./types";
import type { Channel, Experiment, MatrixData } from "./types";

export function MatrixView({
  run,
  reader,
}: {
  run: Experiment | null;
  reader: boolean;
}) {
  const [channel, setChannel] = useState<Channel>("R");
  const [patch, setPatch] = useState({ x: 240, y: 128 });
  const [data, setData] = useState<MatrixData | null>(null);
  const [selected, setSelected] = useState(0);
  const [row, setRow] = useState(0);
  const [output, setOutput] = useState(0);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!run || reader) {
      setData(null);
      return;
    }
    const controller = new AbortController();
    api<MatrixData>(
      `/runs/${run.id}/matrix?channel=${channel}&x=${patch.x}&y=${patch.y}`,
      { signal: controller.signal },
    )
      .then((value) => {
        setData(value);
        setError("");
      })
      .catch((err) => {
        if (err.name !== "AbortError") setError(err.message);
      });
    return () => controller.abort();
  }, [
    run?.id,
    run?.status,
    run?.stages[0]?.status,
    channel,
    patch.x,
    patch.y,
    reader,
  ]);
  if (!run)
    return (
      <div className="empty-section">
        <Grid3X3 />
        <h2>Every pixel has a place.</h2>
        <p>
          Upload an image or use the sample to explore its real RGB values and
          fingerprint.
        </p>
      </div>
    );
  if (reader)
    return (
      <div className="empty-section">
        <Grid3X3 />
        <h2>The original stays with the capture.</h2>
        <p>
          This explorer reads original pixel data. Switch to Capture view to
          inspect it. The public fingerprint is available in Evidence.
        </p>
      </div>
    );
  const weights = Array.from(
    { length: 8 },
    (_, i) => ((row + 1) * (i + 3) + 7) % 97,
  );
  const products = weights.map((weight, i) => weight * (data?.values[i] || 0));
  const sum = products.reduce((a, b) => a + b, 0);
  return (
    <div className="matrix-view">
      <div className="section-intro">
        <div>
          <span className="eyebrow">UNDER THE PIXELS</span>
          <h2>A different kind of image.</h2>
        </div>
        <p>
          Explore your actual pixels, then follow a small example of the
          arithmetic behind a fingerprint.
        </p>
      </div>
      {error && (
        <p role="alert" className="inline-error">
          {error}
        </p>
      )}
      <div className="matrix-workspace">
        <section className="pixel-panel">
          <div className="panel-heading">
            <span>01 / Choose a patch</span>
            <span className="micro">ORIGINAL · PRIVATE</span>
          </div>
          <div className="patch-image">
            <img
              src={asset(run.id, "prepared.png")}
              alt="Normalized original with selected pixel patch"
            />
            <span
              style={{
                left: `${(patch.x / 1024) * 100}%`,
                top: `${(patch.y / 512) * 100}%`,
              }}
            />
          </div>
          <div className="patch-sliders">
            <label>
              X{" "}
              <input
                aria-label="Pixel patch X"
                type="range"
                min="0"
                max="1016"
                step="8"
                value={patch.x}
                onChange={(e) => setPatch({ ...patch, x: +e.target.value })}
              />
              <b>{patch.x}</b>
            </label>
            <label>
              Y{" "}
              <input
                aria-label="Pixel patch Y"
                type="range"
                min="0"
                max="504"
                step="8"
                value={patch.y}
                onChange={(e) => setPatch({ ...patch, y: +e.target.value })}
              />
              <b>{patch.y}</b>
            </label>
          </div>
          <div className="channel-tabs" aria-label="Color channel">
            {(["R", "G", "B"] as Channel[]).map((c) => (
              <button
                key={c}
                aria-pressed={channel === c}
                className={channel === c ? "selected" : ""}
                onClick={() => setChannel(c)}
              >
                {c === "R" ? "Red" : c === "G" ? "Green" : "Blue"}
              </button>
            ))}
          </div>
        </section>
        <section className="pixel-panel">
          <div className="panel-heading">
            <span>02 / Read the pixels</span>
            <span className="micro">8 × 8 SAMPLE</span>
          </div>
          <div className="pixel-grid">
            {data?.values.map((value, index) => (
              <button
                key={index}
                className={selected === index ? "selected" : ""}
                style={{
                  background: `rgb(${channel === "R" ? value : 0},${channel === "G" ? value : 0},${channel === "B" ? value : 0})`,
                  color: channel === "G" && value > 150 ? "#111" : "#fff",
                }}
                aria-label={`Pixel ${index}, ${channel} value ${value}`}
                onClick={() => setSelected(index)}
              >
                {value}
              </button>
            ))}
          </div>
          <div className="pixel-readout">
            <span>
              PIXEL [{patch.x + (selected % 8)},{" "}
              {patch.y + Math.floor(selected / 8)}]
            </span>
            <b>
              {channel} = {data?.values[selected] ?? "—"}
            </b>
            <span>
              VECTOR INDEX {data?.indices[selected]?.toLocaleString() ?? "—"}
            </span>
          </div>
        </section>
        <section className="pixel-panel vector-panel">
          <div className="panel-heading">
            <span>03 / Flatten into a vector</span>
          </div>
          <div className="vector-art">
            <Grid3X3 size={64} strokeWidth={0.6} />
            <ArrowDown size={27} strokeWidth={1} />
            <div>
              {data?.values.slice(0, 8).map((value, index) => (
                <span key={index}>{value}</span>
              ))}
            </div>
          </div>
          <p>
            The full channel contains <strong>524,288 values</strong> in
            row-major order. This patch samples 8 consecutive pixels from each
            of 8 rows.
          </p>
          <span className="micro">1024 × 512 → 524,288</span>
        </section>
      </div>
      <section className="arithmetic-panel">
        <div className="panel-heading">
          <span>04 / Multiply. Add. Reduce.</span>
          <span className="teaching-tag">
            TEACHING MODEL · NOT THE REAL PROOF
          </span>
        </div>
        <div className="equation">
          <div>
            <span className="micro">ONE ROW OF A</span>
            <div className="number-strip">
              {weights.map((value, i) => (
                <span key={i}>{value}</span>
              ))}
            </div>
          </div>
          <b>×</b>
          <div>
            <span className="micro">8 PIXELS FROM YOUR PATCH</span>
            <div className="number-strip">
              {data?.values.slice(0, 8).map((value, i) => (
                <span key={i}>{value}</span>
              ))}
            </div>
          </div>
          <ArrowRight />
          <div className="equation-result">
            <b>{sum % 97}</b>
            <span>mod 97</span>
          </div>
        </div>
        <div className="arithmetic-bottom">
          <p>
            Sum the 8 products: <b>{sum.toLocaleString()}</b>. Divide by 97 and
            keep the remainder. The real fingerprint does this over the
            BLS12-381 scalar field, using 128 deterministic rows and every pixel
            in a channel.
          </p>
          <button
            className="outline-button"
            onClick={() => setRow((value) => (value + 1) % 8)}
          >
            <RefreshCw size={14} /> Next example row
          </button>
        </div>
      </section>
      <section className="fingerprint-panel">
        <div className="panel-heading">
          <span>05 / The actual fingerprint</span>
          <span className="micro">128 FIELD ELEMENTS / CHANNEL</span>
        </div>
        {data?.fingerprint ? (
          <>
            <div className="fingerprint-grid">
              {data.fingerprint.map((value, index) => (
                <button
                  key={index}
                  aria-label={`Fingerprint element ${index}`}
                  className={output === index ? "selected" : ""}
                  style={{
                    opacity: 0.3 + (parseInt(value.slice(-2), 16) / 255) * 0.7,
                  }}
                  onClick={() => setOutput(index)}
                />
              ))}
            </div>
            <div className="fingerprint-value">
              <span>
                {channel}[{output}]
              </span>
              <code>{data.fingerprint[output]}</code>
            </div>
            <p>
              These are real values from this capture. Tile brightness is a
              visual aid, not a measure of cryptographic strength.
            </p>
          </>
        ) : (
          <div className="waiting-note">
            Generate the capture receipt to reveal its real fingerprint here.
            The 128 outputs are public; the full original is not attached to the
            published PNG.
          </div>
        )}
      </section>
    </div>
  );
}
