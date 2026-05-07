export type AspectRatio = "16:9" | "9:16" | "1:1" | "4:3" | "21:9";

export interface VideoMetadata {
  duration: number;
  width: number;
  height: number;
  fps: number;
  size: number;
}

export interface Project {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  aspectRatio: AspectRatio;
  canvasWidth: number;
  canvasHeight: number;
  frameRate: 24 | 30 | 60;
  duration: number;
}

export type TrackType = "video" | "audio" | "text";

export interface Track {
  id: string;
  type: TrackType;
  name: string;
  muted: boolean;
  locked: boolean;
  visible: boolean;
  height: number;
}

export interface MediaAsset {
  id: string;
  name: string;
  /** Original source file path. Always preserved for export. */
  path: string;
  /**
   * Optional WebKit-friendly preview file. Used for playback in the editor
   * when the source codec/format hits WebKit limits (4K HEVC, 24-bit WAV,
   * etc.). Falls back to `path` when absent. The original is never modified.
   */
  proxyPath?: string;
  /** Lifecycle of the proxy generation job for UI feedback. */
  proxyStatus?: "none" | "pending" | "ready" | "error";
  proxyError?: string;
  type: "video" | "audio" | "image";
  duration: number;
  width?: number;
  height?: number;
  posterFrame?: string;
  size: number;
}

export interface Clip {
  id: string;
  trackId: string;
  mediaId: string;
  startTime: number;
  duration: number;
  trimIn: number;
  trimOut: number;
  x: number;
  y: number;
  width: number;
  height: number;
  opacity: number;
  rotation: number;
}

export interface TextClip extends Clip {
  text: string;
  fontSize: number;
  fontFamily: string;
  color: string;
  bold: boolean;
  italic: boolean;
}

export type DragItem = { type: "MEDIA_ASSET"; asset: MediaAsset } | { type: "CLIP"; clip: Clip };
