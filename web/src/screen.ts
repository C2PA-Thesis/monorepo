// The presenter page, opened on the laptop: the QR codes that pair a phone
// (the app's, and the capture page's for a browser), every capture as the
// laptop proves and verifies it, and a drop zone that verifies any file as a
// reader would.

import QRCode from "qrcode";
import "./style.css";
import "./screen.css";
import * as api from "./api";
import { $, area, h, rect, short, time } from "./dom";
import { CellMap } from "./map";
import { Timeline } from "./timeline";
import * as verdict from "./verdict";

const app = $("app");
const list = h("ol", { class: "capture-list" });
const detail = h("section", { class: "detail" });
const verifyView = h("section", { class: "verify-view", hidden: true });
let captures: api.Status[] = [];
let selected: string | undefined;
let shown: { id: string; timeline: Timeline; map: CellMap; parts: Record<string, HTMLElement> } | undefined;
let followNewest = true;

async function main() {
  let session: api.Session;
  try {
    session = await api.session();
  } catch (error) {
    app.replaceChildren(
      h(
        "div",
        { class: "card blocked" },
        h("h2", {}, "Open this page on the laptop"),
        h("p", { class: "muted" }, `It shows the pairing secrets and the private originals, so it only answers on the laptop running provenance serve. ${message(error)}`),
      ),
    );
    return;
  }
  app.replaceChildren(
    h("aside", { class: "side" }, await connect(session), h("div", { class: "card list-card" }, h("h2", {}, "Captures"), list)),
    h("main", { class: "main" }, detail, verifyView),
  );
  verifyView.append(...dropZone());
  tabs();
  await refresh();
  window.setInterval(() => void refresh(), 1000);
}

async function connect(session: api.Session): Promise<HTMLElement> {
  const app = new AppPairing();
  await app.refresh();
  window.setInterval(() => void app.refresh(), 2000);

  const browser = h("div", { class: "browser-pair" }, h("h3", { class: "section-title" }, "Or with the browser"));
  if (session.pair_url) {
    const qr = h("div", { class: "qr qr-browser" });
    qr.innerHTML = await QRCode.toString(session.pair_url, { type: "svg", margin: 1, errorCorrectionLevel: "M" });
    browser.append(
      h(
        "div",
        { class: "browser-row" },
        qr,
        h(
          "div",
          { class: "browser-text" },
          h("p", { class: "small muted" }, "Scan with the phone's camera. It opens the capture page through the tunnel and pairs on its own."),
          h("p", { class: "small url mono" }, session.public_url ?? ""),
        ),
      ),
    );
  } else {
    browser.append(
      h(
        "p",
        { class: "small warning" },
        "No tunnel address was given, so there is no QR code for the browser. Start with scripts/serve.sh, which opens an ngrok tunnel.",
      ),
    );
  }
  browser.append(
    h("div", { class: "code" }, h("span", { class: "small muted" }, "Pairing code"), h("strong", { class: "mono" }, session.pairing_code)),
  );
  return h(
    "div",
    { class: "card connect" },
    h("h2", {}, "Connect a phone"),
    app.element,
    browser,
    h(
      "p",
      { class: "small muted" },
      session.publishes
        ? "Each capture is proved, signed and verified here as soon as it arrives."
        : "Captures are only stored; publish them with scripts/publish.sh.",
    ),
  );
}

/// The app's QR code. Its token is single use and expires, so the code is
/// drawn again whenever the server hands out another one.
class AppPairing {
  readonly element = h("div", { class: "app-pair" });
  private readonly qr = h("div", { class: "qr qr-app" });
  private readonly urls = h("p", { class: "small url mono" });
  private readonly expiry = h("p", { class: "small muted expiry" });
  private readonly problem = h("p", { class: "small warning", hidden: true });
  private shown: string | undefined;
  private expiresAt = 0;

  constructor() {
    this.element.append(
      h("h3", { class: "section-title" }, "With the app"),
      this.qr,
      this.problem,
      h("p", { class: "small muted" }, "Scan in Provenance Capture. The phone joins over this network, checks this laptop's key, and proves the location itself."),
      this.urls,
      this.expiry,
    );
    window.setInterval(() => this.tick(), 1000);
  }

  async refresh() {
    let pairing: api.AppPairing;
    try {
      pairing = await api.appPairing();
    } catch (error) {
      this.shown = undefined;
      this.qr.hidden = true;
      this.expiry.hidden = true;
      this.show(`The app cannot pair: ${message(error)}`);
      return;
    }
    this.expiresAt = Date.now() + pairing.expiresIn * 1000;
    // Redrawn only when the token changed: used, expired or rotated.
    if (pairing.qr.token !== this.shown) {
      this.shown = pairing.qr.token;
      this.qr.innerHTML = await QRCode.toString(pairing.text.trim(), { type: "svg", margin: 1, errorCorrectionLevel: "L" });
      this.qr.hidden = false;
      this.expiry.hidden = false;
      this.urls.textContent = pairing.qr.urls.join("  ");
      this.show(
        pairing.qr.urls.length
          ? undefined
          : "This laptop has no network address yet. Join the phone's Wi-Fi, then restart scripts/serve.sh.",
      );
    }
    this.tick();
  }

  private show(problem: string | undefined) {
    this.problem.hidden = !problem;
    this.problem.textContent = problem ?? "";
  }

  private tick() {
    const left = Math.max(0, Math.round((this.expiresAt - Date.now()) / 1000));
    this.expiry.textContent = `Single use. A new code in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}.`;
  }
}

/// Who signed a capture, in the words of the presenter page.
/// The app declares a Secure Enclave key, and nothing here checks it.
function deviceKind(status: api.Status): string {
  const kind = status.device_kind ?? (status.capture.source === "simulated" ? "laptop" : "browser");
  return {
    "secure-enclave": "App · declared: Secure Enclave",
    software: "App · software key",
    browser: "Browser",
    laptop: "Laptop",
  }[kind];
}

function locationNote(status: api.Status): string {
  const location = status.capture.location;
  if (location?.simulated) return " · simulated location";
  if (location?.accessory) return " · external GPS";
  return "";
}

function tabs() {
  const select = (verify: boolean) => {
    $("tab-captures").setAttribute("aria-selected", String(!verify));
    $("tab-verify").setAttribute("aria-selected", String(verify));
    detail.hidden = verify;
    verifyView.hidden = !verify;
    if (!verify) shown?.map.resize();
  };
  $("tab-captures").onclick = () => select(false);
  $("tab-verify").onclick = () => select(true);
}

// Captures

async function refresh() {
  try {
    captures = await api.captures();
  } catch {
    return;
  }
  const newest = captures[0]?.id;
  if (followNewest && newest && newest !== selected) selected = newest;
  if (selected && !captures.some((each) => each.id === selected)) selected = newest;
  renderList();
  const status = captures.find((each) => each.id === selected);
  if (status) renderDetail(status);
  else if (!captures.length) renderEmpty();
}

function renderList() {
  list.replaceChildren(
    ...captures.map((status) =>
      h(
        "li",
        {},
        h(
          "button",
          {
            type: "button",
            class: "capture-item",
            "aria-current": status.id === selected ? "true" : undefined,
            onclick: () => {
              selected = status.id;
              followNewest = status.id === captures[0]?.id;
              void refresh();
            },
          },
          h("span", { class: "capture-time" }, time(status.captured_at)),
          phaseBadge(status.phase),
          h(
            "span",
            { class: "capture-meta" },
            h("span", { class: "kind" }, deviceKind(status)),
            h("span", { class: "mono" }, `${status.capture.cell} · ${rect(status.capture.crop)}`),
          ),
        ),
      ),
    ),
  );
  if (!captures.length) list.append(h("li", { class: "small muted" }, "None yet."));
}

function renderEmpty() {
  shown = undefined;
  detail.replaceChildren(
    h(
      "div",
      { class: "card empty" },
      h("h2", {}, "Waiting for the first capture"),
      h(
        "ol",
        { class: "how" },
        h("li", {}, "Scan the large QR code in the app, or the small one with the phone's camera to use the capture page. Either pairs the phone."),
        h("li", {}, "Allow location, take a photo, choose the rectangle to publish and the size of the cell."),
        h("li", {}, "Sign and send. The phone fingerprints the frame, seals the coordinate and signs both. The app also proves the cell on the phone, so the coordinate never leaves it."),
        h("li", {}, "This laptop proves the crop (and the location, for the capture page), signs the C2PA manifest and verifies the result, here and on the phone."),
      ),
    ),
  );
}

function renderDetail(status: api.Status) {
  if (shown?.id !== status.id) buildDetail(status);
  const { parts, timeline } = shown!;
  parts.badge!.replaceWith((parts.badge = phaseBadge(status.phase)));
  timeline.update(status.events, status.phase === "failed" ? (status.error ?? "failed") : undefined);
  const idle = !status.events.length && !api.isRunning(status.phase);
  timeline.element.hidden = idle;
  parts.idle!.hidden = !idle;
  parts.idle!.textContent =
    status.phase === "stored"
      ? "Stored, not published yet. Publish it to watch the proofs being made and checked."
      : "Published before this server started. Verify again to watch the reader checks, or publish again to watch the proofs.";

  const key = `${status.phase}`;
  if (parts.published!.dataset.key !== key) {
    parts.published!.dataset.key = key;
    parts.published!.replaceChildren(
      status.published && !api.isRunning(status.phase)
        ? h("img", { src: `${api.signedUrl(status.id)}?${Date.now()}`, alt: "The published file" })
        : h("div", { class: "placeholder" }, api.isRunning(status.phase) ? h("span", { class: "spinner" }) : false, status.published ? "Publishing…" : publishedHint(status.phase)),
    );
    parts.outcome!.replaceChildren(...outcome(status));
    parts.actions!.replaceChildren(...actions(status));
  }
}

function publishedHint(phase: api.Phase): string {
  if (api.isRunning(phase)) return "Appears once it is published";
  if (phase === "stored") return "Not published yet";
  return "No published file";
}

function buildDetail(status: api.Status) {
  const timeline = new Timeline(["publish", "verify"]);
  const map = new CellMap();
  const parts: Record<string, HTMLElement> = {
    badge: phaseBadge(status.phase),
    published: h("div", { class: "frame checker" }),
    outcome: h("div", { class: "outcome" }),
    actions: h("div", { class: "detail-actions" }),
    idle: h("p", { class: "muted small", hidden: true }),
  };
  const { crop } = status.capture;
  const overlay = h("div", {
    class: "overlay",
    style: `left:${(crop.x / 1024) * 100}%;top:${(crop.y / 512) * 100}%;width:${(crop.width / 1024) * 100}%;height:${(crop.height / 512) * 100}%`,
  });
  detail.replaceChildren(
    h(
      "div",
      { class: "detail-head" },
      h(
        "div",
        {},
        h("h2", {}, "Capture ", h("span", { class: "mono" }, status.id)),
        h(
          "p",
          { class: "muted small" },
          h("span", { class: "kind" }, deviceKind(status)),
          ` device ${short(status.device)} · ${time(status.captured_at)}`,
          status.capture.accuracy_meters ? ` · GPS ±${Math.round(status.capture.accuracy_meters)} m` : "",
          locationNote(status),
        ),
      ),
      h("div", { class: "head-right" }, parts.badge, parts.actions),
    ),
    h(
      "div",
      { class: "images" },
      h(
        "figure",
        { class: "card" },
        h("figcaption", {}, h("span", { class: "tag private" }, "Private"), " The original, on this laptop only"),
        h("div", { class: "frame original" }, h("img", { src: api.originalUrl(status.id), alt: "The private original" }), overlay),
        h("p", { class: "small muted" }, `Only its fingerprint is published. The box is the ${rect(crop)} that gets published.`),
      ),
      h(
        "figure",
        { class: "card" },
        h("figcaption", {}, h("span", { class: "tag public" }, "Published"), " The signed PNG anyone can check"),
        parts.published,
        h("p", { class: "small muted" }, "Carries the receipt, both proofs and the cell in its C2PA manifest."),
      ),
    ),
    h(
      "div",
      { class: "columns" },
      h("div", { class: "card pad" }, timeline.element, parts.idle),
      h(
        "div",
        { class: "card pad region" },
        h("h3", { class: "section-title" }, "Where, as published"),
        map.element,
        h("p", { class: "small muted" }, `Cell ${status.capture.cell} at resolution ${status.capture.resolution}. The exact coordinate stays sealed in the envelope.`),
        parts.outcome,
      ),
    ),
  );
  shown = { id: status.id, timeline, map, parts };
  void map.show(status.capture.cell).then((outline) => {
    if (outline) map.element.title = `${outline.cell}, ${area(outline.area_km2)}`;
  });
}

function outcome(status: api.Status): HTMLElement[] {
  if (status.phase === "accepted" && status.claim) {
    return [verdict.accepted(status.claim), verdict.facts(status.claim), h("p", { class: "caveat" }, verdict.CAVEAT)];
  }
  if (status.phase === "rejected") return [verdict.rejected("Rejected", status.error ?? "")];
  if (status.phase === "failed") return [verdict.rejected("Publishing failed", status.error ?? "")];
  if (status.phase === "published") {
    return [h("p", { class: "small muted" }, "Published by an earlier run. Verify it again to see the checks.")];
  }
  return [];
}

function actions(status: api.Status): HTMLElement[] {
  const buttons: HTMLElement[] = [];
  if (status.published && !api.isRunning(status.phase)) {
    buttons.push(
      h("button", { type: "button", class: "button-quiet", onclick: () => void reverify(status) }, "Verify again"),
      h("a", { class: "button", href: api.signedUrl(status.id), download: `${status.id}.png` }, "Download"),
    );
  }
  if (!api.isRunning(status.phase) && status.phase !== "accepted") {
    buttons.push(
      h(
        "button",
        {
          type: "button",
          class: status.published ? "button-quiet" : undefined,
          onclick: async (event: Event) => {
            (event.currentTarget as HTMLButtonElement).disabled = true;
            await api.republish(status.id).catch((error) => alert(message(error)));
            await refresh();
          },
        },
        status.published ? "Publish again" : "Publish",
      ),
    );
  }
  return buttons;
}

async function reverify(status: api.Status) {
  const png = await (await fetch(api.signedUrl(status.id))).blob();
  $("tab-verify").click();
  await verifyFile(new File([png], `${status.id}.png`, { type: "image/png" }));
}

function phaseBadge(phase: api.Phase): HTMLElement {
  const label: Record<api.Phase, string> = {
    stored: "Stored",
    published: "Published",
    queued: "Queued",
    publishing: "Proving",
    verifying: "Verifying",
    accepted: "Verified",
    rejected: "Rejected",
    failed: "Failed",
  };
  const tone = phase === "accepted" ? "ok" : phase === "rejected" || phase === "failed" ? "bad" : api.isRunning(phase) ? "busy" : "";
  return h("span", { class: `badge ${tone}` }, api.isRunning(phase) ? h("span", { class: "spinner small-spinner" }) : false, label[phase]);
}

// Verify a file

const result = h("div", { class: "verify-result" });

function dropZone(): HTMLElement[] {
  const input = h("input", { type: "file", accept: "image/png", hidden: true }) as HTMLInputElement;
  input.onchange = () => input.files?.[0] && void verifyFile(input.files[0]);
  const zone = h(
    "label",
    { class: "drop card" },
    input,
    h("strong", {}, "Drop a PNG here, or click to choose one"),
    h(
      "span",
      { class: "small muted" },
      "Runs the four reader checks, as provenance verify does. Try a published file, a forged copy from out/attacks, or any other PNG.",
    ),
  );
  zone.ondragover = (event) => {
    event.preventDefault();
    zone.classList.add("over");
  };
  zone.ondragleave = () => zone.classList.remove("over");
  zone.ondrop = (event) => {
    event.preventDefault();
    zone.classList.remove("over");
    const file = event.dataTransfer?.files[0];
    if (file) void verifyFile(file);
  };
  return [zone, result];
}

async function verifyFile(file: File) {
  const preview = URL.createObjectURL(file);
  const timeline = new Timeline(["verify"]);
  timeline.update([{ event: "checking", check: "manifest", intent: "reading the C2PA manifest and validating its claim signature" }]);
  const outcomeSlot = h("div", { class: "outcome" }, h("p", { class: "muted" }, "Verifying…"));
  const map = new CellMap();
  map.element.hidden = true;
  result.replaceChildren(
    h(
      "div",
      { class: "columns" },
      h(
        "figure",
        { class: "card" },
        h("figcaption", {}, h("span", { class: "mono" }, file.name), ` · ${Math.round(file.size / 1000)} KB`),
        h("div", { class: "frame checker" }, h("img", { src: preview, alt: file.name })),
      ),
      h("div", { class: "card pad" }, timeline.element),
    ),
    h("div", { class: "card pad region" }, outcomeSlot, map.element),
  );
  try {
    const report = await api.verify(file);
    timeline.update(report.events);
    const { claim, rejected } = report.verdict;
    if (claim) {
      outcomeSlot.replaceChildren(verdict.accepted(claim), verdict.facts(claim), h("p", { class: "caveat" }, verdict.CAVEAT));
      map.element.hidden = false;
      void map.show(claim.cell);
    } else if (rejected) {
      const title = { manifest: "C2PA manifest", device: "device receipt", image: "crop proof", location: "location proof" }[rejected.check];
      outcomeSlot.replaceChildren(verdict.rejected("Rejected", `The ${title} check refused this file: ${rejected.detail}`));
    }
  } catch (error) {
    timeline.update([], message(error));
    outcomeSlot.replaceChildren(verdict.rejected("Could not verify", message(error)));
  }
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

void main();
