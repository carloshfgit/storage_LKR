// Wrappers tipados dos commands do Tauri. Os tipos espelham os structs Rust
// (crates/scanner/src/model.rs e src-tauri/src/state.rs); os campos seguem o
// snake_case do serde.
import { Channel, invoke } from "@tauri-apps/api/core";

export type Category = "photos" | "videos" | "audio" | "documents" | "apps_system" | "other";

export const CATEGORIES: Category[] = ["photos", "videos", "audio", "documents", "apps_system", "other"];

export interface DiskInfo {
  mount: string;
  device: string;
  total_bytes: number;
  /** Espaço utilizável (`f_bavail`), já sem os blocos reservados. */
  available_bytes: number;
}

export interface FileEntry {
  path: string;
  /** Espaço alocado em disco. */
  bytes: number;
  /** mtime em segundos Unix. */
  modified: number;
}

export interface CategoryStats {
  bytes: number;
  files: number;
  /** Maiores arquivos, em ordem decrescente. */
  top: FileEntry[];
}

/** Uma linha do `docker system df`. */
export interface DockerItem {
  size: number;
  /** O que o Docker considera removível (sem uso ativo). */
  reclaimable: number;
  count: number;
}

export interface DockerUsage {
  images: DockerItem;
  containers: DockerItem;
  volumes: DockerItem;
  build_cache: DockerItem;
}

export interface ScanResult {
  disk: DiskInfo;
  categories: Record<Category, CategoryStats>;
  trash_bytes: number;
  docker: DockerUsage | null;
  docker_bytes: number;
  unaccounted_bytes: number;
  denied_dirs: number;
  files: number;
  elapsed_ms: number;
}

/** Ordem: `progress`* → (`done` → `docker`) | `cancelled` | `error`. */
export type ScanEvent =
  | { kind: "progress"; bytes: Record<Category, number>; files: number }
  | { kind: "done"; result: ScanResult }
  | { kind: "docker"; usage: DockerUsage | null; docker_bytes: number; unaccounted_bytes: number }
  | { kind: "cancelled" }
  | { kind: "error"; message: string };

export function diskInfo(): Promise<DiskInfo> {
  return invoke("disk_info");
}

/** Retorna assim que o scan começa; os eventos chegam por `onEvent`. */
export function startScan(onEvent: (event: ScanEvent) => void): Promise<void> {
  const channel = new Channel<ScanEvent>();
  channel.onmessage = onEvent;
  return invoke("start_scan", { onEvent: channel });
}

export function cancelScan(): Promise<void> {
  return invoke("cancel_scan");
}

export function categoryFiles(category: Category): Promise<FileEntry[]> {
  return invoke("category_files", { category });
}

/** Abre o gerenciador de arquivos com o arquivo selecionado. Só vale para arquivos do último scan. */
export function reveal(path: string): Promise<void> {
  return invoke("reveal", { path });
}
