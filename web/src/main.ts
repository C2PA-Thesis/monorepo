// The capture flow on the phone: pair once, take a photo, choose the crop
// and the cell, compute both hashes and sign the receipt on the device,
// upload, then follow the laptop as it proves, signs and verifies.

import "./style.css";
import "./phone.css";
import * as api from "./api";
import { base64, framing, type Framing } from "./camera";
import { cropSelector, PRESETS, type CropSelector, type Rect } from "./crop";
import { $, area, h, icon, rect, short, svg, time } from "./dom";
import { envelope, freshSalt, hints } from "./envelope";
import { fingerprint } from "./fingerprint";
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

function step(name: StepName, state: StepState, summary?: string) {
  $(`step-${name}`).dataset.state = state;
  if (summary !== undefined) $(`${name}-summary`).textContent = summary;
}

async function main() {
  $("map-slot").append(map.element);
  $("presets").append(
    ...PRESETS.map((preset) =>
      h("button", { type: "button", class: "chip", onclick: () => crop.set(preset.rect) }, preset.name),
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
      $("pair-status").textContent = "Pairing with the laptop…";
      await api.pair(code, key.pem);
      trusted = true;
    }
  } catch (error) {
    $("pair-status").textContent = `Could not pair: ${message(error)}`;
    $("pair-status").className = "error-text";
    trusted = false;
  }
  if (trusted) return paired();

  if (!code) $("pair-status").textContent = `This phone's key is ${short(key.id)}. Type the code the laptop shows.`;
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
  step("pair", "done", `Trusted by the laptop as device ${short(key.id)}`);
  step("photo", "active");
}

// 2. GPS and photo

function watchGps() {
  if (!("geolocation" in navigator)) return gps("error", "This browser has no geolocation.");
  if (!window.isSecureContext) {
    return gps("error", "Location needs HTTPS. Open the https address the laptop shows.");
  }
  navigator.geolocation.watchPosition(
    (position) => {
      fix = position;
      gps("ok", `GPS fix, ±${Math.round(position.coords.accuracy)} m`);
      for (const waiter of gpsWaiters) waiter(position);
      gpsWaiters = [];
    },
    (error) => {
      const denied = error.code === error.PERMISSION_DENIED;
      gps(
        "error",
        denied ? "Location access is blocked for this page." : `No GPS fix yet: ${error.message}`,
        denied
          ? "On iPhone: Settings › Privacy & Security › Location Services › Safari Websites › While Using. Then reload this page."
          : undefined,
      );
    },
    { enableHighAccuracy: true, maximumAge: 0 },
  );
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
  $("photo-summary").textContent = fix ? "" : "Waiting for a GPS fix…";
  const position = await nextFix();
  shot = { framing: await framing(file, $<HTMLCanvasElement>("preview")), capturedAt, fix: position };
  $("frame-control").hidden = !shot.framing.slidable;
  $<HTMLInputElement>("frame").value = "0.5";
  $("result").hidden = true;
  step(
    "photo",
    "done",
    `${time(capturedAt)}, GPS ±${Math.round(position.coords.accuracy)} m. Tap to retake.`,
  );
  for (const name of ["frame", "region", "send"] as const) step(name, "active");
  step("laptop", "locked", "");
  $<HTMLButtonElement>("submit").disabled = false;
  $("local-steps").hidden = true;
  map.resize();
  await lookupCell();
}

// 3. Crop

function onCrop(chosen: Rect) {
  $("crop-text").textContent = `Now ${rect(chosen)}.`;
  $("frame-summary").textContent = `The ${rect(chosen)} of the 1024×512 frame`;
  for (const chip of $("presets").children) {
    const preset = PRESETS.find((each) => each.name === chip.textContent);
    chip.setAttribute("aria-pressed", String(!!preset && sameRect(preset.rect, chosen)));
  }
  renderPublished(chosen);
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
    $("cell-text").textContent = `Cell ${found.cell}. The published file names this cell, never the point inside it.`;
    $("cell-text").className = "hint";
    $<HTMLButtonElement>("submit").disabled = false;
    const outline = await map.show(found.cell, coords);
    if (outline && mine === lookup) {
      region.area = outline.area_km2;
      $("resolution-text").textContent = verdict.resolutionText(resolution, outline.area_km2);
      $("cell-text").textContent = `Cell ${found.cell}, ${area(outline.area_km2)}. The published file names this cell, never the point inside it.`;
      warnAccuracy(outline.area_km2, coords.accuracy);
    }
  } catch (error) {
    if (mine !== lookup) return;
    region = undefined;
    $("cell-text").textContent = `${message(error)}. Try another size.`;
    $("cell-text").className = "hint error-text";
    $<HTMLButtonElement>("submit").disabled = true;
  }
  $("region-summary").textContent = region
    ? `Cell ${region.cell}${region.area ? `, ${area(region.area)}` : ""}`
    : "";
  renderPublished();
}

function warnAccuracy(km2: number, accuracy: number) {
  // A fix less accurate than the cell is wide may put the photo in a neighbouring cell.
  const width = Math.sqrt(km2) * 1000;
  const warning = $("accuracy-warning");
  warning.hidden = accuracy * 2 < width;
  warning.textContent = `The GPS fix is only accurate to ±${Math.round(accuracy)} m, which is coarse for a cell this small. The photo may have been taken in a neighbouring cell.`;
}

// 5. Sign and send

function renderPublished(chosen: Rect = crop.get()) {
  const items = [
    `The ${rect(chosen)} crop`,
    region ? `Cell ${region.cell}${region.area ? ` (${area(region.area)})` : ""}` : "The cell",
    shot ? `Capture time, ${time(shot.capturedAt)}` : "The capture time",
    `This phone's key id, ${short(key?.id ?? "")}`,
    "A fingerprint of the whole frame (384 numbers)",
    "An envelope that seals the coordinate",
  ];
  $("published-list").replaceChildren(...items.map((item) => h("li", {}, item)));
}

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

    local.start("Fingerprint the whole frame, in this browser");
    const print = await fingerprint(channels, (done, total) => local.progress(done / total));
    local.done();
    local.start("Seal the coordinate in an envelope (MiMC)");
    const sealed = envelope(secrets.latitude, secrets.longitude, secrets.salt, secrets.hints);
    local.done(short(sealed));
    local.start("Sign fingerprint and envelope with this phone's key");
    const receipt = await signReceipt(key, print, sealed, shot.capturedAt);
    local.done(`device ${short(receipt.device)}`);
    local.start("Send to the laptop, which checks it as a reader would");
    const accepted = await api.upload({
      original_png: await base64(png),
      secrets,
      receipt,
      cell: region.cell,
      resolution: Number($<HTMLInputElement>("resolution").value),
      crop: crop.get(),
      accuracy_meters: coords.accuracy,
    });
    local.done(accepted.id);
    step("send", "done", `Signed and accepted as ${accepted.id}`);
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
      { class: "row is-running" },
      h("span", { class: "mark" }, h("span", { class: "spinner" })),
      h("div", { class: "row-body" }, h("div", { class: "row-title" }, h("span", {}, title), h("span", { class: "seconds" }))),
    );
    this.list.append(this.current);
  }

  progress(fraction: number) {
    const body = this.current?.querySelector(".row-body");
    if (!body) return;
    const bar = body.querySelector(".progress > span") as HTMLElement | null;
    if (bar) bar.style.width = `${fraction * 100}%`;
    else body.append(h("div", { class: "progress" }, h("span", { style: `width: ${fraction * 100}%` })));
  }

  done(detail?: string) {
    this.finish("is-done", icon.check, detail);
  }

  fail(reason: string) {
    if (!this.current) this.start("Prepare the capture");
    this.finish("is-failed", icon.cross, reason);
  }

  private finish(state: string, mark: string, detail?: string) {
    if (!this.current) return;
    this.current.className = `row ${state}`;
    this.current.querySelector(".mark")!.replaceChildren(svg(mark));
    this.current.querySelector(".seconds")!.textContent = `${((performance.now() - this.started) / 1000).toFixed(1)} s`;
    this.current.querySelector(".progress")?.remove();
    if (detail) this.current.querySelector(".row-body")!.append(h("div", { class: "row-detail" }, detail));
    this.current = undefined;
  }
}

// 6. The laptop

async function follow(id: string) {
  step("laptop", "active", "");
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
        h(
          "p",
          {},
          "The laptop stored the capture without publishing it. Publish it there with ",
          h("code", {}, `scripts/publish.sh ${id}`),
          ".",
        ),
      );
      return;
    }
    timeline.update(status.events, status.phase === "failed" ? (status.error ?? "failed") : undefined);
    if (!api.isRunning(status.phase)) break;
    $("laptop-summary").textContent =
      status.phase === "queued" ? "Waiting for another capture to finish" : "Working…";
    await sleep(700);
  }
  $("laptop-summary").textContent =
    status.phase === "accepted" ? "Published and verified" : status.phase === "rejected" ? "Rejected" : "Failed";
  showResult(status);
}

// 7. Result

function showResult(status: api.Status) {
  const card = $("result");
  card.hidden = false;
  const again = h("button", { type: "button", class: "button-quiet", onclick: retake }, "Take another");
  if (status.phase !== "accepted" || !status.claim) {
    card.replaceChildren(
      verdict.rejected(
        status.phase === "rejected" ? "Rejected" : "Publishing failed",
        status.error ?? "unknown error",
      ),
      h("div", { class: "actions" }, again),
    );
    card.scrollIntoView({ behavior: "smooth" });
    return;
  }
  const resultMap = new CellMap();
  card.replaceChildren(
    verdict.accepted(status.claim),
    h(
      "div",
      { class: "checker", style: "border-radius: 8px; padding: 0.5rem" },
      h("img", { class: "published-image", src: api.signedUrl(status.id), alt: "The published crop" }),
    ),
    verdict.facts(status.claim),
    resultMap.element,
    h(
      "div",
      { class: "actions" },
      h("a", { class: "button", href: api.signedUrl(status.id), download: `${status.id}.png` }, "Download"),
      again,
    ),
    h("p", { class: "caveat" }, verdict.CAVEAT),
  );
  card.scrollIntoView({ behavior: "smooth" });
  void resultMap.show(status.claim.cell);
}

function retake() {
  $("result").hidden = true;
  shot = undefined;
  step("photo", "active", "");
  for (const name of ["frame", "region", "send", "laptop"] as const) step(name, "locked", "");
  $("local-steps").hidden = true;
  $<HTMLInputElement>("photo").value = "";
  $("step-photo").scrollIntoView({ behavior: "smooth" });
}

function wireControls() {
  $<HTMLInputElement>("photo").onchange = (event) => {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (file) void onPhoto(file).catch((error) => ($("photo-summary").textContent = message(error)));
  };
  // A done photo step reopens the camera.
  $("step-photo").onclick = (event) => {
    if ($("step-photo").dataset.state === "done" && !(event.target as HTMLElement).closest("input")) {
      $<HTMLInputElement>("photo").click();
    }
  };
  $<HTMLInputElement>("frame").oninput = (event) =>
    shot?.framing.frame(Number((event.target as HTMLInputElement).value));
  let debounce: number | undefined;
  $<HTMLInputElement>("resolution").oninput = () => {
    $("resolution-text").textContent = verdict.resolutionText(Number($<HTMLInputElement>("resolution").value));
    window.clearTimeout(debounce);
    debounce = window.setTimeout(() => void lookupCell(), 250);
  };
  $("submit").onclick = () => void submit();
  $("resolution-text").textContent = verdict.resolutionText(7);
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

void main();
