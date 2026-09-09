import { useEffect, useRef, useState } from "react";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { Crosshair, Globe2, Info, Minus, Plus } from "lucide-react";
import { api } from "./useLab";
import { DEFAULT_LOCATION } from "./types";
import type { Location, Region } from "./types";

type Props = {
  location: Location;
  onChange: (value: Location) => void;
  onRegion: (value: Region | null) => void;
  locked: boolean;
  reader: boolean;
};

export function LocationPanel({
  location,
  onChange,
  onRegion,
  locked,
  reader,
}: Props) {
  const element = useRef<HTMLDivElement>(null);
  const map = useRef<L.Map | null>(null);
  const layers = useRef<L.LayerGroup | null>(null);
  const callback = useRef(onChange);
  const current = useRef({ location, locked, reader });
  const [region, setRegion] = useState<Region | null>(null);
  const [error, setError] = useState("");
  const [fields, setFields] = useState([
    String(location.latitude),
    String(location.longitude),
  ]);
  const [tilesFailed, setTilesFailed] = useState(false);
  const invalidCoordinates = fields.some(
    (value, index) =>
      !value.trim() ||
      !Number.isFinite(Number(value)) ||
      Math.abs(Number(value)) > (index === 0 ? 90 : 180),
  );
  callback.current = onChange;
  current.current = { location, locked, reader };

  useEffect(() => {
    if (!element.current) return;
    const instance = L.map(element.current, {
      zoomControl: false,
      attributionControl: true,
    }).setView([location.latitude, location.longitude], 13);
    map.current = instance;
    const tiles = L.tileLayer(
      "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
      {
        maxZoom: 19,
        attribution:
          '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
      },
    ).addTo(instance);
    tiles.on("tileerror", () => setTilesFailed(true));
    tiles.on("load", () => setTilesFailed(false));
    layers.current = L.layerGroup().addTo(instance);
    instance.on("click", (event) => {
      const state = current.current;
      if (!state.locked && !state.reader)
        callback.current({
          ...state.location,
          latitude: +event.latlng.lat.toFixed(6),
          longitude: +event.latlng.wrap().lng.toFixed(6),
          cell: undefined,
        });
    });
    const observer = new ResizeObserver(() => instance.invalidateSize());
    observer.observe(element.current);
    return () => {
      observer.disconnect();
      instance.remove();
      map.current = null;
    };
    // The map is created once; click handlers read the current selection from refs.
  }, []);

  useEffect(() => {
    setFields([String(location.latitude), String(location.longitude)]);
  }, [location.latitude, location.longitude]);
  useEffect(() => {
    const controller = new AbortController();
    onRegion(null);
    if (invalidCoordinates && !reader) {
      setRegion(null);
      return;
    }
    const timer = setTimeout(() => {
      api<Region>("/regions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(location),
        signal: controller.signal,
      })
        .then((value) => {
          setRegion(value);
          onRegion(value);
          setError("");
        })
        .catch((err) => {
          if (err.name !== "AbortError") {
            setError(err.message);
            setRegion(null);
          }
        });
    }, 250);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [
    location.latitude,
    location.longitude,
    location.resolution,
    location.cell,
    invalidCoordinates,
    reader,
    onRegion,
  ]);

  useEffect(() => {
    const instance = map.current,
      group = layers.current;
    if (!instance || !group) return;
    group.clearLayers();
    if (region) {
      const polygon = L.polygon(region.boundary, {
        color: region.supported ? "#ed5e28" : "#a3362d",
        weight: 2,
        fillColor: "#f77946",
        fillOpacity: 0.17,
      }).addTo(group);
      instance.fitBounds(polygon.getBounds().pad(0.6), {
        animate: false,
        maxZoom: 15,
      });
    }
    if (!reader) {
      L.circleMarker([location.latitude, location.longitude], {
        radius: 5,
        color: "#242724",
        fillColor: "#242724",
        fillOpacity: 1,
        weight: 3,
      }).addTo(group);
      L.circleMarker([location.latitude, location.longitude], {
        radius: 11,
        color: "#242724",
        fillOpacity: 0,
        weight: 1,
        dashArray: "2,3",
      }).addTo(group);
    }
  }, [region, reader, location.latitude, location.longitude]);

  const setField = (index: number, text: string) => {
    setFields((old) => old.map((value, i) => (i === index ? text : value)));
    const number = Number(text);
    if (
      text.trim() &&
      Number.isFinite(number) &&
      Math.abs(number) <= (index === 0 ? 90 : 180)
    ) {
      onChange({
        ...location,
        [index === 0 ? "latitude" : "longitude"]: number,
        cell: undefined,
      });
    }
  };

  return (
    <section className="location-panel">
      <div className="panel-heading">
        <span>
          <Globe2 size={15} /> {reader ? "Public region" : "Simulated location"}
        </span>
        <span className="micro">02 / GEOGRAPHY</span>
      </div>
      <div className="map-wrap">
        <div
          className="map"
          ref={element}
          aria-label="Interactive capture location map"
        />
        <div className="map-caption">
          <span className="orange-square" />
          {reader
            ? "REGION ONLY"
            : locked
              ? "CAPTURE FROZEN"
              : "CLICK TO PLACE CAPTURE"}
        </div>
        <div className="map-controls">
          <button aria-label="Zoom in" onClick={() => map.current?.zoomIn()}>
            <Plus size={15} />
          </button>
          <button aria-label="Zoom out" onClick={() => map.current?.zoomOut()}>
            <Minus size={15} />
          </button>
        </div>
        {tilesFailed && (
          <div className="map-fallback">
            Basemap unavailable. The H3 region and coordinates remain usable.
          </div>
        )}
      </div>
      {!reader && (
        <>
          <div className="coordinate-fields">
            {["Latitude", "Longitude"].map((label, i) => (
              <label key={label}>
                <span>{label}</span>
                <input
                  aria-label={label}
                  inputMode="decimal"
                  aria-invalid={invalidCoordinates}
                  value={fields[i]}
                  onChange={(e) => setField(i, e.target.value)}
                  disabled={locked}
                />
              </label>
            ))}
          </div>
          <div className="preset-row">
            <span className="micro">JUMP TO</span>
            {[
              ["UTDT", -34.5478, -58.4462],
              ["San Francisco", 37.7749, -122.4194],
              ["Tokyo", 35.6762, 139.6503],
            ].map(([name, latitude, longitude]) => (
              <button
                key={name}
                disabled={locked}
                onClick={() => {
                  setFields([String(latitude), String(longitude)]);
                  onChange({
                    ...DEFAULT_LOCATION,
                    latitude: Number(latitude),
                    longitude: Number(longitude),
                    resolution: location.resolution,
                  });
                }}
              >
                {name}
              </button>
            ))}
          </div>
        </>
      )}
      <div className="resolution-row">
        <label htmlFor="resolution">
          Region resolution{" "}
          <b>{location.resolution.toString().padStart(2, "0")}</b>
        </label>
        <input
          id="resolution"
          type="range"
          min="0"
          max="15"
          value={location.resolution}
          disabled={locked || reader}
          onChange={(e) =>
            onChange({
              ...location,
              resolution: Number(e.target.value),
              cell: undefined,
            })
          }
        />
        <div className="range-labels">
          <span>Larger region</span>
          <span>Smaller region</span>
        </div>
      </div>
      {invalidCoordinates && !reader && (
        <p className="inline-error" role="alert">
          Enter latitude from −90 to 90 and longitude from −180 to 180 before
          starting.
        </p>
      )}
      <div className="region-summary">
        <Crosshair size={17} />
        <div>
          <span className="micro">H3 CELL</span>
          <code>{region?.cell || "Calculating…"}</code>
        </div>
        <span>
          {region
            ? `${region.area_km2 < 0.01 ? region.area_km2.toFixed(6) : region.area_km2.toFixed(2)} km²`
            : "—"}
        </span>
      </div>
      {(error || (region && (!region.supported || !region.contains_point))) && (
        <p className="inline-error" role="alert">
          {error ||
            region?.reason ||
            "This region does not contain the selected point."}
        </p>
      )}
      <details className="region-help">
        <summary>
          <Info size={13} /> Why are some regions unavailable?
        </summary>
        <p>
          This PoC excludes H3 pentagons and cells crossing an icosahedron face
          edge. It also checks that the cell center remains consistent after
          32-bit coordinate conversion. These are implementation limits, not
          limits of H3 itself.
        </p>
        <p>
          The location circuit also has an unresolved constraint gap. A passing
          demo check does not yet establish security against a modified prover.
        </p>
      </details>
    </section>
  );
}
