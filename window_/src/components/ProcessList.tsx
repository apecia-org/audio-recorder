import { useMemo } from "react";
import { api, type Category, type ProcessInfo } from "../api";
import { useRecorder } from "../store";

const badgeFor: Record<Category, string> = {
  browser: "Browser",
  meetingapp: "Meeting",
  other: "App",
};

export default function ProcessList() {
  const { processes, selectedPid, search, setSelectedPid, setSearch, setProcesses, setError } =
    useRecorder();

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return processes;
    return processes.filter(
      (p) =>
        p.display_name.toLowerCase().includes(q) ||
        p.exe_basename.toLowerCase().includes(q)
    );
  }, [processes, search]);

  const refresh = async () => {
    try {
      setProcesses(await api.listProcesses());
    } catch (e) {
      setError(`Refresh failed: ${e}`);
    }
  };

  return (
    <section className="process-list">
      <div className="process-list-header">
        <input
          className="search"
          placeholder="Search apps…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <button className="refresh" onClick={refresh}>
          Refresh
        </button>
      </div>
      <ul>
        {filtered.map((p: ProcessInfo) => (
          <li
            key={p.pid}
            className={selectedPid === p.pid ? "selected" : ""}
            onClick={() => setSelectedPid(p.pid)}
          >
            {p.icon_b64 ? (
              <img
                className="proc-icon"
                src={`data:image/png;base64,${p.icon_b64}`}
                alt=""
              />
            ) : (
              <div className="proc-icon-placeholder">{p.display_name.charAt(0)}</div>
            )}
            <div className="proc-meta">
              <span className="proc-name">{p.display_name}</span>
              <span className="proc-exe">{p.exe_basename}</span>
            </div>
            <span className={`badge badge-${p.category}`}>{badgeFor[p.category]}</span>
          </li>
        ))}
        {filtered.length === 0 && (
          <li className="empty">No processes match.</li>
        )}
      </ul>
    </section>
  );
}
