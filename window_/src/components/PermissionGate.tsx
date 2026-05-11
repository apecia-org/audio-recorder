import { relaunch } from "@tauri-apps/plugin-process";
import { api } from "../api";

interface Props {
  onRetry: () => void;
}

export default function PermissionGate({ onRetry }: Props) {
  return (
    <div className="permission-gate">
      <h2>Screen Recording permission required</h2>
      <p>
        macOS requires Screen Recording permission to capture per-application
        audio via ScreenCaptureKit. No video is saved — only the audio of the
        app you choose.
      </p>
      <ol>
        <li>Click <strong>Open Privacy Settings</strong> below.</li>
        <li>Enable <em>Audio Recorder</em> under Screen Recording.</li>
        <li>Relaunch the app to apply the permission.</li>
      </ol>
      <div className="permission-buttons">
        <button onClick={() => api.openSystemSettings("screenrecording")}>
          Open Privacy Settings
        </button>
        <button onClick={() => relaunch()}>Relaunch app</button>
        <button onClick={onRetry}>I’ve granted it</button>
      </div>
    </div>
  );
}
