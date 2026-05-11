import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export type Category = "browser" | "meetingapp" | "other";

export interface ProcessInfo {
  pid: number;
  display_name: string;
  exe_basename: string;
  bundle_id: string | null;
  icon_b64: string | null;
  category: Category;
  is_top_level: boolean;
}

export type RecordingStatus =
  | { kind: "idle" }
  | {
      kind: "recording";
      pid: number;
      source_app: string;
      output_path: string;
      started_at_iso: string;
      mic_mix: boolean;
    };

export interface RecordingMeta {
  output_path: string;
  duration_seconds: number;
  source_app: string;
}

export interface PermissionStatus {
  screen_recording: boolean;
  microphone: boolean;
}

export const api = {
  listProcesses: () => invoke<ProcessInfo[]>("list_recordable_processes"),

  startRecording: (args: {
    pid: number;
    mic_mix: boolean;
    output_path?: string;
    source_app?: string;
  }) => invoke<RecordingStatus>("start_recording", { args }),

  stopRecording: () => invoke<RecordingMeta>("stop_recording"),

  getRecordingState: () => invoke<RecordingStatus>("get_recording_state"),

  checkPermissions: () => invoke<PermissionStatus>("check_permissions"),

  openSystemSettings: (pane: "screenrecording" | "microphone") =>
    invoke<void>("open_system_settings", { pane }),

  defaultRecordingsDir: () => invoke<string>("default_recordings_dir"),
};

export function onAudioLevel(handler: (level: number) => void): Promise<UnlistenFn> {
  return listen<number>("audio-level", (event) => handler(event.payload));
}

export function onRecordingError(handler: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("recording-error", (event) => handler(event.payload));
}

export function onRecordingWarning(handler: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("recording-warning", (event) => handler(event.payload));
}
