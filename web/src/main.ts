// The capture flow: pair once, take a photo, frame it, choose the crop and
// the resolution, then compute both hashes, sign the receipt on the device,
// and upload.

import * as api from "./api";
import { base64, framing, type Framing } from "./camera";
import { cropSelector, type CropSelector } from "./crop";
import { envelope, freshSalt } from "./envelope";
import { fingerprint } from "./fingerprint";
import { loadOrCreateKey, type DeviceKey } from "./key";
import { signReceipt, timestamp } from "./receipt";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const deviceStatus = $("device-status");
const pairForm = $<HTMLFormElement>("pair-form");
const gpsStatus = $("gps");
const framingSection = $("framing");
const cellStatus = $("cell");
const resolutionInput = $<HTMLInputElement>("resolution");
const submitButton = $<HTMLButtonElement>("submit");
const result = $("result");
const log = $("log");

let key: DeviceKey;
let crop: CropSelector;
let fix: GeolocationPosition | undefined;
let current: { framing: Framing; capturedAt: string; fix: GeolocationPosition } | undefined;

async function main() {
  key = await loadOrCreateKey();
  deviceStatus.textContent = `Device ${key.id.slice(0, 4)}…${key.id.slice(-4)}. Pair it once with the code the laptop shows.`;
  pairForm.hidden = false;
  pairForm.onsubmit = async (event) => {
    event.preventDefault();
    try {
      await api.pair($<HTMLInputElement>("pair-code").value, key.pem);
      deviceStatus.textContent = `Device ${key.id.slice(0, 4)}…${key.id.slice(-4)}, paired.`;
      pairForm.hidden = true;
    } catch (error) {
      deviceStatus.textContent = String(error);
    }
  };

  crop = cropSelector($("stage"), $("crop-box"), (rect) => {
    $("crop").textContent = `Crop: ${rect.width}x${rect.height} at (${rect.x}, ${rect.y}). Drag on the photo to change it.`;
  });

  // Watching from page load means the fix is fresh when the photo arrives.
  navigator.geolocation.watchPosition(
    (position) => {
      fix = position;
      gpsStatus.textContent = `GPS fix, ±${Math.round(position.coords.accuracy)} m.`;
    },
    (error) => (gpsStatus.textContent = `No GPS fix: ${error.message}`),
    { enableHighAccuracy: true, maximumAge: 0 },
  );

  $<HTMLInputElement>("photo").onchange = async (event) => {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;
    // The coordinate and the time are taken now, when the photo reaches the
    // page, which is a few seconds after the shutter.
    const capturedAt = timestamp(new Date());
    if (!fix) return (gpsStatus.textContent = "No GPS fix yet; try again in a moment.");
    current = { framing: await framing(file, $<HTMLCanvasElement>("preview")), capturedAt, fix };
    framingSection.hidden = false;
    result.hidden = true;
    await lookupCell();
  };
  $<HTMLInputElement>("frame").oninput = (event) =>
    current?.framing.frame(Number((event.target as HTMLInputElement).value));
  resolutionInput.oninput = () => {
    $("resolution-value").textContent = resolutionInput.value;
    void lookupCell();
  };
  submitButton.onclick = () => void submit();
}

async function lookupCell() {
  if (!current) return;
  cellStatus.textContent = "Cell: …";
  try {
    const { coords } = current.fix;
    const region = await api.cell(coords.latitude, coords.longitude, Number(resolutionInput.value));
    cellStatus.textContent = `Cell: ${region.cell}`;
    cellStatus.dataset.cell = region.cell;
  } catch (error) {
    cellStatus.textContent = `Cell: ${String(error)}`;
    delete cellStatus.dataset.cell;
  }
}

async function submit() {
  const cell = cellStatus.dataset.cell;
  if (!current || !cell) return;
  submitButton.disabled = true;
  result.hidden = false;
  log.className = "";
  log.textContent = "";
  try {
    const { coords } = current.fix;
    const secrets = { latitude: coords.latitude, longitude: coords.longitude, salt: freshSalt() };
    const { channels, png } = await current.framing.original();
    say("Fingerprinting the original…");
    const print = await fingerprint(channels, (done, total) => say(`Fingerprinting the original… ${done}/${total}`));
    say("Fingerprint done.");
    const sealed = envelope(secrets.latitude, secrets.longitude, secrets.salt);
    say(`Envelope ${sealed.slice(0, 6)}…${sealed.slice(-4)}.`);
    const receipt = await signReceipt(key, print, sealed, current.capturedAt);
    say(`Receipt signed by this device at ${receipt.captured_at}.`);
    say("Uploading…");
    const accepted = await api.upload({
      original_png: await base64(png),
      secrets,
      receipt,
      cell,
      resolution: Number(resolutionInput.value),
      crop: crop.get(),
      accuracy_meters: coords.accuracy,
    });
    say(`Accepted as ${accepted.id}.\nOn the laptop: provenance publish --capture ${accepted.dir}`);
  } catch (error) {
    log.className = "error";
    say(String(error));
  } finally {
    submitButton.disabled = false;
  }
}

function say(line: string) {
  const lines = log.textContent.split("\n").filter(Boolean);
  if (lines.at(-1)?.startsWith("Fingerprinting") && line.startsWith("Fingerprinting")) lines.pop();
  log.textContent = [...lines, line].join("\n");
}

void main();
