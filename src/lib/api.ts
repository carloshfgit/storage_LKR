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

/** Daemon do sistema ou rootless do usuário; cada um tem o seu socket fixo no Rust. */
export type DockerMode = "rootful" | "rootless";

/** `null` onde não há daemon acessível. */
export type DockerStatus = Record<DockerMode, DockerUsage | null>;

export interface ScanResult {
  disk: DiskInfo;
  categories: Record<Category, CategoryStats>;
  trash_bytes: number;
  trash_files: number;
  /** Arquivos sem extensão classificados pelo conteúdo, já contados nas categorias. */
  sniffed_files: number;
  sniffed_bytes: number;
  docker: DockerStatus;
  /** Parte do restante atribuída ao Docker (os dois daemons). */
  docker_bytes: number;
  /** Lido pelo walk em `~/.local/share/docker`, já dentro de Apps/Sistema. */
  rootless_scanned_bytes: number;
  unaccounted_bytes: number;
  denied_dirs: number;
  files: number;
  elapsed_ms: number;
  /** Fim do scan em segundos Unix. */
  finished_at: number;
}

/** Ordem: `progress`* → (`done` → `docker`) | `cancelled` | `error`. */
export type ScanEvent =
  | { kind: "progress"; bytes: Record<Category, number>; files: number }
  | { kind: "done"; result: ScanResult }
  | { kind: "docker"; docker: DockerStatus; docker_bytes: number; unaccounted_bytes: number }
  | { kind: "cancelled" }
  | { kind: "error"; message: string };

export function diskInfo(): Promise<DiskInfo> {
  return invoke("disk_info");
}

/** Último resultado conhecido: ao abrir o app, o que estava no cache (`~/.cache/storage-lkr`). */
export function lastScan(): Promise<ScanResult | null> {
  return invoke("last_scan");
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

/** Arquivo do detalhe, com o veredito da mesma validação do `moveToTrash`. */
export interface FileItem extends FileEntry {
  trashable: boolean;
}

export function categoryFiles(category: Category): Promise<FileItem[]> {
  return invoke("category_files", { category });
}

/** Abre o gerenciador de arquivos com o arquivo selecionado. Só vale para arquivos do último scan. */
export function reveal(path: string): Promise<void> {
  return invoke("reveal", { path });
}

export interface TrashReport {
  moved: number;
  moved_bytes: number;
  rejected: { path: string; reason: string }[];
  /** Último resultado já atualizado no Rust. */
  result: ScanResult;
}

export interface Cleanup {
  /** Diferença do espaço livre antes e depois, como o `df` mostra. */
  freed_bytes: number;
  result: ScanResult;
}

export type DockerTarget = "build_cache" | "unused_images";

/** Valida cada caminho no Rust; os recusados voltam em `rejected` com o motivo. */
export function moveToTrash(paths: string[]): Promise<TrashReport> {
  return invoke("move_to_trash", { paths });
}

/** Apaga permanentemente a Lixeira. Chamar só depois da confirmação do usuário. */
export function emptyTrash(): Promise<Cleanup> {
  return invoke("empty_trash", { confirm: true });
}

/** `build_cache` → `docker builder prune -f`; `unused_images` → `docker image prune -a -f`, no daemon de `mode`. */
export function dockerPrune(mode: DockerMode, target: DockerTarget): Promise<Cleanup> {
  return invoke("docker_prune", { mode, target, confirm: true });
}
