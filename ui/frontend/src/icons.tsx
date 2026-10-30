import { ReactNode, SVGProps } from "react";

/** Minimal stroke icon set (lucide-style paths, no dependency). */
function make(path: ReactNode, extra?: SVGProps<SVGSVGElement>) {
  return function Icon() {
    return (
      <svg
        width={14}
        height={14}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        {...extra}
      >
        {path}
      </svg>
    );
  };
}

export const IconThumbsUp = make(
  <>
    <path d="M7 10v12" />
    <path d="M15 5.88 14 10h5.83a2 2 0 0 1 1.92 2.56l-2.33 8A2 2 0 0 1 17.5 22H4a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2h2.76a2 2 0 0 0 1.79-1.11L12 2a3.13 3.13 0 0 1 3 3.88Z" />
  </>,
);
export const IconCheck = make(<path d="M4 12.5l5.5 5.5L20 6.5" />);
export const IconUserCheck = make(
  <>
    <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
    <circle cx="9" cy="7" r="4" />
    <path d="m16 11 2 2 4-4" />
  </>,
);
export const IconCircleCheck = make(
  <>
    <circle cx="12" cy="12" r="10" />
    <path d="m9 12 2 2 4-4" />
  </>,
);
export const IconX = make(<path d="M6 6l12 12M18 6L6 18" />);
export const IconUndo = make(
  <>
    <path d="M9 14L4 9l5-5" />
    <path d="M4 9h10a6 6 0 0 1 0 12h-3" />
  </>,
);
export const IconMessage = make(
  <path d="M21 12a8 8 0 0 1-8 8H4l2.3-2.9A8 8 0 1 1 21 12z" />,
);
export const IconPlay = make(<path d="M7 4.5v15l13-7.5z" />);
export const IconPause = make(<path d="M8 5v14M16 5v14" />);
export const IconBot = make(
  <>
    <rect x="4" y="8" width="16" height="12" rx="3" />
    <path d="M12 8V4M9 14h.01M15 14h.01" />
  </>,
);
export const IconUnlock = make(
  <>
    <rect x="4" y="11" width="16" height="10" rx="2" />
    <path d="M8 11V7a4 4 0 0 1 7.9-.9" />
  </>,
);
export const IconStop = make(
  <>
    <circle cx="12" cy="12" r="9" />
    <rect x="9" y="9" width="6" height="6" rx="1" fill="currentColor" stroke="none" />
  </>,
);
export const IconTrash = make(
  <>
    <path d="M4 7h16M10 11v6M14 11v6" />
    <path d="M6 7l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13M9 7V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v3" />
  </>,
);
export const IconInfo = make(
  <>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5M12 8h.01" />
  </>,
);
export const IconExternal = make(
  <>
    <path d="M14 5h5v5M19 5l-8 8" />
    <path d="M19 14v4a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4" />
  </>,
);
export const IconGitBranch = make(
  <>
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="8" r="2.5" />
    <path d="M6 8.5v7M18 10.5a8 8 0 0 1-8 7" />
  </>,
);
export const IconGitMerge = make(
  <>
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="12" r="2.5" />
    <path d="M6 8.5v7M8.5 6.5c4 1 6 2.5 7 4" />
  </>,
);
export const IconGitPullRequest = make(
  <>
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="18" r="2.5" />
    <path d="M6 8.5v7M13 6h2a3 3 0 0 1 3 3v6.5M13 6l3-3M13 6l3 3" />
  </>,
);
export const IconFileText = make(
  <>
    <path d="M6 3h8l4 4v14H6z" />
    <path d="M14 3v4h4M9 12h6M9 16h6" />
  </>,
);
export const IconTerminalSquare = make(
  <>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M7 9l3.5 3L7 15M12.5 15H17" />
  </>,
);
export const IconSliders = make(
  <>
    <path d="M4 8h10M18 8h2M4 16h4M12 16h8" />
    <circle cx="16" cy="8" r="2" />
    <circle cx="10" cy="16" r="2" />
  </>,
);
export const IconFolder = make(
  <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />,
);
export const IconPencil = make(<path d="M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4z" />);
