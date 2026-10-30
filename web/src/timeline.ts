// The publish stages and the reader checks as a live list: what each step
// establishes, whether it is waiting, running, done or refused, and its
// timing. Built from the events `provenance serve` reports.

import type { Event } from "./api";
import { h, icon, svg } from "./dom";

interface Row {
  key: string;
  title: string;
  what: string;
  /// Seconds the step usually takes on a laptop, when it is worth saying.
  typical?: number;
}

const STAGES: Row[] = [
  { key: "crop", title: "Crop", what: "Cuts the chosen rectangle out of the private original." },
  {
    key: "location-proof",
    title: "Location proof",
    what: "Groth16: the coordinate sealed in the envelope lies in the cell.",
  },
  {
    key: "crop-proof",
    title: "Crop proof",
    what: "HyperVerITAS PST: the published pixels are that rectangle of the fingerprinted original.",
    typical: 15,
  },
  { key: "publish", title: "C2PA manifest", what: "Embeds the receipt and both proofs, signed by the editor." },
];

const CHECKS: Row[] = [
  { key: "manifest", title: "C2PA manifest", what: "The file carries a valid C2PA claim with one zkloc assertion." },
  { key: "device", title: "Device receipt", what: "A trusted device signed the fingerprint and the envelope together." },
  { key: "image", title: "Crop proof", what: "These pixels are the claimed rectangle of the fingerprinted original." },
  { key: "location", title: "Location proof", what: "The coordinate in the signed envelope lies in the claimed cell." },
];

type State =
  | { kind: "pending" }
  | { kind: "running"; detail?: string }
  | { kind: "done"; seconds: number; detail: string; note?: string }
  | { kind: "failed"; seconds?: number; detail: string; note?: string }
  | { kind: "skipped" };

export type Section = "publish" | "verify";

export class Timeline {
  readonly element: HTMLElement;
  private started = new Map<string, number>();
  private ticker: number | undefined;

  constructor(private sections: Section[]) {
    this.element = h("div", { class: "timeline" });
    this.update([]);
  }

  /// `halted` marks a step still running as failed, for a job that stopped
  /// with an error between events.
  update(events: Event[], halted?: string): void {
    const states = new Map<string, State>();
    for (const event of events) {
      switch (event.event) {
        case "started":
          states.set(event.stage, { kind: "running" });
          break;
        case "finished":
          states.set(event.stage, { kind: "done", seconds: event.seconds, detail: event.detail });
          break;
        case "checking":
          states.set(event.check, { kind: "running", detail: event.intent });
          break;
        case "passed":
          states.set(event.check, { kind: "done", seconds: event.seconds, detail: event.outcome, note: event.intent });
          break;
        case "rejected":
          states.set(event.check, { kind: "failed", seconds: event.seconds, detail: event.reason, note: event.intent });
          break;
      }
    }
    if (halted) {
      for (const [key, state] of states) {
        if (state.kind === "running") states.set(key, { kind: "failed", detail: halted });
      }
    }
    const rejected = [...states.values()].some((state) => state.kind === "failed");

    const groups = this.sections.map((section) => {
      const rows = section === "publish" ? STAGES : CHECKS;
      let stopped = false;
      const items = rows.map((row) => {
        let state = states.get(row.key) ?? { kind: "pending" };
        if (stopped && state.kind === "pending") state = { kind: "skipped" };
        if (state.kind === "failed") stopped = true;
        if (state.kind === "pending" && rejected) state = { kind: "skipped" };
        return this.row(row, state);
      });
      return h(
        "section",
        { class: "timeline-group" },
        h("h3", {}, section === "publish" ? "On the laptop: prove and sign" : "As a reader: verify the file"),
        h("ol", {}, items),
      );
    });
    this.element.replaceChildren(...groups);

    const running = [...states.values()].some((state) => state.kind === "running");
    if (running && this.ticker === undefined) this.ticker = window.setInterval(() => this.tick(), 200);
    if (!running && this.ticker !== undefined) {
      window.clearInterval(this.ticker);
      this.ticker = undefined;
    }
  }

  private row(row: Row, state: State): HTMLElement {
    const mark =
      state.kind === "done"
        ? svg(icon.check)
        : state.kind === "failed"
          ? svg(icon.cross)
          : state.kind === "running"
            ? h("span", { class: "spinner" })
            : h("span", { class: "dot" });
    let seconds: HTMLElement | false = false;
    if (state.kind === "done" || (state.kind === "failed" && state.seconds !== undefined)) {
      seconds = h("span", { class: "seconds" }, `${state.seconds!.toFixed(1)} s`);
    } else if (state.kind === "running") {
      if (!this.started.has(row.key)) this.started.set(row.key, performance.now());
      seconds = h("span", { class: "seconds live", "data-key": row.key }, this.elapsed(row));
    }
    const detail =
      state.kind === "done" || state.kind === "failed"
        ? h("div", { class: "row-detail" }, state.detail)
        : state.kind === "running" && state.detail
          ? h("div", { class: "row-detail" }, state.detail)
          : false;
    return h(
      "li",
      { class: `row is-${state.kind}` },
      h("span", { class: "mark" }, mark),
      h(
        "div",
        { class: "row-body" },
        h("div", { class: "row-title" }, h("span", {}, row.title), seconds),
        h("div", { class: "row-what" }, row.what),
        detail,
        (state.kind === "done" || state.kind === "failed") && state.note
          ? h("div", { class: "row-note" }, state.note)
          : false,
        state.kind === "skipped" ? h("div", { class: "row-note" }, "Not run") : false,
      ),
    );
  }

  private elapsed(row: Row): string {
    const seconds = (performance.now() - (this.started.get(row.key) ?? performance.now())) / 1000;
    return row.typical ? `${seconds.toFixed(0)} s of ~${row.typical} s` : `${seconds.toFixed(0)} s`;
  }

  private tick(): void {
    for (const span of this.element.querySelectorAll<HTMLElement>(".seconds.live")) {
      const row = [...STAGES, ...CHECKS].find((each) => each.key === span.dataset.key);
      if (row) span.textContent = this.elapsed(row);
    }
  }
}
