// The toast: a short message over the status line that goes after a few seconds (`toast` in the
// store; errors from engine commands come here too).
import { useEffect, useState } from "react";
import { useStore } from "../state/store";

export function Toast() {
  const toast = useStore((s) => s.toast);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!toast) return;
    setVisible(true);
    const t = window.setTimeout(() => setVisible(false), toast.ms ?? 3500);
    return () => window.clearTimeout(t);
  }, [toast]);
  if (!toast || !visible) return null;
  return <div className="toast">{toast.text}</div>;
}
