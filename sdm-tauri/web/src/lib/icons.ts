import ArrowLeftToLine from "@lucide/svelte/icons/arrow-left-to-line";
import Check from "@lucide/svelte/icons/check";
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import ChevronUp from "@lucide/svelte/icons/chevron-up";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import CircleArrowDown from "@lucide/svelte/icons/circle-arrow-down";
import CircleCheck from "@lucide/svelte/icons/circle-check";
import Copy from "@lucide/svelte/icons/copy";
import Eraser from "@lucide/svelte/icons/eraser";
import Eye from "@lucide/svelte/icons/eye";
import FolderOpen from "@lucide/svelte/icons/folder-open";
import FolderPlus from "@lucide/svelte/icons/folder-plus";
import Link from "@lucide/svelte/icons/link";
import PanelBottom from "@lucide/svelte/icons/panel-bottom";
import Pause from "@lucide/svelte/icons/pause";
import Play from "@lucide/svelte/icons/play";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
import RotateCw from "@lucide/svelte/icons/rotate-cw";
import SquarePlay from "@lucide/svelte/icons/square-play";
import ToggleLeft from "@lucide/svelte/icons/toggle-left";
import ToggleRight from "@lucide/svelte/icons/toggle-right";
import Trash2 from "@lucide/svelte/icons/trash-2";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import WandSparkles from "@lucide/svelte/icons/wand-sparkles";

/** Every icon the window uses, one Lucide glyph per SF Symbol (spec section 6.1). */
export const ICONS = {
  "arrow-left-to-line": ArrowLeftToLine,
  check: Check,
  "chevron-down": ChevronDown,
  "chevron-right": ChevronRight,
  "chevron-up": ChevronUp,
  "circle-arrow-down": CircleArrowDown,
  "circle-check": CircleCheck,
  copy: Copy,
  eraser: Eraser,
  eye: Eye,
  "folder-open": FolderOpen,
  "folder-plus": FolderPlus,
  link: Link,
  "panel-bottom": PanelBottom,
  pause: Pause,
  play: Play,
  "refresh-cw": RefreshCw,
  "rotate-ccw": RotateCcw,
  "rotate-cw": RotateCw,
  "square-play": SquarePlay,
  "toggle-left": ToggleLeft,
  "toggle-right": ToggleRight,
  "trash-2": Trash2,
  "triangle-alert": TriangleAlert,
  "wand-sparkles": WandSparkles,
} as const;

export type IconName = keyof typeof ICONS;
