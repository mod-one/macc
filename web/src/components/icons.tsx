import type { SVGProps } from 'react';

type IconProps = SVGProps<SVGSVGElement>;

function createIcon(path: string) {
  return function Icon(props: IconProps) {
    return (
      <svg
        aria-hidden="true"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="1.8"
        viewBox="0 0 24 24"
        {...props}
      >
        <path d={path} />
      </svg>
    );
  };
}

export const CheckIcon = createIcon('M5 12.5 9.5 17 19 7.5');
export const AlertTriangleIcon = createIcon('M12 4 20 19H4L12 4Zm0 5v4m0 3h.01');
export const XCircleIcon = createIcon('M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm-3-6 6-6m0 6-6-6');
export const ClockIcon = createIcon('M12 7v5l3 3m6-3a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z');
export const PauseIcon = createIcon('M9 7v10m6-10v10');
export const PlayIcon = createIcon('M8 7.5v9l8-4.5-8-4.5Z');
export const CopyIcon = createIcon('M9 9V5h10v10h-4m-6 4H5V9h10v10');
export const PinIcon = createIcon('m14 4 6 6m-3-3-5.5 5.5m-2 2L7 17l2.5-2.5m0 0L5 10l5-5 4.5 4.5Z');
export const SparklesIcon = createIcon('M12 3l1.7 4.3L18 9l-4.3 1.7L12 15l-1.7-4.3L6 9l4.3-1.7L12 3Z');
export const ArrowUpIcon = createIcon('m12 18 0-12m0 0-4 4m4-4 4 4');
export const ArrowDownIcon = createIcon('m12 6 0 12m0 0-4-4m4 4 4-4');
export const RefreshIcon = createIcon('M20 5v5h-5m4 7v-5h-5M6 19v-5h5M5 8a7 7 0 0 1 12-2l3 4M19 16a7 7 0 0 1-12 2l-3-4');
export const LogsIcon = createIcon('M7 7h10M7 12h10M7 17h6');
export const MinusIcon = createIcon('M6 12h12');
export const SearchIcon = createIcon('M21 21l-4.35-4.35M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z');
export const ChevronUpIcon = createIcon('m18 15-6-6-6 6');
export const ChevronDownIcon = createIcon('m6 9 6 6 6-6');
export const FolderOpenIcon = createIcon('M2 11V6a2 2 0 0 1 2-2h7l2 3h7a2 2 0 0 1 2 2v2M2 11v7a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-7M2 11h20');
export const XIcon = createIcon('M18 6 6 18M6 6l12 12');
export const ActivityIcon = createIcon('M22 12h-4l-3 9L9 3l-3 9H2');
export const DownloadIcon = createIcon('M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4m4-5 5 5 5-5m-5 5V3');
export const SwitchIcon = createIcon('m16 3 4 4-4 4m-6 10-4-4 4-4m-6 4h14m-4-10H2');
export const PlusIcon = createIcon('M12 5v14M5 12h14');
export const FolderIcon = createIcon('M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z');
export const TableIcon = createIcon('M3 3h18v18H3V3Zm0 6h18M3 15h18M9 3v18M15 3v18');
export const LayoutGridIcon = createIcon('M3 3h7v7H3V3Zm11 0h7v7h-7V3ZM3 14h7v7H3v-7Zm11 0h7v7h-7v-7Z');
export const BranchIcon = createIcon('M18 15V6a2 2 0 0 0-2-2H8M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm12 0a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm-12-6a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z');
export const TrashIcon = createIcon('M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2');
export const ArrowRightIcon = createIcon('m12 5 7 7-7 7M5 12h14');
export const BellIcon = createIcon('M6 8a6 6 0 1 1 12 0c0 7 3 9 3 9H3s3-2 3-9m3 13a3 3 0 0 0 6 0');
export const InfoIcon = createIcon('M12 16h.01M12 8h.01M12 12h.01M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Z');
export const CheckCircleIcon = createIcon('M9 12l2 2 4-4m6 2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z');
