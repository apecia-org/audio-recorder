import { create } from "zustand";
import type { ProcessInfo, RecordingStatus } from "./api";

interface RecorderStore {
  processes: ProcessInfo[];
  selectedPid: number | null;
  search: string;
  micMix: boolean;
  status: RecordingStatus;
  level: number;
  errorBanner: string | null;
  warningBanner: string | null;
  defaultDir: string;
  outputOverride: string | null;

  setProcesses: (p: ProcessInfo[]) => void;
  setSelectedPid: (pid: number | null) => void;
  setSearch: (s: string) => void;
  setMicMix: (b: boolean) => void;
  setStatus: (s: RecordingStatus) => void;
  setLevel: (l: number) => void;
  setError: (e: string | null) => void;
  setWarning: (w: string | null) => void;
  setDefaultDir: (d: string) => void;
  setOutputOverride: (p: string | null) => void;
}

export const useRecorder = create<RecorderStore>((set) => ({
  processes: [],
  selectedPid: null,
  search: "",
  micMix: false,
  status: { kind: "idle" },
  level: 0,
  errorBanner: null,
  warningBanner: null,
  defaultDir: "",
  outputOverride: null,

  setProcesses: (p) => set({ processes: p }),
  setSelectedPid: (pid) => set({ selectedPid: pid }),
  setSearch: (s) => set({ search: s }),
  setMicMix: (b) => set({ micMix: b }),
  setStatus: (s) => set({ status: s }),
  setLevel: (l) => set({ level: l }),
  setError: (e) => set({ errorBanner: e }),
  setWarning: (w) => set({ warningBanner: w }),
  setDefaultDir: (d) => set({ defaultDir: d }),
  setOutputOverride: (p) => set({ outputOverride: p }),
}));
