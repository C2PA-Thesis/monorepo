// The H3 cell on an OpenStreetMap base map. A reader's map shows the cell
// only; the capture page also shows the phone its own position.

import L from "leaflet";
import "leaflet/dist/leaflet.css";
import * as api from "./api";
import { area } from "./dom";

export class CellMap {
  readonly element: HTMLElement;
  private map: L.Map | undefined;
  private cell: L.Polygon | undefined;
  private position: L.CircleMarker | undefined;
  private shown: string | undefined;

  constructor() {
    this.element = document.createElement("div");
    this.element.className = "map";
  }

  /// Draws `cell` and returns its outline. Later calls for the same cell do nothing.
  async show(cell: string, at?: { latitude: number; longitude: number }): Promise<api.Outline | undefined> {
    const map = this.ensure();
    if (at) {
      const point: L.LatLngExpression = [at.latitude, at.longitude];
      this.position ??= L.circleMarker(point, { radius: 6, className: "you-dot" })
        .bindTooltip("Your position, private")
        .addTo(map);
      this.position.setLatLng(point);
    }
    if (this.shown === cell) return undefined;
    this.shown = cell;
    const outline = await api.outline(cell);
    if (this.shown !== cell) return undefined;
    this.cell?.remove();
    // Stroke and fill come from the stylesheet's tokens, so the cell follows the theme.
    this.cell = L.polygon(outline.boundary, { className: "cell-shape" })
      .bindTooltip(`${area(outline.area_km2)}, published`)
      .addTo(map);
    map.invalidateSize();
    map.fitBounds(this.cell.getBounds(), { padding: [24, 24], maxZoom: 17 });
    return outline;
  }

  /// Leaflet measures its container once; call after the map becomes visible.
  resize(): void {
    this.map?.invalidateSize();
    if (this.cell) this.map?.fitBounds(this.cell.getBounds(), { padding: [24, 24], maxZoom: 17 });
  }

  private ensure(): L.Map {
    if (this.map) return this.map;
    this.map = L.map(this.element, { zoomControl: false, attributionControl: true, scrollWheelZoom: false });
    this.map.attributionControl.setPrefix(false);
    L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
      maxZoom: 19,
      attribution: '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
    }).addTo(this.map);
    this.map.setView([0, 0], 2);
    return this.map;
  }
}
