# ARCHITECTURE — Armazenamento (Ubuntu, estilo iOS/Android)

## 1. Visão geral

```
┌──────────────────────────── Frontend (Svelte 5, WebKitGTK) ───────────────────────────┐
│  App.svelte ─ Header (disco) ─ StorageBar (7 segmentos) ─ Legend ─ CategoryDetail     │
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
├── Cargo.toml                 # workspace: ["crates/scanner", "src-tauri"]
├── package.json  vite.config.ts  tsconfig.json  index.html
├── crates/scanner/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # API pública: scan(), disk_info(), tipos
│       ├── model.rs           # Category, FileEntry, ScanResult, ScanProgress
│       ├── disk.rs            # statvfs → DiskInfo
│       ├── walk.rs            # walk paralelo + agregação
│       ├── classify.rs        # regras caminho/extensão → Category
│       └── bin/scan.rs        # CLI p/ benchmark e validação
├── src-tauri/
│   ├── Cargo.toml  tauri.conf.json  build.rs
│   ├── capabilities/default.json
│   └── src/{main.rs, commands.rs, state.rs}
└── src/
    ├── App.svelte  main.ts  theme.css
    └── lib/
        ├── StorageBar.svelte  Legend.svelte  CategoryDetail.svelte  FileRow.svelte
        ├── api.ts             # wrappers tipados de invoke/Channel
        └── format.ts          # bytes → "12,4 GB" (decimal, pt-BR)
```

O motor fica num crate separado de propósito: pode ser testado com `cargo test` e medido com o CLI sem abrir a janela.

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
    unaccounted_bytes: u64,               // "Reservado / sem acesso" → somado em AppsSystem
    denied_dirs: u64,
    elapsed_ms: u64,
}
```

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

**Meta de desempenho** (SSD, 4 núcleos, cache quente): `/` com cerca de 1 milhão de entradas em **menos de 5 s**. O primeiro scan com cache frio pode levar mais.

---

## 5. Classificação (`classify.rs`)

Função pura: `fn classify(path: &Path, home: &Path) -> Category`. As regras são avaliadas em ordem e **a primeira que casar vence**:

| # | Regra | Categoria |
|---|---|---|
| 1 | Prefixos de sistema/app: `/usr`, `/opt`, `/boot`, `/etc`, `/bin`, `/sbin`, `/lib*`, `/var/lib/{snapd,flatpak,apt,dpkg}`, `/var/cache/apt` e, em `$HOME`, `~/snap`, `~/.var/app`, `~/.local/share/{flatpak,Steam}` | **Apps/Sistema** |
| 2 | `~/.local/share/Trash/**` | **Outros** (conta também em `trash_bytes`) |
| 3 | Dentro de `$HOME` com qualquer componente oculto (`.cache`, `.mozilla`, `.config`…) | **Outros** |
| 4 | Dentro de `$HOME`, visível, extensão (minúscula) na tabela abaixo | Fotos / Vídeos / Áudio / Documentos |
| 5 | Dentro de `$HOME`, visível, outra extensão (zip, iso, código, sem extensão) | **Outros** |
| 6 | Fora de `$HOME` e não coberto acima (`/var/log`, `/srv`, `/root`…) | **Apps/Sistema** |

A regra 1 vem antes da extensão para que um `.png` de ícone em `/usr/share` não vire "Fotos". A regra 3 impede que os JPGs do cache do navegador inflem "Fotos".

**Tabela de extensões** (um `match` estático, sem alocação):

| Categoria | Extensões |
|---|---|
| Fotos | jpg jpeg png gif webp heic heif avif bmp tif tiff svg raw dng cr2 cr3 nef arw orf rw2 |
| Vídeos | mp4 m4v mkv mov avi webm wmv flv mpg mpeg 3gp ts |
| Áudio | mp3 flac ogg oga opus m4a aac wav aiff wma |
| Documentos | pdf doc docx odt rtf txt md xls xlsx ods csv ppt pptx odp epub |

Não há leitura de conteúdo (magic bytes) no MVP, porque custaria um `open` + `read` por arquivo.

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

---

## 7. Ponte Tauri (`src-tauri`)

| Command | Assinatura (Rust) | Observação |
|---|---|---|
| `disk_info` | `() -> DiskInfo` | Instantâneo: a UI desenha "Livre" antes mesmo do scan |
| `start_scan` | `(on_event: Channel<ScanEvent>) -> ()` | `std::thread::spawn`. Recusa se já houver um scan rodando |
| `cancel_scan` | `() -> ()` | Seta o `AtomicBool` |
| `category_files` | `(cat: Category) -> Vec<FileEntry>` | Top-N do último resultado |
| `move_to_trash` | `(paths: Vec<PathBuf>) -> TrashReport` | Validação abaixo |
| `empty_trash` | `(confirm: bool) -> u64` | Exige `confirm == true` e devolve os bytes liberados |
| `reveal` | `(path: PathBuf) -> ()` | `tauri_plugin_opener::reveal_item_in_dir` |

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScanEvent { Progress { bytes: HashMap<Category,u64>, files: u64 }, Done { result: ScanResult }, Error { message: String } }
```

**Estado (`state.rs`):** `AppState { last: Mutex<Option<ScanResult>>, cancel: Arc<AtomicBool>, running: AtomicBool }`.

Depois de `move_to_trash`, cada item sai do top-N e do total da sua categoria e seus bytes vão para `Other`/`trash_bytes`. **Mover para a Lixeira não libera espaço**; só `empty_trash` libera, e aí `livre` é recalculado pelo `statvfs`.

**Segurança (validação no Rust, não na UI):**
- `move_to_trash`: o caminho é canonicalizado (`fs::canonicalize`) e precisa estar sob `$HOME`, **não** casar a regra 1 (apps) e constar no último `ScanResult`. Caminhos que falham são rejeitados e listados no `TrashReport`.
- Nunca há `remove_file` direto. O único apagamento permanente é `empty_trash`, que exige confirmação explícita.
- `capabilities/default.json` libera só os commands acima e `opener:allow-reveal-item-in-dir`.

---

## 8. Frontend (`src/`)

| Componente | Responsabilidade |
|---|---|
| `App.svelte` | Orquestra o estado (`idle` / `scanning` / `done` / `error`) e a navegação entre a tela principal e o detalhe |
| `StorageBar.svelte` | Barra única. Cada segmento usa `flex-basis` de `tweened(pct)`, com cantos arredondados só nas pontas e "Livre" como trilha neutra. Clicar abre o detalhe |
| `Legend.svelte` | Linhas com bolinha de cor, nome e tamanho, ordenadas por bytes, mais a porcentagem |
| `CategoryDetail.svelte` | Lista dos maiores arquivos com checkbox, "Abrir na pasta" e "Mover para Lixeira", e um rodapé com o total selecionado. Em Outros aparece o card "Lixeira — X GB · Esvaziar"; em Apps/Sistema, o card "Reservado / sem acesso" |
| `FileRow.svelte` | Nome, caminho abreviado (`~/…`), tamanho e data relativa |
| `api.ts` | `startScan(onEvent)` com `new Channel<ScanEvent>()` e wrappers tipados |
| `format.ts` | Unidades decimais (1 GB = 10⁹ B, como GNOME/iOS) e `Intl.NumberFormat('pt-BR')` |

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

- Claro e escuro via `prefers-color-scheme`.
- Fonte do sistema: `system-ui, "Ubuntu", "Cantarell"`.
- `--radius: 14px`, cards com `box-shadow` suave e transições de 0,6 s com easing "iOS" (`cubic-bezier(.2,.8,.2,1)`).

---

## 9. Persistência (Etapa 7)

O último `ScanResult` é salvo em `~/.cache/storage-lkr/last.json`. Ao abrir, o app mostra esse resultado na hora, com a marca "há 2 h", e dispara um novo scan em background.

## 10. Fora do MVP

- Detalhar Apps por Snap, Flatpak e APT (`snap list`, `flatpak list --columns=size`, `dpkg-query`).
- Cards de sugestão ("Downloads > 90 dias", "Caches", "Maiores arquivos").
- Atualização incremental com inotify ou fanotify.
- Modo privilegiado via `pkexec` para zerar o "sem acesso".
- Classificação por magic bytes para arquivos sem extensão.
