// The publish stages and the reader checks as a live list: a mark, a title
// and a time per step. A group that fully passed folds into one row; what
// each step establishes and what it found sit behind the row. A refusal's
// sentence is the verdict's; the server's words sit behind the failed row. Built from the events `provenance serve` reports.

import type { Event as Progress } from "./api";
import { h, icon, svg } from "./dom";
import { tidy } from "./verdict";

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
    what: "Groth16: the hints sealed in the envelope map to the cell.",
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
  { key: "location", title: "Location proof", what: "The hints sealed in the signed envelope map to the claimed cell." },
];

type State =
  | { kind: "pending" }
  | { kind: "running"; detail?: string }
  | { kind: "done"; seconds: number; detail: string }
  | { kind: "failed"; seconds?: number; detail: string }
  | { kind: "skipped" };

export type Section = "publish" | "verify";

export class Timeline {
  readonly element: HTMLElement;
  private started = new Map<string, number>();
  private open = new Set<string>();
  private ticker: number | undefined;

  constructor(private sections: Section[]) {
    this.element = h("div", { class: "timeline" });
    this.update([]);
  }

  /// `halted` marks a step still running as failed, for a job that stopped
  /// with an error between events.
  update(events: Progress[], halted?: string): void {
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
          states.set(event.check, { kind: "done", seconds: event.seconds, detail: event.outcome });
          break;
        case "rejected":
          states.set(event.check, { kind: "failed", seconds: event.seconds, detail: event.reason });
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
      const title = section === "publish" ? "Publish" : "Verify";
      let stopped = false;
      const resolved = rows.map((row): [Row, State] => {
        let state = states.get(row.key) ?? { kind: "pending" };
        if (stopped && state.kind === "pending") state = { kind: "skipped" };
        if (state.kind === "failed") stopped = true;
        if (state.kind === "pending" && rejected) state = { kind: "skipped" };
        return [row, state];
      });
      const items = resolved.map(([row, state]) => this.row(`${section}:${row.key}`, row, state));
      const settled =
        resolved.every(([, state]) => state.kind === "pending" || state.kind === "skipped") ||
        resolved.every(([, state]) => state.kind === "done");
      if (settled) this.started.delete(`group:${section}`);
      if (resolved.every(([, state]) => state.kind === "pending" || state.kind === "skipped")) {
        const dot = h("span", { class: "mark" }, h("span", { class: "dot" }));
        return h("div", { class: "timeline-group is-pending" }, h("div", { class: "row-line" }, dot, h("span", { class: "row-title" }, title)));
      }
      if (resolved.every(([, state]) => state.kind === "done")) {
        const total = resolved.reduce((sum, [, state]) => sum + (state.kind === "done" ? state.seconds : 0), 0);
        return h(
          "details",
          {
            class: "timeline-group row group is-done",
            open: this.open.has(section),
            ontoggle: (event: Event) => {
              if ((event.target as HTMLDetailsElement).open) this.open.add(section);
              else this.open.delete(section);
            },
          },
          h(
            "summary",
            {},
            h("span", { class: "mark" }, svg(icon.check)),
            h("span", { class: "row-title" }, title),
            h("span", { class: "seconds" }, seconds(total)),
          ),
          h("ol", {}, items),
        );
      }
      // Running or stopped: the same row as a finished group, its steps open under it.
      const failed = resolved.some(([, state]) => state.kind === "failed");
      const key = `group:${section}`;
      if (!this.started.has(key)) this.started.set(key, performance.now());
      const head = h(
        "div",
        { class: "row-line" },
        failed ? h("span", { class: "mark bad" }, svg(icon.cross)) : h("span", { class: "mark" }, h("span", { class: "spinner" })),
        h("span", { class: "row-title" }, title),
        !failed && h("span", { class: "seconds live", "data-key": key }, this.elapsed(key)),
      );
      return h("div", { class: "timeline-group group" }, head, h("ol", {}, items));
    });
    this.element.replaceChildren(...groups);

    const running = [...states.values()].some((state) => state.kind === "running");
    if (running && this.ticker === undefined) this.ticker = window.setInterval(() => this.tick(), 200);
    if (!running && this.ticker !== undefined) {
      window.clearInterval(this.ticker);
      this.ticker = undefined;
    }
  }

  private row(id: string, row: Row, state: State): HTMLElement {
    const mark =
      state.kind === "done"
        ? svg(icon.check)
        : state.kind === "failed"
          ? svg(icon.cross)
          : state.kind === "running"
            ? h("span", { class: "spinner" })
            : h("span", { class: "dot" });
    let time: HTMLElement | false = false;
    // Steps under a tenth of a second show no time: a column of "0.0 s" is noise.
    if ((state.kind === "done" || state.kind === "failed") && state.seconds !== undefined && state.seconds >= 0.05) {
      time = h("span", { class: "seconds" }, seconds(state.seconds));
    } else if (state.kind === "running") {
      if (!this.started.has(row.key)) this.started.set(row.key, performance.now());
      time = h("span", { class: "seconds live", "data-key": row.key }, this.elapsed(row.key, row.typical));
    }
    const line: (HTMLElement | false)[] = [h("span", { class: "mark" }, mark), h("span", { class: "row-title" }, row.title), time];
    if (state.kind !== "done" && state.kind !== "failed") {
      return h("li", { class: `is-${state.kind}` }, h("div", { class: "row-line" }, line));
    }
    const more = h(
      "details",
      {
        class: "row",
        open: this.open.has(id),
        ontoggle: (event: Event) => {
          if ((event.target as HTMLDetailsElement).open) this.open.add(id);
          else this.open.delete(id);
        },
      },
      h("summary", {}, line),
      h(
        "div",
        { class: "row-more" },
        h("span", {}, row.what),
        state.kind === "done"
          ? h("span", { class: "mono outcome-value" }, outcome(state.detail))
          : h("span", { class: "error-text" }, tidy(state.detail)),
      ),
    );
    return h("li", { class: `is-${state.kind}` }, more);
  }

  private elapsed(key: string, typical?: number): string {
    const seconds = (performance.now() - (this.started.get(key) ?? performance.now())) / 1000;
    return typical ? `${seconds.toFixed(0)} / ~${typical} s` : `${seconds.toFixed(0)} s`;
  }

  private tick(): void {
    for (const span of this.element.querySelectorAll<HTMLElement>(".seconds.live")) {
      const key = span.dataset.key!;
      span.textContent = this.elapsed(key, [...STAGES, ...CHECKS].find((each) => each.key === key)?.typical);
    }
  }
}

const seconds = (value: number) => `${value.toFixed(1)} s`;

/// What a step found, as a value: "kept 512x512 at (0, 0)" reads
/// "512×512 at (0, 0)" and "proof accepted" reads "Accepted".
export function outcome(text: string): string {
  const value = text
    .trim()
    .replace(/^(?:kept|proof) (?=\S)/i, "")
    .replace(/\.$/, "")
    .replace(/\b([1-9]\d*)x([1-9]\d*)\b/g, "$1×$2");
  return value ? `${value[0]!.toUpperCase()}${value.slice(1)}` : value;
}
