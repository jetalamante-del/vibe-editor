import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useProjectStore } from "../store/projectStore";
import type { MediaAsset, VideoMetadata } from "../types";

export const useMediaImport = () => {
  const [isLoading, setIsLoading] = useState(false);
  const { addMediaAsset, updateMediaAsset } = useProjectStore();

  const generateProxy = async (asset: MediaAsset) => {
    if (asset.type !== "video" && asset.type !== "audio") return;
    updateMediaAsset(asset.id, { proxyStatus: "pending" });
    try {
      const cmd = asset.type === "audio" ? "prepare_audio_proxy" : "prepare_video_proxy";
      const proxyPath: string = await invoke(cmd, { path: asset.path });
      const isProxy = proxyPath !== asset.path;
      updateMediaAsset(asset.id, {
        proxyPath: isProxy ? proxyPath : undefined,
        proxyStatus: "ready",
      });
    } catch (err) {
      console.error(`[proxy] ${asset.type} proxy failed for ${asset.name}:`, err);
      updateMediaAsset(asset.id, { proxyStatus: "error", proxyError: String(err) });
    }
  };

  const importMedia = async () => {
    try {
      setIsLoading(true);
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "Media",
            extensions: ["mp4", "mov", "avi", "mkv", "mp3", "wav", "aac", "jpg", "png", "webp"],
          },
        ],
      });

      if (!selected) return;

      const files = Array.isArray(selected) ? selected : [selected];

      for (const path of files) {
        try {
          const filename = path.split("/").pop() || "Unknown";
          const type = getMediaType(path);

          if (type === "video" || type === "audio") {
            const metadata: VideoMetadata = await invoke("get_video_metadata", { path });
            const posterFrame: string | undefined =
              type === "video"
                ? ((await invoke("extract_poster_frame", { path, time: 0.0 }).catch((err) => {
                    console.error("Failed to extract poster frame:", err);
                    return undefined;
                  })) as string | undefined)
                : undefined;

            const asset: MediaAsset = {
              id: `asset-${Date.now()}-${Math.random()}`,
              name: filename,
              path,
              type,
              duration: metadata.duration,
              width: metadata.width,
              height: metadata.height,
              posterFrame,
              size: metadata.size,
              proxyStatus: "none",
            };
            addMediaAsset(asset);
            // Kick off proxy generation in the background — playback falls back
            // to the original until the proxy is ready, then switches automatically.
            void generateProxy(asset);
          } else {
            // For images, use the convertFileSrc to create a proper asset URL
            const { convertFileSrc } = await import("@tauri-apps/api/core");
            const asset: MediaAsset = {
              id: `asset-${Date.now()}-${Math.random()}`,
              name: filename,
              path,
              type: "image",
              duration: 0,
              size: 0,
              posterFrame: convertFileSrc(path), // Use the image itself as preview
            };
            addMediaAsset(asset);
          }
        } catch (fileError) {
          console.error(`Failed to import ${path}:`, fileError);
          // Continue with next file instead of stopping
        }
      }
    } catch (error) {
      console.error("Import failed:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const getMediaType = (path: string): "video" | "audio" | "image" => {
    const lower = path.toLowerCase();
    if (/\.(mp4|mov|avi|mkv|webm|flv)$/i.test(lower)) return "video";
    if (/\.(mp3|wav|aac|flac|m4a)$/i.test(lower)) return "audio";
    return "image";
  };

  return {
    importMedia,
    isLoading,
  };
};
