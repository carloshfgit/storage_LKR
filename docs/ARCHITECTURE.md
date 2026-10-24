# ARCHITECTURE — Armazenamento (Ubuntu, estilo iOS/Android)

## 1. Visão geral

```
┌──────────────────────────── Frontend (Svelte 5, WebKitGTK) ───────────────────────────┐
│  +page.svelte ─ Header (disco) ─ StorageBar (7 segmentos) ─ Legend ─ CategoryDetail   │
│                    ▲ ScanEvent (Channel)                 │ invoke(...)                │
└────────────────────┼─────────────────────────────────────┼────────────────────────────┘
                     │                                     ▼
┌──────────────────────────── src-tauri (Rust, comandos) ───────────────────────────────┐
│  commands.rs: disk_info · start_scan · cancel_scan · category_files · move_to_trash   │
│               empty_trash · reveal          state.rs: Mutex<Option<ScanResult>>       │
└────────────────────┬──────────────────────────────────────────────────────────────────┘
                     ▼
┌──────────────────────────── crates/scanner (Rust puro, sem Tauri) ────────────────────┐
│  disk.rs (statvfs) → walk.rs (jwalk, xdev, blocks, hardlinks) → classify.rs           │
│                    → aggregate (totais + top-N por categoria) → ScanResult            │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

**Fluxo:**
1. A UI chama `start_scan`, que sobe uma thread com jwalk.
2. Cada arquivo passa por `classify` e entra no agregador.
3. A cada cerca de 100 ms sai um `Progress` pelo Channel, e a barra anima.
4. No fim sai `Done`, e a UI passa a oferecer o detalhe e as ações.
5. As ações chamam o crate `trash`, o estado é atualizado e a barra é re-renderizada.

---

## 2. Layout do repositório

```
storage_LKR/
├── Cargo.toml                 # workspace: ["crates/monitor", "crates/scanner", "src-tauri"]
├── package.json  vite.config.js  svelte.config.js  tsconfig.json
├── crates/scanner/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # API pública: scan(), disk_info(), tipos
│       ├── model.rs           # Category, FileEntry, ScanResult, ScanProgress
│       ├── disk.rs            # statvfs → DiskInfo
│       ├── walk.rs            # walk paralelo + agregação
│       ├── classify.rs        # regras caminho/extensão → Category
│       ├── docker.rs          # `docker system df` por daemon → DockerStatus (opcional)
│       └── bin/scan.rs        # CLI p/ benchmark e validação
├── crates/monitor/            # telemetria em tempo real (V2, §11)
│   └── src/
│       ├── lib.rs  model.rs   # Sampler, Sample e os tipos serializados
│       ├── sampler.rs         # junta os coletores e calcula as taxas
│       ├── cpu.rs  mem.rs  disk_io.rs  net.rs   # parsers puros de /proc e /sys
│       └── bin/monitor.rs     # CLI p/ validar contra top, free, iostat, sar
├── src-tauri/
│   ├── Cargo.toml  tauri.conf.json
│   ├── build.rs               # lista de commands → permissões allow-<command>
│   ├── capabilities/default.json
│   └── src/
│       ├── main.rs  lib.rs    # lib.rs: registra AppState e os commands
│       ├── commands.rs        # commands finos, só repassam ao AppState
│       ├── state.rs           # AppState + ScanEvent: ciclo do scan, testável sem Tauri
│       ├── state/cleanup.rs   # ações de limpeza e a validação delas (Etapa 6)
│       ├── state/trash.rs     # itens da Lixeira: listar, restaurar e apagar (Etapa 12)
│       └── state/cache.rs     # último resultado em ~/.cache/storage-lkr (Etapa 7)
└── src/                       # SvelteKit em modo SPA (adapter-static, ssr = false)
    ├── app.html
    ├── routes/+layout.ts  routes/+page.svelte   # tela única; orquestra o estado
    └── lib/                   # importado como $lib/…
        ├── StorageBar.svelte  Legend.svelte  CategoryDetail.svelte  FileRow.svelte
        ├── theme.css
        ├── api.ts             # wrappers tipados de invoke/Channel (✅ Etapa 3)
        ├── categories.ts      # nome e cor de cada categoria (✅ Etapa 4)
        └── format.ts          # bytes → "12,4 GB" (decimal, pt-BR) (✅ Etapa 4)
```

O motor fica num crate separado de propósito: pode ser testado com `cargo test` e medido com o CLI sem abrir a janela.

O template gerado na Etapa 0 é **SvelteKit**, não Svelte puro. Como é um app de uma tela, a diferença é só de lugar: o papel do `App.svelte` fica com `routes/+page.svelte`. O SvelteKit roda como SPA estática (`adapter-static`, `ssr = false`), sem servidor.

O perfil `dev` compila o `scanner`, o `jwalk`, o `rayon` e o `crossbeam` com `opt-level = 3`. Sem isso, o walk no `tauri dev` leva o dobro (DECISIONS D27).

---

## 3. Modelo de dados (`model.rs`)

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category { Photos, Videos, Audio, Documents, AppsSystem, Other }
// "Livre" não é Category: vem do statvfs.

pub struct FileEntry { path: PathBuf, bytes: u64, modified: i64 /* unix s */ }

pub struct CategoryStats { bytes: u64, files: u64, top: Vec<FileEntry> /* top-N desc */ }

pub struct ScanResult {
    disk: DiskInfo,                       // total, available, mount, device
    categories: HashMap<Category, CategoryStats>,
    trash_bytes: u64,                     // subitem de Other
    trash_files: u64,                     // arquivos dentro da Lixeira, já contados em Other
    trash_items: Vec<TrashEntry>,         // por item direto de Trash/files (§4, Etapa 12)
    sniffed_files: u64, sniffed_bytes: u64, // sem extensão, classificados pelo conteúdo (§5, regra 6b)
    home: PathBuf,                        // $HOME da classificação; não serializado
    docker: DockerStatus,                 // { rootful, rootless }: preenchido depois, por apply_docker (§6.1)
    docker_bytes: u64,                    // parte do restante atribuída ao Docker (os dois daemons)
    rootless_scanned_bytes: u64,          // lido pelo walk em ~/.local/share/docker, já em AppsSystem
    unaccounted_bytes: u64,               // "Reservado / sem acesso" → somado em AppsSystem
    denied_dirs: u64,
    suggestions: Suggestions,             // downloads antigos e caches (§4, Etapa 10)
    elapsed_ms: u64,
    finished_at: i64,                     // fim do scan (unix s): "há 2 h" do cache
}

pub struct Suggestions {
    downloads_dir: Option<PathBuf>,       // XDG_DOWNLOAD_DIR do user-dirs.dirs; None = desligada
    old_before: i64,                      // mtime de corte: início do scan − 90 dias
    old_downloads: OldDownloads,          // { bytes, files, top: Vec<SuggestedFile> } (top-N = 200)
    caches: Vec<CacheEntry>,              // { path, bytes, files }: os 20 maiores itens diretos de ~/.cache
}
pub struct SuggestedFile { #[serde(flatten)] entry: FileEntry, category: Category }

pub struct TrashEntry {
    path: PathBuf,                        // em Trash/files; se moved, o caminho original
    bytes: u64, files: u64,
    moved: bool,                          // movido pelo app depois do scan (o nome na Lixeira só o sistema sabe)
}
```

Métodos usados pelas ações de limpeza (§7), todos mantendo a barra em 100%:
- `contains_file(path)`: o arquivo consta no top-N de uma categoria ou nos downloads antigos. É a regra 1 do `check_trashable`.
- `record_trashed(path)`: tira o arquivo do top-N e da categoria e soma em `Other`, `trash_bytes` e `trash_files`. Um download antigo fora do top-N da categoria é achado em `old_downloads`, que guarda a categoria dele. Se o arquivo é um download antigo (pasta e mtime), também sai de `old_downloads`.
- `record_trashed_cache(path)`: tira o item de `caches` e soma os bytes e arquivos dele em `trash_bytes`/`trash_files`. `Other` não muda, porque `~/.cache` já estava lá (regra 4). Os arquivos de dentro do item saem dos top-N, porque mudaram de caminho.
- `record_trashed` e `record_trashed_cache` também acrescentam o item em `trash_items`, com `moved = true` e o caminho original: o crate `trash` pode guardar `foto.jpg` como `foto.2.jpg`, e só o `.trashinfo` diz isso.
- `find_trash_item(stored, original)`: o item do scan guardado em `stored` (ou, se `moved`, vindo de `original`).
- `record_restored(stored, original)`: tira o item de `trash_items`, de `Other`/`trash_bytes`/`trash_files` e do top-N, e soma os bytes na categoria de `classify(original)`, fora do top-N. Um item que o scan não contou não muda nada.
- `record_purged(stored, original)`: o mesmo, sem devolver os bytes a nenhuma categoria. Chamar `set_disk` depois.
- `clear_trash()`: tira de `Other` o que estava na Lixeira, inclusive os itens dela no top-N, e zera `trash_items`.
- `set_disk(disk)`: troca o `DiskInfo` depois de uma limpeza, recalcula o restante e reaplica o limite do Docker.

---

## 4. Motor de scan (`walk.rs`)

| Decisão | Detalhe |
|---|---|
| Raiz | `/`, rodando como usuário comum, sem root |
| Não cruzar filesystem | Só desce em diretórios com `st_dev` igual ao da raiz. Isso exclui `/proc`, `/sys`, `/run`, `/dev`, os squashfs de `/snap/*`, pendrives e afins |
| Symlinks | Não são seguidos e não contam tamanho |
| Tamanho | `st_blocks * 512`, o **espaço alocado**, que é o que bate com o `df`. Arquivos esparsos e pequenos ficam corretos |
| Hardlinks | Quando `st_nlink > 1`, o arquivo só conta se `(dev, ino)` ainda não estiver no `HashSet` |
| Paralelismo | `jwalk::WalkDirGeneric` com `Parallelism::RayonNewPool(num_cpus)` e `skip_hidden(false)`, mais `process_read_dir` para podar diretórios de outro `st_dev` |
| Erros | `EACCES` incrementa `denied_dirs` e o walk continua. O espaço não lido aparece em `unaccounted_bytes` |
| Agregação | Por categoria: `bytes`, `files` e `BinaryHeap<Reverse<(bytes, path)>>` limitado a **N = 200**. Só o top-N vai para a UI, nunca os mais de 600 mil arquivos |
| Progresso | Um callback `Fn(ScanProgress)` com rate-limit de cerca de 100 ms (bytes por categoria e arquivos vistos) |
| Cancelamento | `Arc<AtomicBool>`, checado no laço do walk |
| Sugestões | `suggest.rs`, Etapa 10. Nas threads do walk, `Suggester` marca cada entrada com dois testes de prefixo: download antigo (arquivo sob a pasta de Downloads com mtime antes de `old_before`) e "dentro de `~/.cache`". Na thread que consome o walk, `SuggestionsAgg` soma os downloads antigos (bytes, arquivos e um heap top-N próprio, com a categoria) e os bytes e arquivos por item direto de `~/.cache`, chaveados pelo primeiro componente de `parent_path`, sem alocar por entrada. Sem I/O extra. A pasta de Downloads vem de `ScanOptions.downloads`, resolvida em `ScanOptions::new` por `download_dir(home, config_dir)`: `XDG_DOWNLOAD_DIR` do `user-dirs.dirs`, `~/Downloads` sem o arquivo e `None` se apontar para o próprio `$HOME` |
| Lixeira por item | Etapa 12. Cada entrada sob a Lixeira soma, além de `trash_bytes`, no item direto de `~/.local/share/Trash/files` que a contém (`ChildTotals`, o mesmo agregador dos caches). Vira `trash_items`, em ordem decrescente e sem limite: a Lixeira raramente passa de alguns milhares de itens. Os diretórios `Trash/`, `files/` e `info/` e os `.trashinfo` ficam só em `trash_bytes`. `tree_size(path)` mede um item que o scan não viu, com as mesmas regras (blocos alocados, sem seguir links nem cruzar filesystems) |
| Conteúdo | Para arquivos regulares a que `classify_by_path` não sabe dar categoria (regra 6b), `sniff` lê o cabeçalho ali mesmo, nas threads do rayon. `ScanOptions.sniff = false` desliga (`scan --no-sniff`) |

**Meta de desempenho** (SSD, 4 núcleos, cache quente): `/` com cerca de 1 milhão de entradas em **menos de 5 s**. O primeiro scan com cache frio pode levar mais.

---

## 5. Classificação (`classify.rs`)

`Classifier::new(home)` monta uma vez os caminhos derivados do `$HOME`; `classifier.classify(path) -> Category` é pura e roda nas threads do walk. Os prefixos são comparados por bytes e respeitam componentes inteiros (`/usrdata` não é `/usr`). As regras são avaliadas em ordem e **a primeira que casar vence**:

| # | Regra | Categoria |
|---|---|---|
| 1 | Prefixos de sistema e de apps gerenciados: `/usr`, `/opt`, `/boot`, `/etc`, `/bin`, `/sbin`, `/lib*`, `/var/lib/{snapd,flatpak,apt,dpkg}`, `/var/cache/apt` e, em `$HOME`, `~/snap`, `~/.var/app`, `~/.local/share/{flatpak,Steam,docker,containers}` | **Apps/Sistema** (bloqueia limpeza) |
| 2 | `~/.local/share/Trash/**` | **Outros** (conta também em `trash_bytes`) |
| 3 | **App portátil** no `$HOME`: arquivo `*.AppImage`; primeiro componente `Applications`, `Apps`, `Aplicativos` ou `bin`; ou o par de componentes `resources/app`, `resources/app.asar` ou `resources/app.asar.unpacked` (Electron) | **Apps/Sistema** (pode ser limpo) |
| 4 | Dentro de `$HOME` com qualquer componente oculto (`.cache`, `.mozilla`, `.config`…) | **Outros** |
| 5 | **Build/dev**: componente `node_modules`, `__pycache__` ou `site-packages`, ou o par `target/debug` / `target/release` (Cargo) | **Outros** |
| 6 | Dentro de `$HOME`, visível, extensão (minúscula) na tabela abaixo | Fotos / Vídeos / Áudio / Documentos |
| 6b | Dentro de `$HOME`, visível, **sem extensão**, arquivo regular com 4 KiB ou mais: assinatura do cabeçalho (`sniff.rs`) | Fotos / Vídeos / Áudio / Documentos |
| 7 | Dentro de `$HOME`, visível, outra extensão (zip, iso, código) ou sem extensão e sem assinatura conhecida | **Outros** |
| 8 | Fora de `$HOME` e não coberto acima (`/var/log`, `/srv`, `/root`…) | **Apps/Sistema** |

Por que essa ordem:
- **1 antes da extensão:** um `.png` de ícone em `/usr/share` não vira "Fotos".
- **2 antes de 3:** um app jogado na Lixeira continua em Outros, e `trash_bytes` cabe dentro de Outros.
- **3 antes de 4 e 5:** apps Electron trazem o próprio `node_modules` (`resources/app/node_modules/…`), e alguns ficam em pastas ocultas. O app tem que vencer.
- **4:** impede que os JPGs do cache do navegador inflem "Fotos".
- **5:** impede que imagens, `.md` e sons de dependências inflem as mídias.

Detalhes de implementação:
- As regras 3, 4 e 5 são decididas numa **única passada** pelos componentes do caminho relativo ao `$HOME`, sem I/O. Medido: walk de `/` sem mudança (2,2 a 2,5 s).
- Os padrões casam componentes inteiros: `~/Fotos/bin/a.jpg` continua Foto, porque `bin` só vale como primeiro componente, e `~/myresources/app` não é Electron.
- **`target` sozinho não conta**, porque é um nome comum demais (`~/Documentos/target/relatorio.pdf` fica em Documentos). **`vendor` ficou de fora** pelo mesmo motivo.
- `Classifier::is_app_path` cobre **só a regra 1**. Apps portáteis (regra 3) aparecem em Apps/Sistema, mas podem ir para a Lixeira: apagar um AppImage ou um app descompactado é uma limpeza legítima. Apagar arquivos de dentro de um snap ou flatpak quebra o app.

**Tabela de extensões** (um `match` estático, sem alocação):

| Categoria | Extensões |
|---|---|
| Fotos | jpg jpeg png gif webp heic heif avif bmp tif tiff svg raw dng cr2 cr3 nef arw orf rw2 |
| Vídeos | mp4 m4v mkv mov avi webm wmv flv mpg mpeg 3gp |
| Áudio | mp3 flac ogg oga opus m4a aac wav aiff wma |
| Documentos | pdf doc docx odt rtf txt md xls xlsx ods csv ppt pptx odp epub |

`.ts` **não** é vídeo: nesta máquina, 37.267 dos 37.285 "vídeos" eram TypeScript. MPEG-TS é raro no desktop (DECISIONS D22).

**Conteúdo (regra 6b, Etapa 8, DECISIONS D37):** `classify` continua pura. O walk chama `classify_by_path`, que devolve `None` só no caso da regra 6b. Nesse caso, e só para arquivos regulares com pelo menos `MIN_SNIFF_BYTES` (4 KiB), `sniff(path)` lê até 32 bytes, com `O_NOATIME`, e compara com uma tabela estática:

| Categoria | Assinaturas |
|---|---|
| Fotos | JPEG, PNG, GIF, TIFF (e RAW baseados nele), WebP (`RIFF…WEBP`), HEIC/AVIF (`ftyp` + `heic`, `mif1`, `avif`…) |
| Vídeos | `ftyp` + `isom`, `mp4x`, `qt  `, `M4V `, `3gpx`…; Matroska/WebM; AVI (`RIFF…AVI `); MPEG-PS |
| Áudio | `ID3`, quadro MPEG/ADTS sem tag, `fLaC`, `OggS`, WAV (`RIFF…WAVE`), AIFF, `ftyp` + `M4A `/`M4B ` |
| Documentos | `%PDF-` |

ZIP (docx, odt, epub), ELF, scripts e qualquer erro de leitura ficam em Outros. Os bytes não mudam, só a categoria: o total continua batendo com `du`/`df`. `ScanResult.sniffed_files/bytes` contam os reclassificados. Medido: walk de `/` sem diferença além do ruído (cerca de 400 leituras no `$HOME` desta máquina).

---

## 6. Fechamento da barra (`disk.rs`)

Via `statvfs("/")`:

```
total     = f_blocks * f_frsize
livre     = f_bavail * f_frsize          # o que o usuário realmente pode usar
usado     = total − livre
escaneado = Σ categorias
restante  = max(0, usado − escaneado)    # dirs sem permissão + metadados ext4 + blocos reservados (~5%)
AppsSystem += restante
```

Assim, **a barra sempre soma 100% do disco**, como o "Sistema" do iOS. Nesta máquina o ext4 reserva cerca de 10 GB (218 G no total, 155 G usados, 53 G disponíveis), que aparecem como "Reservado / sem acesso" no detalhe de Apps/Sistema.

### 6.1 Docker: identificar o maior "sem acesso"

O `/var/lib/docker` e o `/var/lib/containerd` são só de root, então o walk não os enxerga. Nesta máquina eles explicam a maior parte do restante: dos 106 GB, cerca de 97 GB são Docker (21,7 GB de imagens, 16,7 GB de volumes e 58,7 GB de build cache), dos quais cerca de 75 GB podem ser liberados.

O daemon informa esse uso sem root quando o usuário está no grupo `docker`:

```
docker system df --format json      # uma linha JSON por tipo: Images, Containers, Local Volumes, Build Cache
→ {"Type":"Build Cache","Size":"58.68GB","Reclaimable":"39.88GB","TotalCount":"252","Active":"0"}
```

```rust
pub struct DockerItem { size: u64, reclaimable: u64, count: u64 }   // uma linha do `docker system df`
pub struct DockerUsage { images: DockerItem, containers: DockerItem, volumes: DockerItem, build_cache: DockerItem }
// total() = Σ size; reclaimable() = Σ reclaimable
```

- `docker_usage(mode, timeout)` roda o comando com timeout de 20 s (`DEFAULT_DOCKER_TIMEOUT`). Se o binário não existir, o daemon estiver parado ou faltar permissão, o resultado é `None` e o app segue sem Docker, sem erro.
- **Dois daemons (Etapa 9, DECISIONS D38).** `DockerMode` = `Rootful` | `Rootless`, e cada um tem o seu **socket fixo**, passado como `docker --host unix://…`: `/var/run/docker.sock` para o do sistema e `$XDG_RUNTIME_DIR/docker.sock` (ou `/run/user/<uid>/docker.sock`) para o rootless. O contexto atual do `docker` é ignorado: depois de `docker context use rootless`, o comando sem `--host` falaria com o rootless, e o mesmo daemon apareceria duas vezes. Socket inexistente vira `None` sem nem rodar o `docker`. `docker_status(timeout)` consulta os dois em paralelo e devolve `DockerStatus { rootful, rootless }`.
- **O `scan()` não espera o Docker.** O comando leva cerca de 5,2 s nesta máquina, mais que o walk inteiro (cerca de 2,2 s). Quem chama roda os dois e depois aplica `ScanResult::apply_docker(usage)`. Rodar os dois juntos deixa o walk mais lento (4 a 5 s), porque o daemon varre `/var/lib/docker` ao mesmo tempo. Por isso o app (Etapa 3) dispara a consulta **depois** do `Done`: a barra fecha em cerca de 2 s e o card Docker chega num evento próprio.
- Os tamanhos vêm como texto em unidades decimais (`"58.68GB"`, `"113.1MB"`, `"0B"`) e são convertidos para bytes. A porcentagem entre parênteses do `Reclaimable` é descartada.
- O Docker relata tamanho lógico, que não é exatamente o espaço alocado. Por isso `apply_docker` limita o valor: `docker_bytes = min(Σ docker, restante)`, e depois `restante −= docker_bytes`. A barra continua somando 100%. Chamar de novo substitui o valor anterior (útil depois de um `docker_prune`).
- **Rootless sem contar duas vezes:** os dados ficam em `~/.local/share/docker`, dentro do `$HOME`. O walk lê o que pertence ao usuário e soma em `rootless_scanned_bytes`, que fica em Apps/Sistema pela regra 1. O que pertence a subuids mapeados não é lido e cai no restante. Por isso, do rootless só sai do restante `total − rootless_scanned_bytes`, limitado ao que sobrou depois do rootful:
  ```
  rootful  = min(Σ rootful, restante)
  rootless = min(Σ rootless − rootless_scanned_bytes, restante − rootful)
  docker_bytes = rootful + rootless
  ```
- Na UI, o Docker é um subitem de Apps/Sistema ("Docker — 97 GB, 75 GB liberáveis"), separado de "Reservado / sem acesso". A nota da legenda soma `docker_bytes + rootless_scanned_bytes`. No detalhe há um card por daemon presente ("Docker" e "Docker rootless"), cada um com o total relatado pelo próprio daemon.
- **Item a item (Etapa 13, DECISIONS D42).** `docker_details(mode, timeout)` roda `docker system df -v --format json` (um único objeto JSON com `Images`, `Containers`, `Volumes` e `BuildCache`) e devolve `DockerDetails`, cada lista do maior para o menor:
  ```rust
  pub struct DockerDetails { images: Vec<DockerImage>, containers: Vec<DockerContainer>, volumes: Vec<DockerVolume>, build_cache: Vec<DockerCacheEntry> }
  DockerImage      { id, repository, tag, size, shared_size, containers, created }
  DockerContainer  { id, name, image, state, size, project, created }
  DockerVolume     { name, size, links, anonymous, project }      // links = containers que o montam
  DockerCacheEntry { id, description, size, in_use, shared, last_used, usage_count }
  ```
  `anonymous` vem da label `com.docker.volume.anonymous` (Docker 23+) ou, antes dela, de um nome que é um id de 64 dígitos hexadecimais; `project` vem da `com.docker.compose.project`. `server_version` vem de `docker version --format {{.Server.Version}}`, no mesmo daemon: as dicas só sugerem `docker volume prune` a partir da 23. Datas (`"2026-07-17 13:22:11 -0300 -03"`) viram segundos Unix, e `"N/A"` vira 0. Não entra no `ScanResult`: é consultado só quando o detalhe abre. Leva o mesmo tempo do `df` simples (cerca de 4,5 s), mas a saída tem cerca de 125 KB, mais que o pipe (64 KiB). Por isso o `run_with_timeout`, usado pelas duas consultas, lê a saída numa thread enquanto espera o processo.

---

## 7. Ponte Tauri (`src-tauri`)

| Command | Assinatura (Rust) | Observação |
|---|---|---|
| `disk_info` | `() -> DiskInfo` | Instantâneo: a UI desenha "Livre" antes mesmo do scan |
| `last_scan` | `() -> Option<ScanResult>` | Último resultado conhecido; ao abrir, o do cache (§9) |
| `start_scan` | `(on_event: Channel<ScanEvent>) -> ()` | `std::thread::spawn`. Recusa se já houver um scan rodando. Depois do `Done`, consulta o Docker e emite `Docker` |
| `cancel_scan` | `() -> ()` | Seta o `AtomicBool` |
| `category_files` | `(cat: Category) -> Vec<FileItem>` | Top-N do último resultado. `FileItem` é o `FileEntry` com `trashable`, calculado pela mesma validação do `move_to_trash` |
| `move_to_trash` | `(paths: Vec<PathBuf>) -> TrashReport` | Validação abaixo. Usa `trash::delete` (padrão freedesktop, restaurável pelo Nautilus). `TrashReport` = movidos, bytes, recusados com o motivo e o `ScanResult` atualizado |
| `empty_trash` | `(confirm: bool) -> PurgeReport` | Exige `confirm == true`. Lista com `trash::os_limited::list` (todas as lixeiras do usuário, como o Nautilus) e apaga **item por item** com `purge_all`, como o `purge_from_trash`: um item que falha volta em `rejected` e não impede os outros (Etapa 14, D44). Tudo apagado zera a Lixeira no resultado (`clear_trash`); senão, só os apagados saem (`record_purged`). O liberado é a diferença do `statvfs` |
| `old_downloads` | `() -> Vec<FileItem>` | Os maiores downloads antigos do último resultado, com `trashable` |
| `largest_files` | `(n: usize) -> Vec<FileItem>` | Junta os top-N de todas as categorias, ordena por tamanho e devolve os `n` primeiros (no máximo 50, `MAX_LARGEST`) que passam no `check_trashable` |
| `trash_caches` | `(paths: Vec<PathBuf>) -> TrashReport` | Move itens inteiros de `~/.cache` para a Lixeira (`trash::delete`). Validação no `check_cache_trashable`, abaixo |
| `trash_items` | `() -> Vec<TrashItem>` | `trash::os_limited::list` (o `.trashinfo` de cada item: nome, pasta original, data) juntado com o tamanho do último scan (`find_trash_item`). Itens que o scan não viu (movidos por fora depois dele, ou de outro disco) são medidos com `tree_size` e vêm com `counted = false`. O `id` é o caminho do `.trashinfo`. Cada item traz `blocked` quando `scanner::locked_dirs` acha pastas que o usuário não consegue esvaziar: o motivo e os comandos para o terminal (abaixo) |
| `restore_from_trash` | `(ids: Vec<String>) -> TrashReport` | Relê a listagem e só aceita `id`s presentes nela. `trash::os_limited::restore_all` um item por vez, que **nunca sobrescreve**: um nome ocupado no local original (`RestoreCollision`) volta recusado com o motivo. `moved` conta os restaurados |
| `purge_from_trash` | `(ids: Vec<String>, confirm: bool) -> PurgeReport` | Exige `confirm == true`. Mesma validação de `id`s, `purge_all` só dos escolhidos e o liberado medido pelo `statvfs`, como o `empty_trash`. `PurgeReport` = apagados, liberado, recusados e o `ScanResult` atualizado. Um item que falha com pastas travadas volta com o motivo e os `commands` do `blocked`; senão, com o erro do crate |
| `reveal` | `(path: PathBuf) -> ()` | Só arquivos (e itens de cache) do último scan (`AppState::contains_file`) ou o caminho guardado de um item que está na Lixeira agora, depois `tauri_plugin_opener::reveal_item_in_dir` (D-Bus `FileManager1.ShowItems` → Nautilus com o arquivo selecionado). `async`, porque a chamada D-Bus bloqueia |
| `docker_details` | `(mode: DockerMode) -> DockerDetails` | Só leitura: `scanner::docker_details` no socket fixo de `mode` (§6.1), com o timeout de 20 s. Erro se o daemon não responder. Não passa pelo `AppState` |
| `docker_prune` | `(mode: DockerMode, target: DockerTarget, confirm: bool) -> Cleanup` | `DockerTarget` = `BuildCache` \| `UnusedImages`. Recusa um `mode` sem daemon no último resultado. Roda o comando fixo no socket de `mode`, sem timeout (um prune grande leva minutos), relê só aquele daemon e devolve como o `empty_trash` |
| `start_monitor` | `(on_event: Channel<MonitorEvent>) -> ()` | Aba Recursos (§11). Sobe a thread do `MonitorState` com um `Sampler` novo: uma `Sample` por segundo até o `stop_monitor` ou outro `start_monitor`. Não é recusado durante um scan |
| `stop_monitor` | `() -> ()` | Encerra a sessão na hora (a thread não espera o tick) |
| `system_info` | `() -> SystemInfo` | Ficha técnica: modelo da CPU, núcleos, threads, RAM, kernel, distribuição e hostname |

Status: todos ✅. `disk_info`, `start_scan`, `cancel_scan` e `category_files` na Etapa 3, `reveal` na 5, as ações de limpeza na 6, as sugestões (`old_downloads`, `largest_files`, `trash_caches`) na 10 os itens da Lixeira (`trash_items`, `restore_from_trash`, `purge_from_trash`) na 12 o `docker_details` na 13 e os do monitor (`start_monitor`, `stop_monitor`, `system_info`) na 16. Os commands que tocam no disco são `async`, para não travar a janela.

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScanEvent {
    Progress(ScanProgress),                       // { kind: "progress", bytes, files }
    Done { result: Box<ScanResult> },             // Box: bem maior que os outros eventos
    Docker { docker: DockerStatus, docker_bytes: u64, unaccounted_bytes: u64 },
    Cancelled,
    Error { message: String },
}
```

Sequência de eventos: `progress`* → (`done` → `docker`) | `cancelled` | `error`.

**Estado (`state.rs`):**

```rust
AppState { last: Mutex<Option<ScanResult>>, cancel: Arc<AtomicBool>, running: AtomicBool, generation: AtomicU64 }
```

- Gerenciado como `Arc<AppState>`, porque a thread do scan precisa dele depois que o command retorna.
- `start_scan(opts, docker, send)` recebe a consulta ao Docker e o destino dos eventos como parâmetros. O command passa `docker_status` e o `Channel`; os testes passam um Docker falso e um `mpsc`. Assim, o ciclo inteiro é testado com `cargo test`, sem janela (DECISIONS D24).
- `running` volta a `false` logo no `Done`. Um novo scan pode começar enquanto o Docker do anterior ainda responde. `generation` garante que esse resultado antigo seja descartado em vez de sobrescrever o novo.
- `cancel` é zerado **antes** do command retornar, então um `cancel_scan` imediatamente depois já vale.

**Ações de limpeza (`state/cleanup.rs`):** são métodos do `AppState` que recebem as operações de sistema (Lixeira, `docker`, `statvfs`) como parâmetros, como o `start_scan`. Os testes usam uma pasta temporária e fakes, sem tocar na Lixeira nem no Docker de verdade.
- Todas são recusadas durante um scan (`running`): o resultado sobre o qual agiriam vai ser substituído.
- Depois de `move_to_trash`, cada item sai do top-N e do total da sua categoria e seus bytes vão para `Other`/`trash_bytes`. **Mover para a Lixeira não libera espaço.**
- Os itens da Lixeira ficam em `state/trash.rs`, com a listagem, a restauração e o apagamento também como parâmetros. Restaurar devolve os bytes à categoria do caminho original; a barra continua somando o mesmo, porque nada saiu do disco.
- **Itens travados (Etapa 14, D44).** Apagar exige `wx` na pasta que contém cada arquivo. `scanner::locked_dirs(path)` percorre o item (sem seguir links nem cruzar filesystems) e conta as pastas com conteúdo onde o usuário não tem isso, pela regra do kernel (dono, grupo ou outros), ou que ele nem consegue ler. `LockedDirs { count, foreign_owner }` guarda o dono da primeira que é de outro usuário. Pastas do root dentro do `$HOME` são o caso comum: um container Docker gravou numa pasta montada do projeto. `blocked()` monta o motivo e os comandos, com os caminhos da listagem do sistema (nunca da UI) entre aspas simples (`sh_quote`):
  - de outro dono: `sudo rm -rf -- '<item>' '<.trashinfo>'` e, se o dono é o root **e** o último scan achou o Docker do sistema (`docker.rootful`), a versão sem sudo, `docker run --rm -v '<Trash/files>:/lixeira' alpine rm -rf -- '/lixeira/<nome>' && rm -- '<.trashinfo>'`. O motivo muda se o usuário não estiver em `sudo`/`admin`/`wheel` (`/etc/group`): o `sudo` precisa da senha de um administrador;
  - do próprio usuário, sem escrita (o cache de módulos do Go): `chmod -R u+w -- '<item>'`, e depois apagar de novo pelo app.
  - nome fora do UTF-8: só o motivo, sem comando.
  O app não roda nenhum desses comandos: ele só os mostra.
- `empty_trash`, `purge_from_trash` e `docker_prune` leem o `statvfs` antes e depois. A diferença é o "liberado" mostrado na UI, e o disco novo entra no resultado por `set_disk`. O lock do resultado não fica preso durante a limpeza.
- Cada ação devolve o `ScanResult` atualizado, e a UI troca o dela por ele (DECISIONS D34).

**Segurança (validação no Rust, não na UI):**
- `move_to_trash` (`check_trashable`), em ordem:
  1. o caminho consta no último `ScanResult`;
  2. `fs::canonicalize` funciona (o arquivo ainda existe) e devolve o próprio caminho, ou seja, nenhum componente virou link simbólico depois do scan;
  3. fica sob o `$HOME` canonicalizado;
  4. **não** casa a regra 1 (`is_app_path`: sistema e apps gerenciados; apps portáteis podem ir para a Lixeira);
  5. ainda não está na Lixeira.

  Caminhos que falham são recusados com o motivo e listados no `TrashReport`; os outros seguem (DECISIONS D33).
- `trash_caches` (`check_cache_trashable`), em ordem: o item consta em `suggestions.caches`; `fs::canonicalize` devolve o próprio caminho; o pai canônico é exatamente `<$HOME canônico>/.cache`. Nunca `~/.cache` inteira, nem algo mais fundo (DECISIONS D39).
- `restore_from_trash` e `purge_from_trash` não recebem caminhos: a UI devolve o `id` que o `trash_items` deu, e o Rust o procura na listagem do sistema relida na hora. Um `id` inventado, repetido ou que já saiu da Lixeira é recusado ("não está mais na Lixeira"); o que vai para o crate é o item da listagem, não o texto da UI (DECISIONS D41).
- Nunca há `remove_file` direto. Os únicos apagamentos permanentes são `empty_trash` e `purge_from_trash` (mais o `docker_prune`), e todos exigem confirmação explícita.
- `docker_prune` só executa comandos fixos, sem argumento vindo da UI: `docker --host <socket fixo> builder prune -f` e `docker --host <socket fixo> image prune -a -f`. A UI só escolhe o modo (um enum); o socket é montado no Rust. Os dados do rootless (`~/.local/share/docker`) e do Podman (`~/.local/share/containers`) são regra 1, então nunca vão para a Lixeira. **Volumes nunca são apagados pelo app**, porque guardam dados (bancos, uploads); eles só aparecem com o tamanho e, no detalhe do Docker, item a item, com dicas de comandos para o usuário rodar (D42).
- **Permissões:** o `build.rs` declara os commands no `AppManifest`. Com isso, o Tauri passa a exigir a permissão `allow-<command>` de cada um, em vez de liberar todos os commands registrados. `capabilities/default.json` lista só `core:default` e essas permissões. O `opener:default` do template foi removido e nenhuma permissão do opener é dada à janela: o `reveal` é um command próprio, que valida o caminho antes de chamar o plugin pelo Rust (DECISIONS D25, D31).
- **Caminhos não UTF-8** (válidos no Linux) são serializados com perdas (`�`) para não derrubar o resultado inteiro (DECISIONS D26). As ações sobre esses arquivos são recusadas, já que o caminho recebido de volta não é o real: ele não consta no último scan. A UI também desabilita o checkbox deles.

---

## 8. Frontend (`src/`)

| Componente | Responsabilidade |
|---|---|
| `routes/+page.svelte` | Abas Armazenamento e Recursos (`TabSwitch`): a de fora fica escondida, não desmontada, e o `Esc` na aba Recursos não chega aos detalhes escondidos (captura na janela). A aba escolhida fica no `localStorage`. Orquestra o estado (`idle` / `scanning` / `done` / `cancelled` / `error`), monta os segmentos e alterna entre a tela principal e o detalhe. A tela principal fica escondida, não desmontada, para a barra não refazer a animação na volta. Escaneia ao abrir |
| `StorageBar.svelte` | Barra única, em ordem fixa: as 6 categorias, a faixa "ainda não contado" e "Livre" na ponta direita. Cada largura é um `Tween` (600 ms, `cubicOut`) redirecionado a cada `progress`. Vãos de 2px com a cor do card separam os segmentos. Clicar num segmento abre o detalhe. Hover ou foco mostra um tooltip com nome, tamanho e porcentagem, inclusive em "Livre" |
| `Legend.svelte` | Linhas com bolinha de cor, nome, tamanho e porcentagem do disco, ordenadas por bytes (`animate:flip`) e com "Livre" por último. Apps/Sistema traz a composição: Docker e reservado/sem acesso |
| `CategoryDetail.svelte` | Aberto ao clicar num segmento da barra ou numa linha da legenda (só com um resultado completo). Mostra o total, a porcentagem e a contagem, e lista o top-N de `category_files`, ordenado por tamanho ou por data. Em Outros aparece o card "Lixeira", com "Ver itens" (abre o `TrashDetail`); em Apps/Sistema, um card por daemon, "Docker" e "Docker rootless" (imagens, containers, volumes e build cache, cada um com tamanho, quantidade e liberável; some sem aquele daemon) e "Reservado / sem acesso". Voltar: botão ou `Esc`. Checkbox nos arquivos com `trashable`, barra fixa no rodapé com o total selecionado e "Mover para a Lixeira". Card Lixeira com "Esvaziar Lixeira"; cada card Docker com "Ver detalhes" (abre o `DockerDetail`), "Limpar build cache" e "Remover imagens não usadas" do seu daemon, sem botão para volumes. As ações destrutivas passam pelo `ConfirmDialog`, com o tamanho no título. O resultado aparece num aviso: o que foi movido ou liberado, e os recusados com o motivo |
| `DockerDetail.svelte` | Aberto pelo "Ver detalhes" de um card Docker, por cima do detalhe de Apps/Sistema: voltar (botão ou `Esc`) retorna a ele. Chama `docker_details(mode)` ao abrir e a cada scan, com "Consultando o Docker…" enquanto isso. Abas Volumes, Imagens, Containers e Build cache, com quantidade e total; abre na que mais ocupa. Cada linha tem nome (volume anônimo e imagem sem nome pelo id curto, com selo), uma linha de contexto (sem uso / em uso por N, projeto do Compose, estado do container em pt-BR, último uso do cache) e o tamanho; os 50 maiores e "Mostrar todos". Acima da lista, o card "Como liberar espaço" monta dicas a partir dos itens (anônimos sem uso, volumes com nome sem uso, imagens sem container ou sem nome, containers parados, cache sem uso há mais de 30 dias), cada uma com o comando e "Copiar". No rootless, os comandos levam `DOCKER_HOST=…`. Sem ações próprias (D42) |
| `CommandLine.svelte` | Um comando para o terminal, selecionável (o resto da página tem `user-select: none`), com "Copiar" (`navigator.clipboard`). Usado nas dicas do `DockerDetail` e nos itens travados e recusados da Lixeira |
| `notice.ts` | Tipos do aviso das telas de detalhe (`Notice`, com `details` que podem ter `commands`) e `purgeNotice(report, emptied)`, a mensagem de "apagados / não puderam ser apagados" usada pelo `TrashDetail` e pelo card Lixeira do `CategoryDetail` |
| `ConfirmDialog.svelte` | `<dialog>` modal. Esc cancela |
| `FileRow.svelte` | Nome, pasta abreviada (`~/…`, via `homeDir()`), tamanho, data relativa e o botão "Abrir na pasta". Desabilitado para nomes não UTF-8, que chegam com `�` (D26) |
| `Suggestions.svelte` | Card "Sugestões" abaixo da barra, só com um resultado completo. Uma linha por sugestão com algo a mostrar: "Downloads antigos" (arquivos e bytes do resultado), "Caches" (pastas de `~/.cache`) e "Maiores arquivos" (`largest_files(10)`, recarregado a cada resultado novo). Clicar abre o detalhe |
| `SuggestionDetail.svelte` | Mesma navegação do `CategoryDetail` (voltar, `Esc`, avisos, rodapé de seleção). Downloads antigos e maiores arquivos usam `FileRow` e `move_to_trash`. Caches listam pastas (nome, `~/.cache/…`, arquivos, tamanho, "abrir na pasta"), e "Mover para a Lixeira" pede confirmação no `ConfirmDialog` antes do `trash_caches` |
| `TrashCard.svelte` | Card "Lixeira" na tela principal, entre a barra e as sugestões: `trash_bytes` e a contagem de `trash_items`. Vazia, mostra "Lixeira vazia" e não abre nada. Clicar abre o `TrashDetail` |
| `TrashDetail.svelte` | Mesma navegação do `CategoryDetail`. Lista `trash_items()` (recarregada a cada ação e a cada scan): nome, pasta original (`~/…`), arquivos (se for pasta), tamanho e "apagado há X", ordenável por tamanho ou por data, com "abrir na pasta" no caminho guardado. Um tamanho medido agora (fora do scan) aparece com `*` e uma nota. "Esvaziar Lixeira" no topo (reaproveita o `empty_trash`); no rodapé da seleção, "Restaurar" e "Apagar", este pelo `ConfirmDialog` com o tamanho no título. Itens com `blocked` ganham o selo "travado", e um card recolhido (`<details>`) acima da lista convida a abrir e mostra cada um com o motivo e os comandos. O "Esvaziar Lixeira" avisa na confirmação que eles vão ficar. Os recusados de qualquer ação aparecem no aviso com os comandos, quando houver |
| `TabSwitch.svelte` | Controle segmentado centralizado no topo (Armazenamento \| Recursos), estilo iOS: o fundo desliza para a aba escolhida. `role="tablist"`, setas, Home e End |
| `ResourcesView.svelte` | Aba Recursos (§11): dona do ciclo do monitor e do histórico de 60 amostras. Cards Processador, Memória, Disco e Rede em grade (`auto-fill`, 1 ou 2 colunas) |
| `MetricCard.svelte` | Card da aba Recursos: título, selo opcional, valor grande, subtítulo e o conteúdo |
| `Sparkline.svelte` | Gráfico de linha em SVG puro dos últimos 60 pontos, uma ou duas séries, com área. Escala fixa (`max`) ou automática com teto 1/2/5 × 10ⁿ e mínimo `floor`. `viewBox` distorcido com `vector-effect: non-scaling-stroke`: traço nítido em qualquer largura e em HiDPI |
| `api.ts` | `startScan(onEvent)` com `new Channel<ScanEvent>()` e wrappers tipados |
| `categories.ts` | Nome (espelha `Category::label`) e token de cor de cada categoria, mais o tipo `Segment` usado pela barra e pela legenda |
| `format.ts` | Unidades decimais (1 GB = 10⁹ B, como GNOME/iOS) com 3 algarismos significativos e casas fixas, porcentagem, contagem e segundos, tudo via `Intl.NumberFormat('pt-BR')` |

**Barra durante o scan (DECISIONS D29):**
- Antes do scan, `disk_info` já desenha "Livre"; o resto da barra é a faixa listrada.
- A cada `progress`, as categorias crescem da esquerda, e a faixa listrada (animada) encolhe.
- No `done`, Apps/Sistema recebe `docker_bytes + unaccounted_bytes` e a faixa some: a barra fecha 100%. O evento `docker` só divide o valor entre os dois, sem mudar a barra.
- A largura é `bytes / max(total, Σ segmentos + livre)`, então a soma nunca passa de 100%.
- "Reescanear" relê `disk_info` e zera a barra. Cancelar volta ao último resultado completo, com o disco dele (D30).

**Tema (`theme.css`)**:

| Token | Cor |
|---|---|
| `--cat-photos` | `#FFB800` |
| `--cat-videos` | `#FF375F` |
| `--cat-audio` | `#FF9F0A` |
| `--cat-documents` | `#0A84FF` |
| `--cat-apps` | `#8E8E93` |
| `--cat-other` | `#BF5AF2` |
| `--track` (Livre) | `#E5E5EA` (claro) / `#3A3A3C` (escuro) |
| `--track-stripe` (não contado) | `#D1D1D6` (claro) / `#48484A` (escuro) |

- Claro e escuro via `prefers-color-scheme`. Fundo e card como no iOS: `#F2F2F7`/`#FFFFFF` no claro e `#000000`/`#1C1C1E` no escuro.
- `prefers-reduced-motion` zera as animações.
- Troca de tela: o detalhe entra pela direita e a tela principal volta pela esquerda (0,28 s, `--ease`), com animações CSS que recomeçam quando o elemento sai do `display: none`.
- Fonte do sistema: `system-ui, "Ubuntu", "Cantarell"`.
- `--radius: 14px`, cards com `box-shadow` suave e transições de 0,6 s com easing "iOS" (`cubic-bezier(.2,.8,.2,1)`).

---

## 9. Persistência (Etapa 7)

O último `ScanResult` é salvo em `$XDG_CACHE_HOME/storage-lkr/last.json` (normalmente `~/.cache/…`) por `state/cache.rs`:
- **Quando:** a cada mudança do resultado no `AppState`: no `done`, no `docker` e depois de cada ação de limpeza. A escrita vai para um `.tmp` e é renomeada, com o lock do resultado em mãos.
- **Formato:** `{ version, home, result }`. O `home` vai à parte porque o `ScanResult` não o serializa, e as ações validam contra ele. Um arquivo corrompido ou de outra `version` é ignorado.
- **Ao abrir:** `AppState::with_cache` carrega o arquivo, e o command `last_scan` o entrega à UI. A tela aparece na hora com esse resultado, e um scan começa **em segundo plano**: a barra fica no resultado do cache, e o status mostra "Atualizando… · análise de há 2 h". No `done`, a barra anima do valor antigo para o novo, sem passar pelo zero (DECISIONS D35).
- Com o resultado do cache, o detalhe já funciona. As ações são recusadas até o scan terminar (`running`), e a validação do `move_to_trash` cobre arquivos que sumiram desde então.

## 10. Fora do MVP

- Detalhar Apps por Snap, Flatpak e APT (`snap list`, `flatpak list --columns=size`, `dpkg-query`).
- Atualização incremental com inotify ou fanotify.
- Modo privilegiado via `pkexec` para zerar o "sem acesso".
- Podman (`podman system df`). Hoje `~/.local/share/containers` só é marcado como app (regra 1), sem card próprio.

---

## 11. Monitor de recursos (V2)

Plano em [PLAN-V2.md](PLAN-V2.md). O crate `crates/monitor` é Rust puro, sem Tauri e sem relação com o `scanner`, e lê direto de `/proc` e `/sys` (DECISIONS D45).

**`Sampler`:**
- `Sampler::new(root)` recebe a raiz onde estão `proc/` e `sys/`: `/` no sistema (`Sampler::system()`), uma pasta temporária com uma árvore falsa nos testes (`fixture.rs`).
- `sample()` lê os contadores, compara com os da amostra anterior e devolve uma `Sample`. As taxas usam o tempo real decorrido (`Instant`), não o intervalo pedido, e vêm `None` na primeira amostra.
- Só falha sem `/proc/stat` ou `/proc/meminfo`. Os outros arquivos, se faltarem (container, VM, kernel antigo), deixam a parte deles vazia.
- Os arquivos do `/proc` são lidos de uma vez num buffer reaproveitado (`read_into`). O `fs::read_to_string` faria um `fstat`, começaria com 32 bytes e leria pedaço por pedaço, porque o procfs informa tamanho 0.
- Contador que volta (interface que caiu e voltou, contador de 32 bits que deu a volta) conta como zero, para não desenhar um pico falso.

**Fontes (Etapa 15):**

| Métrica | Fonte | Regra |
|---|---|---|
| Uso da CPU, total e por núcleo | `/proc/stat` | `1 − Δ(idle + iowait) / Δtotal`, com os 8 primeiros campos (o `guest` já está em `user`). O iowait conta como ocioso, como no `top` e no GNOME. Núcleo desligado não tem linha e não aparece |
| Frequência | `cpufreq/scaling_cur_freq` de cada núcleo | Média e máxima. Se há `cpufreq` é decidido na primeira amostra; sem ele (VM, alguns ARM), `None` na sessão inteira, sem procurar os arquivos de novo. O `cpu MHz` do `cpuinfo` de uma VM é fixo e vai só para a ficha técnica, como nominal (D55) |
| Memória | `/proc/meminfo` | Usada = `MemTotal − MemAvailable`, como o `free`. Sem `MemAvailable` (kernel < 3.14), `MemFree + Buffers + Cached` |
| Swap | `/proc/swaps` | Só swap em disco: soma de `Size` e `Used` (KiB) das linhas que não são `/dev/zram*`, que já aparecem no zRAM (D54). Sem o arquivo (kernel sem swap), `SwapTotal − SwapFree` do `meminfo` |
| zRAM | `/sys/block/zram*/{disksize,mm_stat}` | Soma dos dispositivos com `disksize > 0`: dados originais, comprimidos e memória ocupada |
| Pressão de memória | `/proc/pressure/memory` | `avg10` das linhas `some` e `full`; `None` sem PSI |
| Disco | `/proc/diskstats` | Só discos físicos inteiros: em `/sys/block` (sem partições), com `device/`, fora `loop`, `ram`, `zram`, `sr`, `fd`, `nbd`, `dm-*`, `md*` e as partições de boot/RPMB do eMMC. Bytes = setores × 512. Ocupado = Δ`io_ticks`, como o `%util` do `iostat`. Tipo pelo nome (`nvme`, `mmcblk`, `vd`/`xvd`, `sd`) e USB pelo link do sysfs; modelo de `device/model` ou `device/name`. Os metadados ficam em cache por nome até ele sumir do `diskstats` |
| Rede | `/proc/net/dev` | Só interfaces com `device/` (fora `lo`, bridges, `veth`, VPN). Wi-Fi por `wireless/` ou `phy80211/`. Total da sessão = soma das diferenças desde a primeira amostra. Também em cache por nome |
| Uptime | `/proc/uptime` | Primeiro campo |
| Temperatura da CPU (Etapa 17) | `/sys/class/hwmon` | Procurada uma vez, na primeira amostra, nesta ordem: `coretemp` (`Package id`), `k10temp`/`zenpower` (`Tdie`, senão `Tctl`), `cpu_thermal` (ARM), a zona `x86_pkg_temp` de `/sys/class/thermal` e, por último, o `acpitz` da placa, marcado como aproximado (`~`). A busca lê só `name` e `temp*_label`, nunca um `temp*_input` de outro chip (ler o de uma GPU dedicada em repouso a acordaria) |
| Temperatura do disco | `/sys/class/hwmon` (`nvme`, `drivetemp`) | O chip cujo `device` canônico é o mesmo de `/sys/block/<disco>/device`; no NVMe, o `Composite`. Procurada junto com os metadados do disco. SATA sem sensor e sem `/sys/module/drivetemp` (e que não é disco virtual pelo `device/vendor`) ganha a dica `sudo modprobe drivetemp`, que o app só mostra |
| Limiar de alerta | `temp*_max`, senão `temp*_crit` | 10 °C abaixo do limite do sensor, sem passar de 85 °C (CPU) ou 70 °C (disco). O `max` do `coretemp` é o TjMax (100 °C), alto demais para avisar |
| GPU, descoberta (Etapa 18) | `/sys/class/drm/card[0-9]*/device/uevent` | Uma vez, na primeira amostra. `DRIVER`, `PCI_ID`, `PCI_SUBSYS_ID` e `PCI_SLOT_NAME`; sem conectores, `renderD*`, framebuffers do firmware (`simple-framebuffer`, `efidrm`…) e monitores USB (`evdi`, `udl`, `gud`). Nome pelo `pci.ids` (`/usr/share/misc` ou `/usr/share/hwdata`), lido direto: o comercial entre colchetes, ou o do subsistema quando começa com a mesma palavra. Integrada: `amdgpu` com `in1_input` (`vddnb`) no hwmon, `i915`/`xe` sem hwmon, fora do PCI (ARM); GPU virtual não é nem uma nem outra. Lê só o que não acorda uma placa em D3cold (D49) |
| GPU, estado | `device/power/runtime_status` | A cada amostra. `suspended`/`suspending`: "em repouso", e nada mais é lido |
| GPU AMD (`amdgpu`) | `device/` e o hwmon dele | `gpu_busy_percent`, `mem_info_vram_used` (o total na descoberta), `mem_info_gtt_*` só nas APUs; do hwmon, `temp*_input` (`edge`), `power1_average` ou `power1_input` (µW) e `freq1_input` (Hz). Consumo acima de `power1_cap_max`/`power1_cap` × 1,25 é descartado |
| GPU NVIDIA proprietária | `nvidia-smi --query-gpu=… -lms 1000` | Um processo de vida longa, lido numa thread; cada linha liga à placa pelo `pci.bus_id`. Só roda com uma NVIDIA ativa e fora da pausa; morre no `Drop`. Sem o programa (`/usr/bin` ou `/usr/local/bin`): `no_nvidia_smi`. Se ele sai sozinho, não sobe de novo na sessão |
| GPU Intel (`i915`/`xe`) | `card*/gt_act_freq_mhz` / `device/tile0/gt0/freq0/act_freq` | Frequência atual e máxima (`gt_max_freq_mhz`, senão `RP0`). Uso e consumo exigem root: `needs_root` |
| Outras GPUs (`nouveau`, `radeon`, virtuais, ARM) | hwmon do `device/`, se houver | Só temperatura e consumo; `not_exposed` |
| GPU, pausa | — | Dedicada com `power/control = auto`: depois de 3 leituras sem uso, nenhuma leitura por `autosuspend_delay_ms + 3 s` (a UI mantém a última), para a placa poder dormir. Ainda ativa depois disso: a pausa se desliga até ela dormir |
| Processos (Etapa 19) | `/proc/[pid]/stat`, `/proc/[pid]/io` | A cada 2 amostras (`PROCS_EVERY`), todos os processos, sem threads do kernel (`PF_KTHREAD`) nem zumbis. Do `stat` (campos depois do último `)`, porque o `comm` pode ter espaços e parênteses): `comm`, pai, `utime + stime`, início e RSS. CPU = Δticks / `CLK_TCK` / Δt, em % de um núcleo; processo novo conta tudo o que usou. Disco pelo `io`, só nos do próprio usuário (os outros falham uma vez e não são tentados de novo); com o tempo de CPU igual ao da leitura anterior, o `io` não é relido e a taxa é zero. Cada arquivo com um `read` só |
| Grupos de processos | o `stat` acima e `/proc/[pid]/cmdline` | Um processo entra no grupo do pai quando o pai tem o mesmo `comm`, até a raiz. O nome da raiz vem do `.desktop` (pelo `argv[0]`: caminho real, nome do arquivo ou pasta própria do app), e grupos do mesmo `.desktop` se juntam; sem ele, o nome do executável. Os `.desktop` (do usuário, `/usr/share`, `/usr/local/share`, snap, flatpak) são lidos uma vez, na primeira leitura; o `cmdline`, uma vez por grupo. Três listas de até 5: CPU, memória (soma do RSS) e disco (só quem leu ou gravou) |
| Ficha técnica | `system_info()` | Lida uma vez, ao abrir a aba: modelo da CPU (ou da placa ARM pelo devicetree), núcleos físicos (pares soquete/núcleo do `cpuinfo`), threads, RAM, kernel (`/proc/sys/kernel/osrelease`), distribuição (`os-release`), hostname, a frequência nominal (o `cpu MHz` do `cpuinfo`, só sem `cpufreq`) e os discos físicos com tamanho (`size` × 512) e as GPUs (`GpuSpec`: nome, fabricante, driver, `vendor:device`, integrada ou dedicada, VRAM) |

**CLI (`bin/monitor.rs`):** `monitor` imprime uma linha por segundo até o Ctrl+C. `--interval MS`, `--count N` (no fim, o custo médio e o pior de uma amostra e a CPU do próprio processo), `--once` (uma amostra com taxas), `--json` (uma `Sample` por linha), `--cores` (uso por núcleo) e `--procs` (os três "Top 5" a cada leitura de processos e, no fim, quanto ela custa: a média das amostras com processos menos a das sem).

**Ponte (Etapa 16, `src-tauri/src/state/monitor.rs`):**
- `MonitorState { generation: Mutex<u64>, wake: Condvar }`, gerenciado como `Arc` à parte do `AppState`: não divide lock com o scan e roda durante ele.
- `start(interval, sample, send)` incrementa a geração e sobe uma thread. A primeira amostra sai na hora (sem taxas) e as outras a cada `interval`, numa cadência fixa que não acumula atraso. Entre os ticks, a thread espera no `Condvar` enquanto a geração for a dela: `stop()` ou outro `start()` incrementam a geração, acordam a thread, e ela emite `stopped` e sai na hora.
- `send` devolve `false` quando o `Channel` falha (janela fechada ou recarregada), e a thread sai sem esperar um `stop`. Um erro do `Sampler` (sem `/proc/stat`) vira `error` e encerra a sessão.
- A amostra e o destino são parâmetros: os testes usam um amostrador falso e um `mpsc`, como o scan (D24).

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
enum MonitorEvent {
    Sample { sample: Box<Sample> },
    Stopped,
    Error { message: String },
}
```

**Interface (`ResourcesView.svelte`):**
- Coleta só quando a aba está ativa, a página visível (`visibilitychange`) e a janela não minimizada (`isMinimized()`, conferido a cada `onResized` e `onFocusChanged`, porque o WebKitGTK nem sempre marca a página como oculta ao minimizar). Perder o foco não pausa (D46).
- Um `$effect` sobre essas três condições chama `startMonitor` e, ao sair, `stopMonitor`. Cada sessão tem um id; eventos de uma sessão antiga são ignorados.
- O histórico é um anel de 60 amostras no frontend e recomeça a cada sessão (D47).
- Cards:
  - **Processador:** uso total, modelo, núcleos, threads e frequência média; gráfico de 0 a 100% e uma mini-barra por thread, que quebram em linhas (de 2 a 64 threads).
  - **Memória:** usada, total e %; gráfico de 0 a 100%; disponível, swap em disco (ou "sem swap") e zRAM com a taxa de compressão; com zRAM, a linha do swap se chama "Swap em disco" (D54). Selo "Sob pressão" com PSI `some avg10` acima de 10%.
  - **Disco:** leitura e gravação somadas dos discos físicos, num gráfico com escala automática (mínimo 1 MB/s); por disco, modelo, nome, tipo, SSD/HD, taxas e % ocupado.
  - **Rede:** download e upload num gráfico (mínimo 100 kB/s), o total desde que a aba abriu e cada interface (Wi-Fi ou cabo).
  - **GPU** (`GpuCard.svelte`, só com placa): uma por vez, com abas pequenas no título quando há mais de uma ("Intel · integrada", "AMD · dedicada"), começando na primeira acordada. Uso (ou "Em repouso") e gráfico de 0 a 100% (sem uso, como na Intel, a frequência em % da máxima); memória de vídeo (ou "usa a RAM"), memória compartilhada (GTT) nas APUs, frequência, temperatura e consumo. No lugar do que falta, o motivo ("exige root", "não exposto pelo driver"); com a placa em repouso, só uma explicação, sem números. Dicas para a Intel (root) e para a NVIDIA sem `nvidia-smi`.
  - **Processos** (`TopProcesses.svelte`): total de processos e os 5 aplicativos que mais usam CPU, memória ou disco (`MiniTabs` no título), com "×N" e uma mini-barra: CPU contra um núcleo (ou o processador inteiro, conforme a escolha), memória contra a RAM, disco contra o maior da lista. A troca de posição anima com `animate:flip`, menos com `prefers-reduced-motion`. A lista é a da última amostra que trouxe processos. O critério e a escala da CPU ficam no `localStorage`. Só leitura.
  - **Ficha técnica** (`SystemInfoCard.svelte`): ocupa a linha inteira do grid (`MetricCard wide`), com as linhas em duas colunas quando cabem (D56). Distribuição, hostname, processador, núcleos e threads, memória, cada disco (modelo, tamanho, tipo), cada GPU (nome, integrada ou dedicada, driver, VRAM; sem nome, o `vendor:device`), kernel e o tempo ligado, atualizado a cada amostra.
- Temperaturas (`TempValue.svelte`) na linha do Processador e em cada disco: em vermelho a partir do limiar do sensor, `~` quando aproximada, "—" sem sensor. A dica do `drivetemp` aparece uma vez, abaixo da lista de discos, com o `CommandLine` (copiar).
- O cabeçalho mostra hostname e distribuição.

