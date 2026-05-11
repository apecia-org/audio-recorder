import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useRecorder } from "../store";

export default function RecordControls() {
  const {
    selectedPid,
    micMix,
    status,
    defaultDir,
    outputOverride,
    setMicMix,
    setStatus,
    setOutputOverride,
    setError,
    processes,
  } = useRecorder();
  const [elapsed, setElapsed] = useState(0);

  const isRecording = status.kind === "recording";

  useEffect(() => {
    if (!isRecording) {
      setElapsed(0);
      return;
    }
    const start = new Date(status.started_at_iso).getTime();
    const tick = () => setElapsed(Math.max(0, (Date.now() - start) / 1000));
    tick();
    const id = window.setInterval(tick, 250);
    return () => window.clearInterval(id);
  }, [isRecording, status]);

  const startStop = async () => {
    if (isRecording) {
      try {
        const meta = await api.stopRecording();
        setStatus({ kind: "idle" });
        setOutputOverride(null);
        // eslint-disable-next-line no-console
        console.info(`Saved ${meta.output_path} (${meta.duration_seconds.toFixed(1)}s)`);
      } catch (e) {
        setError(`Stop failed: ${e}`);
      }
      return;
    }
    if (selectedPid == null) {
      setError("Pick an app to record first.");
      return;
    }
    const proc = processes.find((p) => p.pid === selectedPid);
    try {
      const result = await api.startRecording({
        pid: selectedPid,
        mic_mix: micMix,
        output_path: outputOverride ?? undefined,
        source_app: proc?.display_name ?? proc?.exe_basename,
      });
      setStatus(result);
    } catch (e) {
      setError(`Start failed: ${e}`);
    }
  };

  const chooseSavePath = async () => {
    const chosen = await save({
      title: "Save recording as",
      defaultPath: defaultDir || undefined,
      filters: [{ name: "MP3 audio", extensions: ["mp3"] }],
    });
    if (chosen) setOutputOverride(chosen);
  };

  return (
    <section className="controls">
      <div className="control-row">
        <label className="mic-toggle">
          <input
            type="checkbox"
            checked={micMix}
            onChange={(e) => setMicMix(e.target.checked)}
            disabled={isRecording}
          />
          Mix microphone
        </label>
        <button className="save-as" onClick={chooseSavePath} disabled={isRecording}>
          Save to…
        </button>
      </div>
      <p className="output-path">
        {outputOverride
          ? truncate(outputOverride)
          : defaultDir
            ? `Default: ${truncate(defaultDir)}/`
            : "Default folder…"}
      </p>
      <button className={`record-button ${isRecording ? "stop" : "start"}`} onClick={startStop}>
        {isRecording ? `Stop (${formatTime(elapsed)})` : "Record"}
      </button>
    </section>
  );
}

function truncate(p: string, max = 56): string {
  if (p.length <= max) return p;
  return "…" + p.slice(p.length - max + 1);
}

function formatTime(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}
