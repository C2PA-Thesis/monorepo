// The presenter page, opened on the laptop: the QR code that pairs a phone
// (the app's, or the capture page's behind a toggle), every capture as the
// laptop proves and verifies it, and a drop zone that verifies any file as a
// reader would.

import QRCode from "qrcode";
import "./style.css";
import "./screen.css";
import * as api from "./api";
import { turnSize } from "./crop";
import { $, h, icon, short, svg, time, when } from "./dom";
import { CellMap } from "./map";
import { Timeline } from "./timeline";
import * as verdict from "./verdict";

const app = $("app");
const list = h("ol", { class: "capture-list" });
const detail = h("section", { class: "detail" });
const verifyView = h("section", { class: "verify-view", hidden: true });
let captures: api.Status[] = [];
let selected: string | undefined;
let shown: { id: string; timeline: Timeline; map: CellMap; parts: Record<string, HTMLElement>; km2?: number } | undefined;
let followNewest = true;
let verifying = false;

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
        h("p", { class: "muted" }, message(error)),
      ),
    );
    return;
  }
  app.replaceChildren(
    h("aside", { class: "side" }, await connect(session), list),
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

  // The capture page needs HTTPS for the camera and GPS, so it is offered only through a tunnel.
  const browser = h("div", { class: "browser-pair", hidden: true });
  if (session.pair_url) {
    const qr = h("div", { class: "qr qr-browser" });
    qr.innerHTML = await QRCode.toString(session.pair_url, { type: "svg", margin: 1, errorCorrectionLevel: "M" });
    browser.append(qr, h("p", { class: "meta" }, "Scan with the phone's camera"));
  }
  const toggle = h(
    "button",
    {
      type: "button",
      class: "link-button",
      "aria-expanded": "false",
      onclick: () => {
        browser.hidden = !browser.hidden;
        toggle.setAttribute("aria-expanded", String(!browser.hidden));
        toggle.textContent = browser.hidden ? "Use a browser instead" : "Use the app instead";
        app.element.hidden = !browser.hidden;
      },
    },
    "Use a browser instead",
  );
  return h(
    "div",
    { class: "connect" },
    h("div", { class: "qr-panel" }, app.element, browser),
    h("p", { class: "code" }, h("span", { class: "sr-only" }, "Pairing code "), h("span", { class: "mono" }, session.pairing_code)),
    session.pair_url && toggle,
    !session.publishes && h("p", { class: "meta" }, "Captures are stored, not published."),
  );
}

/// The app's QR code. Its token is single use and expires, so the code is
/// drawn again whenever the server hands out another one.
class AppPairing {
  readonly element = h("div", { class: "app-pair" });
  private readonly qr = h("div", { class: "qr qr-app" });
  private readonly problem = h("p", { class: "meta warn-text", hidden: true });
  private shown: string | undefined;

  constructor() {
    this.element.append(this.qr, this.problem, h("p", { class: "meta" }, "Scan with the Capture app"));
  }

  async refresh() {
    let pairing: api.AppPairing;
    try {
      pairing = await api.appPairing();
    } catch (error) {
      this.shown = undefined;
      this.qr.hidden = true;
      this.show(`The app cannot pair: ${message(error)}`);
      return;
    }
    // Redrawn only when the token changed: used, expired or rotated.
    if (pairing.qr.token !== this.shown) {
      this.shown = pairing.qr.token;
      this.qr.innerHTML = await QRCode.toString(pairing.text.trim(), { type: "svg", margin: 1, errorCorrectionLevel: "L" });
      this.qr.hidden = false;
      this.qr.title = pairing.qr.urls.join("  ");
      this.show(pairing.qr.urls.length ? undefined : "No network address. Join the phone's Wi-Fi, then restart scripts/serve.sh.");
    }
  }

  private show(problem: string | undefined) {
    this.problem.hidden = !problem;
    this.problem.textContent = problem ?? "";
  }
}

/// Who signed a capture, in the words of the presenter page.
/// A Secure Enclave key is declared by the app unless the laptop verified an
/// App Attest attestation when it paired.
function deviceKind(status: api.Status): string {
  const kind = status.device_kind ?? (status.capture.source === "simulated" ? "laptop" : "browser");
  if (kind === "secure-enclave" && status.device_attested) {
    return `Secure Enclave, attested (${status.device_attested})`;
  }
  return {
    "secure-enclave": "Secure Enclave, declared",
    software: "Software key",
    browser: "Browser key",
    laptop: "Laptop",
  }[kind];
}

/// The row a capture adds under "Details" after its key: an external GPS.
/// A simulated fix is already a pill in the header.
function captureDetails(status: api.Status): [string, string][] {
  return status.capture.location?.accessory && !status.capture.location.simulated ? [["GPS", "External"]] : [];
}

let showCaptures = () => {};

function tabs() {
  const select = (verify: boolean) => {
    verifying = verify;
    $("tab-captures").setAttribute("aria-selected", String(!verify));
    $("tab-verify").setAttribute("aria-selected", String(verify));
    detail.hidden = verify;
    verifyView.hidden = !verify;
    if (!verify) shown?.map.resize();
    renderList();
  };
  showCaptures = () => select(false);
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
            title: time(status.captured_at),
            "aria-current": status.id === selected && !verifying ? "true" : undefined,
            onclick: () => {
              if (verifying) showCaptures();
              selected = status.id;
              followNewest = status.id === captures[0]?.id;
              void refresh();
            },
          },
          h("span", { class: "capture-time num" }, when(status.captured_at)),
          phaseMark(status.phase),
        ),
      ),
    ),
  );
}

function renderEmpty() {
  shown = undefined;
  detail.replaceChildren(
    h("div", { class: "empty" }, h("h2", {}, "Waiting for a capture"), h("p", { class: "meta" }, "Scan the QR with the app.")),
  );
}

function renderDetail(status: api.Status) {
  if (shown?.id !== status.id) buildDetail(status);
  const { parts, timeline } = shown!;
  parts.badges!.replaceChildren(...badges(status));
  timeline.update(status.events, status.phase === "failed" ? (status.error ?? "failed") : undefined);
  parts.steps!.hidden = !status.events.length && !api.isRunning(status.phase);

  const key = `${status.phase}`;
  if (parts.published!.dataset.key !== key) {
    parts.published!.dataset.key = key;
    parts.published!.replaceChildren(
      status.published && !api.isRunning(status.phase)
        ? h("img", { src: `${api.signedUrl(status.id)}?${Date.now()}`, alt: "The published file" })
        : h("div", { class: "placeholder" }, api.isRunning(status.phase) ? h("span", { class: "spinner", "aria-label": "Publishing" }) : "Not published"),
    );
    showOutcome(status, shown!.km2);
    parts.actions!.replaceChildren(...actions(status));
  }
}

/// The images on top, the steps under them, and the cell with the verdict on
/// the right. The capture detail and a verified file share it.
function board(images: HTMLElement[], steps: HTMLElement | false, region: HTMLElement): HTMLElement {
  return h(
    "div",
    { class: "board" },
    h("div", { class: "board-main" }, h("div", { class: "images" }, images), steps),
    region,
  );
}

/// A captioned image whose width follows its aspect ratio, so images side by side share one height.
function figure(caption: string | false, frame: HTMLElement, width: number, height: number): HTMLElement {
  frame.style.aspectRatio = `${width} / ${height}`;
  return h(
    "figure",
    { style: `flex: ${width / height} 1 0` },
    caption && h("figcaption", { class: "meta" }, caption),
    frame,
  );
}

function buildDetail(status: api.Status) {
  const timeline = new Timeline(["publish", "verify"]);
  const map = new CellMap();
  const parts: Record<string, HTMLElement> = {
    badges: h("span", { class: "badges" }),
    published: h("div", { class: "frame" }),
    verdict: h("div", { class: "outcome" }),
    outcome: h("div", { class: "outcome" }),
    actions: h("div", { class: "detail-actions" }),
    steps: h("div", { class: "card pad" }, timeline.element),
  };
  const { crop } = status.capture;
  const { width, height } = status.original;
  const published = turnSize(crop, status.capture.turn ?? 0);
  const overlay = h("div", {
    class: "overlay",
    style: `left:${(crop.x / width) * 100}%;top:${(crop.y / height) * 100}%;width:${(crop.width / width) * 100}%;height:${(crop.height / height) * 100}%`,
  });
  const original = h("div", { class: "frame" }, h("img", { src: api.originalUrl(status.id), alt: "The private original" }), overlay);
  detail.replaceChildren(
    h(
      "div",
      { class: "detail-head" },
      h(
        "div",
        { class: "detail-title" },
        h("div", { class: "title-line" }, h("h2", { class: "num", title: time(status.captured_at) }, when(status.captured_at)), parts.badges),
        h(
          "p",
          { class: "meta num", title: status.id },
          h("span", { class: "mono" }, short(status.device)),
          status.capture.accuracy_meters ? ` · ±${Math.round(status.capture.accuracy_meters)} m` : "",
        ),
      ),
      parts.actions,
    ),
    board(
      [figure("Original · private", original, width, height), figure("Published", parts.published!, published.width, published.height)],
      parts.steps!,
      h("div", { class: "card pad region" }, parts.verdict, map.element, parts.outcome),
    ),
  );
  shown = { id: status.id, timeline, map, parts };
  const building = shown;
  void map.show(status.capture.cell).then((outline) => {
    if (!outline || shown !== building) return;
    building.km2 = outline.area_km2;
    showOutcome(captures.find((each) => each.id === status.id) ?? status, outline.area_km2);
  });
}

function showOutcome(status: api.Status, km2?: number) {
  const { parts } = shown!;
  const { phase } = status;
  const verify = verifyButton(status);
  if (phase === "accepted" && status.claim) {
    parts.verdict!.replaceChildren(verdict.accepted(verify));
    parts.outcome!.replaceChildren(
      verdict.facts(status.claim, km2, { key: deviceKind(status), details: captureDetails(status) }),
      h("p", { class: "caveat" }, verdict.CAVEAT),
    );
    return;
  }
  parts.outcome!.replaceChildren();
  parts.verdict!.replaceChildren(
    phase === "rejected"
      ? verdict.rejected("Rejected", verdict.stopped(status.events, status.error), verify)
      : phase === "failed"
        ? verdict.rejected("Publishing failed", verdict.stopped(status.events, status.error))
        : phase === "stored"
          ? verdict.pending("idle", "Not published", h("button", { type: "button", onclick: (event: Event) => void publish(status, event) }, "Publish"))
          : phase === "published"
            ? verdict.pending("idle", "Not verified", verify)
            : verdict.pending("running", PHASE[phase]),
  );
}

/// Verify sits on the verdict line in every state: the primary "Verify"
/// while a published capture has no verdict, an icon once it has one.
function verifyButton(status: api.Status): HTMLElement | undefined {
  if (!status.published || api.isRunning(status.phase)) return undefined;
  const onclick = () => void reverify(status);
  if (status.phase === "published") return h("button", { type: "button", class: "verify-button", onclick }, "Verify");
  return h(
    "button",
    { type: "button", class: "verify-button secondary icon-button", "aria-label": "Verify again", title: "Verify again", onclick },
    svg(icon.again),
  );
}

function actions(status: api.Status): HTMLElement[] {
  const buttons: HTMLElement[] = [];
  if (api.isRunning(status.phase)) return buttons;
  if (status.phase === "rejected" || status.phase === "failed") {
    buttons.push(h("button", { type: "button", class: "secondary", onclick: (event: Event) => void publish(status, event) }, "Publish again"));
  }
  if (status.published) {
    const primary = status.phase === "accepted";
    buttons.push(h("a", { class: primary ? "button" : "button secondary", href: api.signedUrl(status.id), download: `${status.id}.png` }, "Download"));
  }
  return buttons;
}

async function publish(status: api.Status, event: Event) {
  (event.currentTarget as HTMLButtonElement).disabled = true;
  await api.republish(status.id).catch((error) => alert(message(error)));
  await refresh();
}

async function reverify(status: api.Status) {
  const png = await (await fetch(api.signedUrl(status.id))).blob();
  $("tab-verify").click();
  await verifyFile(new File([png], `${status.id}.png`, { type: "image/png" }));
}

const PHASE: Record<api.Phase, string> = {
  stored: "Stored",
  published: "Published",
  queued: "Queued",
  publishing: "Proving",
  verifying: "Verifying",
  accepted: "Verified",
  rejected: "Rejected",
  failed: "Failed",
};

/// The header's pill: a simulated fix, which a reader should not miss. The
/// phase is the verdict line's.
function badges(status: api.Status): HTMLElement[] {
  return status.capture.location?.simulated ? [h("span", { class: "pill warn" }, "Simulated GPS")] : [];
}

/// The rail's mark: a verdict, a spinner, or a faint dot for a capture with no verdict here.
function phaseMark(phase: api.Phase): HTMLElement {
  const label = h("span", { class: "sr-only" }, PHASE[phase]);
  const tone = phase === "accepted" ? "ok" : phase === "rejected" || phase === "failed" ? "bad" : api.isRunning(phase) ? "running" : "idle";
  return h("span", { class: "phase-mark" }, verdict.mark(tone), label);
}

// Verify a file

const result = h("div", { class: "verify-result" });
const dropLabel = h("span", { class: "drop-label" }, "Drop a PNG to verify, or click to choose");
const dropAgain = h("span", { class: "drop-again", hidden: true }, "Choose another");

function dropZone(): HTMLElement[] {
  const input = h("input", { type: "file", accept: "image/png", hidden: true }) as HTMLInputElement;
  input.onchange = () => input.files?.[0] && void verifyFile(input.files[0]);
  const zone = h("label", { class: "drop", id: "drop" }, input, dropLabel, dropAgain);
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
  $("drop").classList.add("compact");
  dropLabel.replaceChildren(h("span", { class: "drop-name" }, file.name), h("span", { class: "muted num" }, ` · ${Math.round(file.size / 1000)} KB`));
  dropAgain.hidden = false;
  const preview = URL.createObjectURL(file);
  const timeline = new Timeline(["verify"]);
  timeline.update([{ event: "checking", check: "manifest", intent: "reading the C2PA manifest and validating its claim signature" }]);
  const verdictSlot = h("div", { class: "outcome" }, verdict.pending("running", "Verifying"));
  const outcomeSlot = h("div", { class: "outcome" });
  const map = new CellMap();
  map.element.hidden = true;
  const region = h("div", { class: "card pad region" }, verdictSlot, map.element, outcomeSlot);
  result.replaceChildren(
    board(
      [h("figure", { class: "solo" }, h("div", { class: "well" }, h("img", { src: preview, alt: file.name })))],
      h("div", { class: "card pad" }, timeline.element),
      region,
    ),
  );
  try {
    const report = await api.verify(file);
    timeline.update(report.events);
    const { claim, rejected } = report.verdict;
    if (claim) {
      verdictSlot.replaceChildren(verdict.accepted());
      outcomeSlot.replaceChildren(verdict.facts(claim), h("p", { class: "caveat" }, verdict.CAVEAT));
      map.element.hidden = false;
      void map.show(claim.cell).then((outline) => outline && outcomeSlot.firstElementChild?.replaceWith(verdict.facts(claim, outline.area_km2)));
    } else if (rejected) {
      verdictSlot.replaceChildren(verdict.rejected("Rejected", verdict.reason(rejected.check, rejected.detail)));
    }
  } catch (error) {
    timeline.update([{ event: "checking", check: "manifest", intent: "reading the C2PA manifest" }], message(error));
    verdictSlot.replaceChildren(verdict.rejected("Could not verify", verdict.tidy(message(error))));
  }
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

void main();
