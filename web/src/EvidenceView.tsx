import { useState } from "react";
import {
  ArrowDown,
  ArrowUpRight,
  Check,
  Copy,
  Download,
  FileJson,
  Fingerprint,
  LockKeyhole,
  MapPin,
  ShieldCheck,
} from "lucide-react";
import { asset, bytes, busy, seconds } from "./types";
import type { Experiment, Reader } from "./types";

function JsonNode({
  name,
  value,
  root = false,
}: {
  name: string;
  value: unknown;
  root?: boolean;
}) {
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value);
    return (
      <details className="json-node" open={root}>
        <summary>
          <b>{name}</b>
          <span>
            {Array.isArray(value)
              ? `[ ${entries.length} items ]`
              : `{ ${entries.length} fields }`}
          </span>
        </summary>
        <div>
          {entries.map(([key, child]) => (
            <JsonNode key={key} name={key} value={child} />
          ))}
        </div>
      </details>
    );
  }
  const text = JSON.stringify(value);
  if (text && text.length > 160)
    return (
      <details className="json-node">
        <summary>
          <b>{name}</b>
          <span>{text.length.toLocaleString()} characters</span>
        </summary>
        <code className="json-value">{text}</code>
      </details>
    );
  return (
    <div className="json-leaf">
      <span>{name}</span>
      <code>{text}</code>
    </div>
  );
}

export function EvidenceView({
  run,
  reader,
  onVerify,
  working,
}: {
  run: Experiment | null;
  reader: Reader | null;
  onVerify: () => void;
  working: boolean;
}) {
  const [tab, setTab] = useState("receipt");
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  if (!run || !reader?.assertion)
    return (
      <div className="empty-section">
        <FileJson />
        <h2>Evidence, not a black box.</h2>
        <p>
          Generate the proofs to inspect the actual signed receipt, proof
          artifacts, and C2PA manifest.
        </p>
      </div>
    );
  const assertion = reader.assertion;
  const content: Record<string, unknown> = {
    receipt: assertion.receipt,
    region: assertion.region,
    "image proof": assertion.image_proof,
    "location proof": assertion.location_proof,
    manifest: reader.manifest,
  };
  const verified = reader.stages.some(
    (stage) => stage.id === "verifier" && stage.status === "complete",
  );
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(
        JSON.stringify(content[tab], null, 2),
      );
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopyError("Clipboard unavailable. Download the manifest instead.");
    }
  };
  return (
    <div className="evidence-view">
      <div className="section-intro">
        <div>
          <span className="eyebrow">THE PUBLIC RECORD</span>
          <h2>One capture. Connected evidence.</h2>
        </div>
        <p>
          The device signs two public values together. Each proof is checked
          against one of those signed values.
        </p>
      </div>
      <div className="binding-diagram">
        <article>
          <Fingerprint />
          <span className="micro">HYPERVERITAS</span>
          <h3>Image fingerprint</h3>
          <p>128 field elements × 3 color channels</p>
          <code>
            {assertion.receipt.fingerprint.channels.R[0].slice(0, 23)}…
          </code>
        </article>
        <div className="binding-connector">
          <span>signed together</span>
        </div>
        <article className="receipt-card">
          <LockKeyhole />
          <span className="micro">TRUSTED DEMO DEVICE</span>
          <h3>Capture receipt</h3>
          <p>P-256 signature · dimensions · time</p>
          <code>
            {new Date(assertion.receipt.captured_at).toLocaleString()}
          </code>
        </article>
        <div className="binding-connector">
          <span>signed together</span>
        </div>
        <article>
          <MapPin />
          <span className="micro">ZKLP</span>
          <h3>Location envelope</h3>
          <p>Salted MiMC commitment on BN254</p>
          <code>{assertion.receipt.envelope.value.slice(0, 23)}…</code>
        </article>
      </div>
      <div className="publish-connector">
        <ArrowDown size={22} />
        <span>PACKAGED INSIDE THE PUBLISHED PNG</span>
      </div>
      <div className="published-evidence">
        <img
          src={asset(run.id, "signed.png")}
          alt="Published crop carrying C2PA evidence"
        />
        <div>
          <span className="eyebrow">C2PA ASSERTION</span>
          <h3>edu.utdt.td8.zkloc</h3>
          <p>
            The published image carries the receipt, region, and both proofs. A
            separate sample editor key signs the C2PA asset.
          </p>
          <a className="dark-button" href={asset(run.id, "signed.png", true)}>
            <Download size={15} /> Download signed PNG
          </a>
        </div>
        <dl>
          <div>
            <dt>Image proof</dt>
            <dd>{bytes(reader.sizes.image_proof)}</dd>
          </div>
          <div>
            <dt>Location proof</dt>
            <dd>{bytes(reader.sizes.location_proof)}</dd>
          </div>
          <div>
            <dt>Published PNG</dt>
            <dd>{bytes(reader.sizes.signed_png)}</dd>
          </div>
        </dl>
      </div>
      <section className="reader-verification">
        <div>
          <ShieldCheck />
          <div>
            <h3>
              {verified
                ? "Reader checks passed"
                : "Reader verification is pending"}
            </h3>
            <p>
              Verify the published PNG using the requested region, trusted
              device public key, and proof parameters.
            </p>
          </div>
        </div>
        <button
          className="outline-button"
          disabled={!verified || busy(run.status) || working}
          onClick={onVerify}
        >
          Verify again <ArrowUpRight size={15} />
        </button>
        {reader.reader_check && (
          <span className="micro">
            RECHECKED {new Date(reader.reader_check.time).toLocaleTimeString()}{" "}
            · {seconds(reader.reader_check.seconds)}
          </span>
        )}
      </section>
      <div className="verification-cells">
        {[
          "C2PA signature & integrity",
          "01 / Device receipt",
          "02 / Published pixels",
          "03 / Location proof",
        ].map((title) => (
          <div key={title}>
            <span className={verified ? "check-icon" : ""}>
              {verified ? <Check size={15} /> : "·"}
            </span>
            {title}
          </div>
        ))}
      </div>
      <section className="json-inspector">
        <div className="json-toolbar">
          <div>
            {Object.keys(content).map((name) => (
              <button
                key={name}
                onClick={() => setTab(name)}
                className={tab === name ? "selected" : ""}
              >
                {name}
              </button>
            ))}
          </div>
          <button onClick={() => void copy()} aria-label="Copy selected JSON">
            {copied ? <Check size={15} /> : <Copy size={15} />}
          </button>
          <a
            href={asset(run.id, "manifest.json", true)}
            aria-label="Download full manifest JSON"
          >
            <Download size={15} />
          </a>
        </div>
        <div className="json-scroll">
          <JsonNode key={tab} name={tab} value={content[tab]} root />
        </div>
        {copyError && <p className="inline-error">{copyError}</p>}
      </section>
      <p className="research-footnote">
        Research boundary: the public fingerprint is deterministic, and the
        pinned image proof exposes unmasked polynomial evaluations. Complete
        image privacy has not been established. The location circuit also has an
        unresolved coordinate-constraint gap.
      </p>
    </div>
  );
}

const attacks = [
  {
    id: "pixel",
    number: "01",
    title: "Change one pixel",
    description:
      "Alter one red-channel value in the published crop. Reuse the original proof.",
    check: 2,
  },
  {
    id: "signature",
    number: "02",
    title: "Break the signature",
    description: "Flip one byte in the capture receipt’s device signature.",
    check: 1,
  },
  {
    id: "region",
    number: "03",
    title: "Ask for another region",
    description:
      "Request a neighboring H3 cell while keeping the original location evidence.",
    check: 3,
  },
  {
    id: "receipt",
    number: "04",
    title: "Swap the receipt",
    description:
      "Create another capture with a fresh salt. Substitute its signed receipt.",
    check: 3,
  },
];

export function TamperView({
  run,
  reader,
  onAttack,
  working,
}: {
  run: Experiment | null;
  reader: Reader | null;
  onAttack: (name: string) => void;
  working: boolean;
}) {
  const enabled = run?.status === "complete" && !!reader?.published && !working;
  return (
    <div className="tamper-view">
      <div className="section-intro">
        <div>
          <span className="eyebrow">THE ADVERSARY’S WORKBENCH</span>
          <h2>Change something. See what breaks.</h2>
        </div>
        <p>
          Every test uses the real proof bundle. The modified asset is signed
          again with C2PA, so the custom checks must catch the change.
        </p>
      </div>
      {!reader?.published && (
        <p className="notice">
          Complete an experiment first to unlock the real tampering tests.
        </p>
      )}
      <div className="attack-grid">
        {attacks.map((attack) => {
          const result = reader?.attacks[attack.id];
          const active = busy(run?.status) && run?.activity === attack.id;
          return (
            <article
              className={result ? "attack-card rejected" : "attack-card"}
              key={attack.id}
            >
              <span className="attack-number">
                {attack.number}
                <ArrowUpRight size={23} />
              </span>
              <h3>{attack.title}</h3>
              <p>{attack.description}</p>
              <div className="attack-route">
                <span>C2PA</span>
                <i />
                <span className={result ? "caught" : ""}>
                  CHECK {attack.check}
                </span>
              </div>
              <button
                className="outline-button"
                disabled={!enabled}
                onClick={() => onAttack(attack.id)}
              >
                {active
                  ? "Testing change…"
                  : result
                    ? "Run again"
                    : "Run this test"}
                <ArrowUpRight size={15} />
              </button>
              {result && (
                <div className="attack-result">
                  <b>Change rejected</b>
                  <span>
                    C2PA {result.c2pa} · Check {result.observed}
                  </span>
                  <small>{new Date(result.time).toLocaleTimeString()}</small>
                </div>
              )}
            </article>
          );
        })}
      </div>
      {reader?.negative_results && (
        <section className="negative-table">
          <div className="panel-heading">
            <span>Automatic integration checks</span>
            <span className="micro">6 / 6 EXPECTED REJECTIONS</span>
          </div>
          {reader.negative_results.cases.map((item) => (
            <div key={item.name}>
              <Check size={15} />
              <span>{item.name.replaceAll("-", " ")}</span>
              <code>{item.observed}</code>
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
