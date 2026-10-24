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

/** Download antigo: guarda a categoria em que foi contado. */
export interface SuggestedFile extends FileEntry {
  category: Category;
}

/** Item direto de `~/.cache` (normalmente a pasta de cache de um app). */
export interface CacheEntry {
  path: string;
  bytes: number;
  files: number;
}

/** Item direto da Lixeira, como o scan o contou. */
export interface TrashEntry {
  /** Onde está guardado na Lixeira ou, se `moved`, o caminho original. */
  path: string;
  bytes: number;
  files: number;
  /** Movido pelo app depois do scan. */
  moved: boolean;
}

export interface Suggestions {
  /** Pasta de Downloads do `user-dirs.dirs`; `null` se desligada. */
  downloads_dir: string | null;
  /** mtime abaixo do qual um download conta como antigo. */
  old_before: number;
  old_downloads: { bytes: number; files: number; top: SuggestedFile[] };
  /** Os maiores itens de `~/.cache`, em ordem decrescente. */
  caches: CacheEntry[];
}

/** Dias sem modificação para um download contar como antigo (`OLD_DOWNLOAD_DAYS`). */
export const OLD_DOWNLOAD_DAYS = 90;

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
  /** Itens da Lixeira contados no scan (e os que o app moveu para lá depois). */
  trash_items: TrashEntry[];
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
  suggestions: Suggestions;
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

export function oldDownloads(): Promise<FileItem[]> {
  return invoke("old_downloads");
}

/** Os `n` maiores arquivos que podem ir para a Lixeira (no máximo 50). */
export function largestFiles(n: number): Promise<FileItem[]> {
  return invoke("largest_files", { n });
}

/** Move itens inteiros de `~/.cache` para a Lixeira; validados no Rust como o `moveToTrash`. */
export function trashCaches(paths: string[]): Promise<TrashReport> {
  return invoke("trash_caches", { paths });
}

/** Abre o gerenciador de arquivos com o arquivo selecionado. Só vale para arquivos (e caches) do último scan. */
export function reveal(path: string): Promise<void> {
  return invoke("reveal", { path });
}

/** Item recusado por uma ação, com o motivo. */
export interface Rejected {
  path: string;
  reason: string;
  /** Comandos para resolver pelo terminal, quando o app não consegue (pastas do root, por exemplo). */
  commands?: string[];
}

export interface TrashReport {
  moved: number;
  moved_bytes: number;
  rejected: Rejected[];
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

/** Apaga permanentemente a Lixeira, item por item; os que falham voltam em `rejected`. Chamar só depois da confirmação do usuário. */
export function emptyTrash(): Promise<PurgeReport> {
  return invoke("empty_trash", { confirm: true });
}

/** `build_cache` → `docker builder prune -f`; `unused_images` → `docker image prune -a -f`, no daemon de `mode`. */
export function dockerPrune(mode: DockerMode, target: DockerTarget): Promise<Cleanup> {
  return invoke("docker_prune", { mode, target, confirm: true });
}

export interface DockerImage {
  id: string;
  /** `<none>` numa imagem sem nome (dangling). */
  repository: string;
  tag: string;
  size: number;
  /** Camadas que outras imagens também usam; removê-la não libera esta parte. */
  shared_size: number;
  /** Containers (rodando ou parados) que usam a imagem. */
  containers: number;
  /** Segundos Unix; 0 se o Docker não informar. */
  created: number;
}

export interface DockerContainer {
  id: string;
  name: string;
  image: string;
  /** `running`, `exited`, `created`, `paused`… */
  state: string;
  /** Camada gravável do container. */
  size: number;
  /** Projeto do Docker Compose. */
  project: string | null;
  created: number;
}

export interface DockerVolume {
  name: string;
  size: number;
  /** Containers que montam o volume; 0 = sem uso. */
  links: number;
  /** Criado sem nome; o `docker volume prune` remove só esses. */
  anonymous: boolean;
  project: string | null;
}

export interface DockerCacheEntry {
  id: string;
  /** Passo do build (`[builder 3/5] COPY …`, `pulled from …`). */
  description: string;
  size: number;
  in_use: boolean;
  shared: boolean;
  last_used: number;
  usage_count: number;
}

/** `docker system df -v` de um daemon; cada lista do maior para o menor. */
export interface DockerDetails {
  /** Versão do daemon (`"29.9.1"`); `null` se não deu para saber. */
  server_version: string | null;
  images: DockerImage[];
  containers: DockerContainer[];
  volumes: DockerVolume[];
  build_cache: DockerCacheEntry[];
}

/** Só leitura; leva uns 5 s (o daemon mede tudo de novo). */
export function dockerDetails(mode: DockerMode): Promise<DockerDetails> {
  return invoke("docker_details", { mode });
}

/** Um item da Lixeira de agora, como o sistema o lista. */
export interface TrashItem {
  /** O que volta para o Rust em `restoreFromTrash`/`purgeFromTrash`. */
  id: string;
  name: string;
  original_path: string;
  /** Onde está guardado (`~/.local/share/Trash/files/…`); serve para o "abrir na pasta". */
  stored_path: string;
  /** Segundos Unix. */
  deleted_at: number;
  bytes: number;
  files: number;
  /** Contado no último scan; senão, o tamanho foi medido agora e ainda não está na barra. */
  counted: boolean;
  /** Tem pastas que o usuário não consegue esvaziar: apagar pelo app vai falhar. */
  blocked: { reason: string; commands: string[] } | null;
}

export interface PurgeReport {
  purged: number;
  freed_bytes: number;
  rejected: Rejected[];
  result: ScanResult;
}

/** A Lixeira de agora, os maiores primeiro. */
export function trashItems(): Promise<TrashItem[]> {
  return invoke("trash_items");
}

/** Devolve os itens ao local original, sem sobrescrever; um nome já ocupado volta em `rejected`. */
export function restoreFromTrash(ids: string[]): Promise<TrashReport> {
  return invoke("restore_from_trash", { ids });
}

/** Apaga permanentemente só esses itens. Chamar só depois da confirmação do usuário. */
export function purgeFromTrash(ids: string[]): Promise<PurgeReport> {
  return invoke("purge_from_trash", { ids, confirm: true });
}

// ── Monitor de recursos (aba "Recursos", PLAN-V2). Espelha crates/monitor/src/model.rs,
// info.rs e src-tauri/src/state/monitor.rs. Taxas vêm `null` na primeira amostra, e
// todo campo que depende do hardware pode vir `null`.

export interface CoreSample {
  id: number;
  usage_pct: number | null;
  freq_mhz: number | null;
}

/** Temperatura com o limiar de alerta do sensor. */
export interface Temp {
  celsius: number;
  hot_c: number;
  /** Sensor da placa (ACPI), não da peça. */
  approximate: boolean;
}

export interface CpuSample {
  usage_pct: number | null;
  freq_avg_mhz: number | null;
  freq_max_mhz: number | null;
  temp: Temp | null;
  /** Só os núcleos online. */
  cores: CoreSample[];
}

export interface Zram {
  devices: number;
  disksize: number;
  orig_bytes: number;
  compr_bytes: number;
  mem_used: number;
}

/** % do tempo esperando por memória nos últimos 10 s. */
export interface Psi {
  some_avg10: number;
  full_avg10: number | null;
}

export interface MemSample {
  total: number;
  /** `total − available`, como o `free`. */
  used: number;
  available: number;
  swap_total: number;
  swap_used: number;
  zram: Zram | null;
  psi: Psi | null;
}

export type DiskKind = "nvme" | "sata" | "usb" | "mmc" | "virtual" | "other";

export interface DiskIo {
  name: string;
  model: string | null;
  kind: DiskKind;
  rotational: boolean | null;
  read_bps: number | null;
  write_bps: number | null;
  busy_pct: number | null;
  temp: Temp | null;
  /** Comando que liga o sensor (SATA sem o `drivetemp`). */
  temp_hint: string | null;
}

export interface NetIface {
  name: string;
  wireless: boolean;
  rx_bps: number | null;
  tx_bps: number | null;
}

export interface NetSample {
  rx_bps: number | null;
  tx_bps: number | null;
  /** Bytes desde a primeira amostra. */
  rx_session: number;
  tx_session: number;
  /** Só interfaces físicas. */
  interfaces: NetIface[];
}

export type BatteryStatus = "charging" | "discharging" | "full" | "not_charging" | "unknown";

export interface BatterySample {
  name: string;
  model: string | null;
  pct: number | null;
  status: BatteryStatus;
  energy_now_wh: number | null;
  energy_full_wh: number | null;
  energy_full_design_wh: number | null;
  power_w: number | null;
  /** Capacidade atual / de fábrica, em %. */
  health_pct: number | null;
  cycles: number | null;
}

export interface BatteryTotal {
  /** Ponderado pela energia de cada bateria. */
  pct: number | null;
  status: BatteryStatus;
  power_w: number | null;
  /** Até esvaziar ou até encher; `null` nos primeiros segundos e na tomada. */
  time_left_s: number | null;
}

export interface Sample {
  /** ms desde o início da sessão (relógio monotônico). */
  at_ms: number;
  uptime_s: number;
  cpu: CpuSample;
  mem: MemSample;
  /** Só discos físicos inteiros. */
  disks: DiskIo[];
  net: NetSample;
  /** Vazio no desktop. */
  batteries: BatterySample[];
  battery_total: BatteryTotal | null;
}

export interface DiskSpec {
  name: string;
  model: string | null;
  kind: DiskKind;
  rotational: boolean | null;
  size_bytes: number;
}

export interface SystemInfo {
  cpu_model: string | null;
  /** Núcleos físicos; `null` quando o sistema não informa. */
  cores: number | null;
  threads: number;
  ram_total: number;
  kernel: string | null;
  distro: string | null;
  hostname: string | null;
  disks: DiskSpec[];
}

/** Ordem: `sample`* → `stopped` | `error`. */
export type MonitorEvent =
  | { kind: "sample"; sample: Sample }
  | { kind: "stopped" }
  | { kind: "error"; message: string };

/** Uma amostra por segundo até o `stopMonitor` ou outro `startMonitor`. */
export function startMonitor(onEvent: (event: MonitorEvent) => void): Promise<void> {
  const channel = new Channel<MonitorEvent>();
  channel.onmessage = onEvent;
  return invoke("start_monitor", { onEvent: channel });
}

export function stopMonitor(): Promise<void> {
  return invoke("stop_monitor");
}

export function systemInfo(): Promise<SystemInfo> {
  return invoke("system_info");
}
