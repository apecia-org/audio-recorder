import { useEffect, useState } from "react";
import { useRecorder } from "../store";

export default function VuMeter() {
  const level = useRecorder((s) => s.level);
  const [smoothed, setSmoothed] = useState(0);

  useEffect(() => {
    const id = window.setInterval(() => {
      setSmoothed((prev) => {
        const target = level;
        if (target > prev) return target;
        return prev * 0.9;
      });
    }, 33);
    return () => window.clearInterval(id);
  }, [level]);

  const dbfs = smoothed > 0 ? 20 * Math.log10(smoothed) : -Infinity;
  const pct = Math.max(0, Math.min(100, ((dbfs + 60) / 60) * 100));

  return (
    <section className="vu-meter">
      <div className="vu-bar" style={{ width: `${pct}%` }} />
      <span className="vu-label">
        {Number.isFinite(dbfs) ? `${dbfs.toFixed(0)} dBFS` : "-∞ dBFS"}
      </span>
    </section>
  );
}
