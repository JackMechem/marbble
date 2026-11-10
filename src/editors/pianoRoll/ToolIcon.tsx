// The Piano Roll tools' icons. The Tracks area's tool pop-ups use the ones they share.
import type { Tool } from "./model";

/** A tool's icon (the Tracks area's tool pop-ups use the ones they share). */
export const ToolIcon = ({ tool }: { tool: Tool }) => {
  const p = { width: 14, height: 14, viewBox: "0 0 14 14", fill: "none", stroke: "currentColor", strokeWidth: 1.3, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (tool) {
    case "pointer":
      return (
        <svg {...p}>
          <path d="M3.5 1.8v9.4l2.4-2.2 1.7 3.6 1.5-.7-1.7-3.5h3.2z" />
        </svg>
      );
    case "pencil":
      return (
        <svg {...p}>
          <path d="M2.5 11.5l.6-2.6 6.8-6.8 2 2-6.8 6.8zM8.8 3.2l2 2" />
        </svg>
      );
    case "eraser":
      return (
        <svg {...p}>
          <path d="M5.2 11.8 2 8.6l6-6 4.4 4.4-4.8 4.8zM4.6 6 9 10.4M5.2 11.8H12" />
        </svg>
      );
    case "velocity":
      return (
        <svg {...p}>
          <path d="M3 12V8M6 12V4.5M9 12V6.5M12 12V2.5" />
        </svg>
      );
    case "mute":
      return (
        <svg {...p}>
          <rect x="2" y="5" width="10" height="4" rx="1" />
          <path d="M2.5 12 11.5 2" />
        </svg>
      );
    case "scissors":
      return (
        <svg {...p}>
          <circle cx="3.6" cy="10.4" r="1.7" />
          <circle cx="10.4" cy="10.4" r="1.7" />
          <path d="M4.8 9.2 10.5 2M9.2 9.2 3.5 2" />
        </svg>
      );
    case "glue":
      return (
        <svg {...p}>
          <path d="M5 2.2h4l-.6 3.3h-2.8zM5.6 5.5 4.4 11.8h5.2L8.4 5.5" />
        </svg>
      );
    case "line":
      return (
        <svg {...p}>
          <path d="M2 11.5 12 2.5" />
          <circle cx="2.5" cy="11" r="1.2" fill="currentColor" />
          <circle cx="11.5" cy="3" r="1.2" fill="currentColor" />
        </svg>
      );
  }
};
