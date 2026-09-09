import { useCallback, useEffect, useRef, useState } from "react";
import type { Experiment, LabEvent, Location, Reader } from "./types";

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api${path}`, init);
  if (!response.ok) {
    const data = await response
      .json()
      .catch(() => ({ detail: "The lab could not complete this request." }));
    throw new Error(
      typeof data.detail === "string"
        ? data.detail
        : "Check the selected inputs and try again.",
    );
  }
  return response.json();
}

export function useLab(readerMode = false) {
  const [run, setRun] = useState<Experiment | null>(null);
  const [reader, setReader] = useState<Reader | null>(null);
  const [events, setEvents] = useState<LabEvent[]>([]);
  const [history, setHistory] = useState<Experiment[]>([]);
  const [error, setError] = useState("");
  const [working, setWorking] = useState(false);
  const [connected, setConnected] = useState(true);
  const [ready, setReady] = useState(false);
  const selected = useRef<string | null>(null);

  const select = useCallback((item: Experiment) => {
    selected.current = item.id;
    setRun(item);
    setReader(null);
    setEvents([]);
    setError("");
    localStorage.setItem("provenance-lab:v1:run", item.id);
  }, []);

  useEffect(() => {
    let alive = true;
    Promise.all([
      api<{ ready: boolean; error?: string }>("/health"),
      api<Experiment[]>("/runs"),
    ])
      .then(([health, items]) => {
        if (!alive) return;
        setReady(health.ready);
        if (health.error) setError(health.error);
        setHistory(items);
        const saved = items.find(
          (item) => item.id === localStorage.getItem("provenance-lab:v1:run"),
        );
        if (saved) select(saved);
      })
      .catch((err) => {
        if (alive) setError(err.message);
      });
    return () => {
      alive = false;
    };
  }, [select]);

  useEffect(() => {
    if (!run?.id) return;
    const id = run.id;
    let alive = true;
    let refreshTimer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        if (readerMode) {
          const evidence = await api<Reader>(`/runs/${id}/reader`);
          if (alive && selected.current === id) {
            setReader(evidence);
            setRun((previous) =>
              previous
                ? {
                    ...previous,
                    status: evidence.status,
                    activity: evidence.activity,
                    started_at: evidence.started_at,
                    error: evidence.error,
                    stages: evidence.stages,
                    attacks: evidence.attacks,
                  }
                : previous,
            );
          }
          return;
        }
        const [next, evidence] = await Promise.all([
          api<Experiment>(`/runs/${id}`),
          api<Reader>(`/runs/${id}/reader`),
        ]);
        if (alive && selected.current === id) {
          setRun(next);
          setReader(evidence);
          setHistory((items) =>
            [next, ...items.filter((item) => item.id !== id)].slice(0, 30),
          );
        }
      } catch (err) {
        if (alive) setError((err as Error).message);
      }
    };
    void refresh();
    if (readerMode) setEvents([]);
    const source = readerMode
      ? null
      : new EventSource(`/api/runs/${id}/events`);
    if (source) source.onopen = () => setConnected(true);
    if (source) source.onerror = () => setConnected(false);
    if (source)
      source.onmessage = (event) => {
        const value: LabEvent = JSON.parse(event.data);
        setEvents((items) =>
          items.some((item) => item.id === value.id)
            ? items
            : [...items, value].slice(-250),
        );
        if (value.type !== "message") {
          clearTimeout(refreshTimer);
          refreshTimer = setTimeout(() => void refresh(), 120);
        }
      };
    const fallback = setInterval(() => void refresh(), 5000);
    return () => {
      alive = false;
      source?.close();
      clearInterval(fallback);
      clearTimeout(refreshTimer);
    };
  }, [run?.id, readerMode]);

  const perform = async (operation: () => Promise<void>) => {
    setWorking(true);
    setError("");
    try {
      await operation();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(false);
    }
  };
  const upload = (file?: File) =>
    perform(async () => {
      const body = new FormData();
      if (file) body.append("file", file);
      const item = await api<Experiment>(file ? "/runs" : "/runs/sample", {
        method: "POST",
        ...(file ? { body } : {}),
      });
      select(item);
    });
  const start = (location: Location) =>
    perform(async () => {
      if (!run) return;
      setRun(
        await api<Experiment>(`/runs/${run.id}/start`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(location),
        }),
      );
    });
  const action = (name: string) =>
    perform(async () => {
      if (run) {
        const result = await api<Pick<Experiment, "status" | "activity">>(
          `/runs/${run.id}/actions/${name}`,
          { method: "POST" },
        );
        setRun((previous) =>
          previous ? { ...previous, ...result } : previous,
        );
      }
    });
  const cancel = () =>
    perform(async () => {
      if (run) await api(`/runs/${run.id}/cancel`, { method: "POST" });
    });
  return {
    run,
    reader,
    events,
    history,
    error,
    working,
    connected,
    ready,
    select,
    upload,
    start,
    action,
    cancel,
  };
}
