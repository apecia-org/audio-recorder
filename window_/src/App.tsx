import { useEffect, useState } from "react";
import { api, onAudioLevel, onRecordingError, onRecordingWarning } from "./api";
import { useRecorder } from "./store";
import ProcessList from "./components/ProcessList";
import RecordControls from "./components/RecordControls";
import VuMeter from "./components/VuMeter";
import PermissionGate from "./components/PermissionGate";

export default function App() {
  const {
    setProcesses,
    setStatus,
    setLevel,
    setError,
    setWarning,
    setDefaultDir,
    errorBanner,
    warningBanner,
  } = useRecorder();
  const [permissionOk, setPermissionOk] = useState<boolean | null>(null);

  useEffect(() => {
    const init = async () => {
      try {
        const perms = await api.checkPermissions();
        setPermissionOk(perms.screen_recording);
      } catch {
        setPermissionOk(true);
      }
      try {
        setDefaultDir(await api.defaultRecordingsDir());
      } catch {
        // non-fatal
      }
      try {
        setStatus(await api.getRecordingState());
      } catch {
        // non-fatal
      }
      try {
        setProcesses(await api.listProcesses());
      } catch (e) {
        setError(`Failed to list processes: ${e}`);
      }
    };
    void init();
  }, [setProcesses, setStatus, setError, setDefaultDir]);

  useEffect(() => {
    let unlistenLevel: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let unlistenWarning: (() => void) | undefined;
    onAudioLevel(setLevel).then((u) => (unlistenLevel = u));
    onRecordingError((m) => setError(m)).then((u) => (unlistenError = u));
    onRecordingWarning((m) => setWarning(m)).then((u) => (unlistenWarning = u));
    return () => {
      unlistenLevel?.();
      unlistenError?.();
      unlistenWarning?.();
    };
  }, [setLevel, setError, setWarning]);

  if (permissionOk === false) {
    return <PermissionGate onRetry={async () => {
      const p = await api.checkPermissions();
      setPermissionOk(p.screen_recording);
    }} />;
  }

  return (
    <div className="app">
      <header className="app-header">
        <h1>Audio Recorder</h1>
        <p className="subtitle">Record audio from one app at a time. Saved as MP3.</p>
      </header>

      {errorBanner && (
        <div className="banner banner-error">
          <span>{errorBanner}</span>
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}
      {warningBanner && (
        <div className="banner banner-warning">
          <span>{warningBanner}</span>
          <button onClick={() => setWarning(null)}>×</button>
        </div>
      )}

      <ProcessList />
      <VuMeter />
      <RecordControls />
    </div>
  );
}
