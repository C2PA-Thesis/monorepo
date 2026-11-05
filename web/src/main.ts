// The capture flow on the phone: pair once, take a photo, choose the crop
// and the cell, compute both hashes and sign the receipt on the device,
// upload, then follow the laptop as it proves, signs and verifies.

import "./style.css";
import "./phone.css";
import * as api from "./api";
import { base64, framing, type Framing } from "./camera";
import { cropSelector, PRESETS, type CropSelector, type Rect } from "./crop";
import { $, area, cellShort, h, icon, rect, short, svg, when } from "./dom";
import { envelope, freshSalt, hints } from "./envelope";
import { fingerprint, type Size } from "./fingerprint";
import { loadOrCreateKey, type DeviceKey } from "./key";
import { CellMap } from "./map";
import { signReceipt, timestamp } from "./receipt";
import { Timeline } from "./timeline";
import * as verdict from "./verdict";

type StepName = "pair" | "photo" | "frame" | "region" | "send" | "laptop";
type StepState = "locked" | "active" | "done";

interface Shot {
  framing: Framing;
  capturedAt: string;
  fix: GeolocationPosition;
}

let key: DeviceKey;
let crop: CropSelector;
let shot: Shot | undefined;
let fix: GeolocationPosition | undefined;
let gpsWaiters: ((fix: GeolocationPosition) => void)[] = [];
let region: { cell: string; area?: number } | undefined;
const map = new CellMap();

function step(name: StepName, state: StepState, ...summary: (string | Node)[]) {
  $(`step-${name}`).dataset.state = state;
  if (summary.length) $(`${name}-summary`).replaceChildren(...summary);
}

/// An identifier inside a step's summary, set in mono as the presenter sets it.
const mono = (text: string) => h("span", { class: "mono" }, text);

async function main() {
  $("map-slot").append(map.element);
  $("presets").append(
    ...PRESETS.map((preset) =>
      h("button", { type: "button", class: "chip", onclick: () => crop.set(preset.rect(crop.size())) }, preset.name),
    ),
  );
  crop = cropSelector($("stage"), $("crop-box"), onCrop);
  watchGps();
  wireControls();
  await pairing();
}

// 1. Pairing

async function pairing() {
  key = await loadOrCreateKey();
  const code = new URLSearchParams(location.hash.slice(1)).get("pair");
  if (code) history.replaceState(null, "", location.pathname);

  let trusted: boolean;
  try {
    trusted = await api.isTrusted(key.id);
    if (!trusted && code) {
      $("pair-status").textContent = "Pairing…";
      await api.pair(code, key.pem);
      trusted = true;
    }
  } catch (error) {
    $("pair-status").textContent = `Could not pair: ${message(error)}`;
    $("pair-status").className = "error-text";
    trusted = false;
  }
  if (trusted) return paired();

  if (!code) $("pair-status").textContent = "Enter the code on the laptop.";
  const form = $<HTMLFormElement>("pair-form");
  form.hidden = false;
  form.onsubmit = async (event) => {
    event.preventDefault();
    try {
      await api.pair($<HTMLInputElement>("pair-code").value.trim(), key.pem);
      paired();
    } catch (error) {
      $("pair-status").textContent = message(error);
      $("pair-status").className = "error-text";
    }
  };
}

function paired() {
  step("pair", "done", "Paired · ", mono(short(key.id)));
  step("photo", "active");
}

// 2. GPS and photo

function watchGps() {
  if (!("geolocation" in navigator)) return gps("error", "No location in this browser");
  if (!window.isSecureContext) {
    return gps("error", "Location needs HTTPS", "Open the https address the laptop shows.");
  }
  navigator.geolocation.watchPosition(
    (position) => {
      fix = position;
      gps("ok", `±${Math.round(position.coords.accuracy)} m`);
      for (const waiter of gpsWaiters) waiter(position);
      gpsWaiters = [];
    },
    (error) => {
      const denied = error.code === error.PERMISSION_DENIED;
      gps(
        "error",
        denied ? "Location blocked" : "No GPS fix yet",
        denied ? locationHelp() : undefined,
      );
    },
    { enableHighAccuracy: true, maximumAge: 0 },
  );
}

/// One sentence naming where this browser lets location back in.
function locationHelp(): string {
  const ios = /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  if (!ios) return "Allow location for this site, then reload.";
  const safari = !/CriOS|FxiOS|EdgiOS/.test(navigator.userAgent);
  return safari ? "Allow Location for Safari in Settings, then reload." : "Allow Location for this browser in Settings, then reload.";
}

function gps(state: "waiting" | "ok" | "error", text: string, help?: string) {
  $("gps").dataset.state = state;
  $("gps-text").textContent = text;
  $("gps-help").hidden = !help;
  $("gps-help").textContent = help ?? "";
}

function nextFix(): Promise<GeolocationPosition> {
  return fix ? Promise.resolve(fix) : new Promise((resolve) => gpsWaiters.push(resolve));
}

async function onPhoto(file: File) {
  // The time is taken when the photo reaches the page, a few seconds after the shutter.
  const capturedAt = timestamp(new Date());
  $("photo-error").hidden = true;
  const position = await nextFix();
  shot = { framing: await framing(file, $<HTMLCanvasElement>("preview")), capturedAt, fix: position };
  crop.reset(shot.framing.size);
  $("result").hidden = true;
  step("photo", "done", `${when(capturedAt)} · ±${Math.round(position.coords.accuracy)} m`);
  for (const name of ["frame", "region", "send"] as const) step(name, "active");
  step("laptop", "locked", "");
  foldable($("step-laptop"), false);
  $<HTMLButtonElement>("submit").disabled = false;
  $("local-steps").hidden = true;
  map.resize();
  await lookupCell();
}

// 3. Crop

function onCrop(chosen: Rect, size: Size) {
  $("frame-summary").textContent = rect(chosen);
  for (const chip of $("presets").children) {
    const preset = PRESETS.find((each) => each.name === chip.textContent);
    chip.setAttribute("aria-pressed", String(!!preset && sameRect(preset.rect(size), chosen)));
  }
}

const sameRect = (a: Rect, b: Rect) => a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;

// 4. Cell

let lookup = 0;
async function lookupCell() {
  if (!shot) return;
  const resolution = Number($<HTMLInputElement>("resolution").value);
  $("resolution-text").textContent = verdict.resolutionText(resolution);
  const mine = ++lookup;
  const { coords } = shot.fix;
  try {
    const found = await api.cell(coords.latitude, coords.longitude, resolution);
    if (mine !== lookup) return;
    region = { cell: found.cell };
    $("cell-text").hidden = true;
    $<HTMLButtonElement>("submit").disabled = false;
    const outline = await map.show(found.cell, coords);
    if (outline && mine === lookup) {
      region.area = outline.area_km2;
      $("resolution-text").textContent = verdict.resolutionText(resolution, outline.area_km2);
      warnAccuracy(outline.area_km2, coords.accuracy);
    }
  } catch (error) {
    if (mine !== lookup) return;
    region = undefined;
    $("cell-text").textContent = `${message(error)}. Try another size.`;
    $("cell-text").hidden = false;
    $<HTMLButtonElement>("submit").disabled = true;
  }
  $("region-summary").replaceChildren(...(region ? [mono(cellShort(region.cell)), region.area ? ` · ${area(region.area)}` : ""] : []));
}

function warnAccuracy(km2: number, accuracy: number) {
  // A fix less accurate than the cell is wide may put the photo in a neighbouring cell.
  const width = Math.sqrt(km2) * 1000;
  const warning = $("accuracy-warning");
  warning.hidden = accuracy * 2 < width;
  warning.textContent = `GPS ±${Math.round(accuracy)} m: the photo may fall in a neighbouring cell.`;
}

// 5. Sign and send

async function submit() {
  if (!shot || !region) return;
  const button = $<HTMLButtonElement>("submit");
  button.disabled = true;
  for (const name of ["frame", "region"] as const) step(name, "done");
  step("photo", "done");
  const local = new LocalSteps($("local-steps"));
  try {
    const { coords } = shot.fix;
    const secrets = {
      latitude: coords.latitude,
      longitude: coords.longitude,
      salt: freshSalt(),
      hints: hints(coords.latitude, coords.longitude),
    };
    const { channels, png } = await shot.framing.original();

    local.start("Fingerprint");
    const print = await fingerprint(channels, shot.framing.size, (done, total) => local.progress(done / total));
    local.done();
    local.start("Seal position");
    const sealed = envelope(secrets.latitude, secrets.longitude, secrets.salt, secrets.hints);
    local.done();
    local.start("Sign");
    const receipt = await signReceipt(key, print, sealed, shot.capturedAt);
    local.done();
    local.start("Send");
    const accepted = await api.upload({
      original_png: await base64(png),
      secrets,
      receipt,
      cell: region.cell,
      resolution: Number($<HTMLInputElement>("resolution").value),
      crop: crop.get(),
      accuracy_meters: coords.accuracy,
    });
    local.done();
    step("send", "done", "Sent");
    await follow(accepted.id);
  } catch (error) {
    local.fail(message(error));
    button.disabled = false;
    for (const name of ["frame", "region"] as const) step(name, "active");
  }
}

class LocalSteps {
  private current: HTMLElement | undefined;
  private started = 0;

  constructor(private list: HTMLElement) {
    list.replaceChildren();
    list.hidden = false;
  }

  start(title: string) {
    this.started = performance.now();
    this.current = h(
      "li",
      { class: "is-running" },
      h(
        "div",
        { class: "row-line" },
        h("span", { class: "mark" }, h("span", { class: "spinner" })),
        h("span", { class: "row-title" }, title),
        h("span", { class: "seconds" }),
      ),
    );
    this.list.append(this.current);
  }

  progress(fraction: number) {
    const body = this.current;
    if (!body) return;
    const bar = body.querySelector(".progress > span") as HTMLElement | null;
    if (bar) bar.style.width = `${fraction * 100}%`;
    else body.append(h("div", { class: "progress" }, h("span", { style: `width: ${fraction * 100}%` })));
  }

  done() {
    this.finish("is-done", icon.check);
  }

  fail(reason: string) {
    if (!this.current) this.start("Prepare");
    this.finish("is-failed", icon.cross, reason);
  }

  private finish(state: string, mark: string, reason?: string) {
    if (!this.current) return;
    this.current.className = state;
    this.current.querySelector(".mark")!.replaceChildren(svg(mark));
    // Steps under a tenth of a second show no time, as on the timeline.
    const took = (performance.now() - this.started) / 1000;
    this.current.querySelector(".seconds")!.textContent = took >= 0.05 ? seconds(took) : "";
    this.current.querySelector(".progress")?.remove();
    if (reason) this.current.append(h("p", { class: "row-reason" }, reason));
    this.current = undefined;
  }
}

// 6. The laptop

async function follow(id: string) {
  step("laptop", "active", "");
  foldable($("step-laptop"), false);
  const timeline = new Timeline(["publish", "verify"]);
  $("laptop-body").replaceChildren(timeline.element);
  let status: api.Status | undefined;
  for (;;) {
    try {
      status = await api.status(id);
    } catch {
      await sleep(1500);
      continue;
    }
    if (status.phase === "stored") {
      $("laptop-body").replaceChildren(
        h("p", { class: "muted" }, "Stored, not published. Run ", h("code", {}, `scripts/publish.sh ${id}`), " on the laptop."),
      );
      return;
    }
    timeline.update(status.events, status.phase === "failed" ? (status.error ?? "failed") : undefined);
    if (!api.isRunning(status.phase)) break;
    await sleep(700);
  }
  // The result card says the verdict; the folded row says how long it took.
  step("laptop", "done", seconds(total(status.events)));
  foldable($("step-laptop"), true);
  showResult(status);
}

/// The time the laptop's steps took, as the timeline's groups add it up.
function total(events: api.Event[]): number {
  return events.reduce(
    (sum, event) => sum + (event.event === "finished" || event.event === "passed" || event.event === "rejected" ? event.seconds : 0),
    0,
  );
}

/// A done step whose head reopens its body, as the Publishing step keeps its timeline.
function foldable(card: HTMLElement, on: boolean) {
  const head = card.querySelector<HTMLElement>(".step-head")!;
  delete card.dataset.open;
  if (!on) {
    for (const name of ["role", "tabindex", "aria-expanded"]) head.removeAttribute(name);
    head.onclick = head.onkeydown = null;
    return;
  }
  head.setAttribute("role", "button");
  head.tabIndex = 0;
  head.setAttribute("aria-expanded", "false");
  const toggle = () => {
    const open = card.dataset.open === undefined;
    if (open) card.dataset.open = "";
    else delete card.dataset.open;
    head.setAttribute("aria-expanded", String(open));
  };
  head.onclick = toggle;
  head.onkeydown = (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    toggle();
  };
}

// 7. Result

function showResult(status: api.Status) {
  const card = $("result");
  card.hidden = false;
  const again = h("button", { type: "button", class: "secondary", onclick: retake }, "Take another");
  if (status.phase !== "accepted" || !status.claim) {
    card.replaceChildren(
      verdict.rejected(
        status.phase === "rejected" ? "Rejected" : "Publishing failed",
        verdict.stopped(status.events, status.error),
      ),
      h("div", { class: "actions single" }, again),
    );
    card.scrollIntoView({ behavior: "smooth" });
    return;
  }
  const resultMap = new CellMap();
  const claim = status.claim;
  const facts = verdict.facts(claim);
  // The same order as the presenter's verdict card: verdict, image, map, facts, caveat, then the actions.
  card.replaceChildren(
    verdict.accepted(),
    h(
      "div",
      { class: "well" },
      h("img", { class: "published-image", src: api.signedUrl(status.id), alt: "The published crop" }),
    ),
    resultMap.element,
    facts,
    h("p", { class: "caveat" }, verdict.CAVEAT),
    h(
      "div",
      { class: "actions" },
      h("a", { class: "button", href: api.signedUrl(status.id), download: `${status.id}.png` }, "Download"),
      again,
    ),
  );
  card.scrollIntoView({ behavior: "smooth" });
  void resultMap.show(claim.cell).then((outline) => outline && facts.replaceWith(verdict.facts(claim, outline.area_km2)));
}

function retake() {
  $("result").hidden = true;
  shot = undefined;
  step("photo", "active", "");
  for (const name of ["frame", "region", "send", "laptop"] as const) step(name, "locked", "");
  foldable($("step-laptop"), false);
  $("local-steps").hidden = true;
  $<HTMLInputElement>("photo").value = "";
  $("step-photo").scrollIntoView({ behavior: "smooth" });
}

function wireControls() {
  $<HTMLInputElement>("photo").onchange = (event) => {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (file) {
      void onPhoto(file).catch((error) => {
        $("photo-error").textContent = message(error);
        $("photo-error").hidden = false;
      });
    }
  };
  // A done photo step reopens the camera.
  $("step-photo").onclick = (event) => {
    if ($("step-photo").dataset.state === "done" && !(event.target as HTMLElement).closest("input")) {
      $<HTMLInputElement>("photo").click();
    }
  };
  let debounce: number | undefined;
  $<HTMLInputElement>("resolution").oninput = () => {
    $("resolution-text").textContent = verdict.resolutionText(Number($<HTMLInputElement>("resolution").value));
    window.clearTimeout(debounce);
    debounce = window.setTimeout(() => void lookupCell(), 250);
  };
  $("submit").onclick = () => void submit();
  $("resolution-text").textContent = verdict.resolutionText(7);
}

const seconds = (value: number) => `${value.toFixed(1)} s`;
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

void main();
