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
├── Cargo.toml                 # workspace: ["crates/scanner", "src-tauri"]
├── package.json  vite.config.js  svelte.config.js  tsconfig.json
├── crates/scanner/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # API pública: scan(), disk_info(), tipos
│       ├── model.rs           # Category, FileEntry, ScanResult, ScanProgress
│       ├── disk.rs            # statvfs → DiskInfo
│       ├── walk.rs            # walk paralelo + agregação
│       ├── classify.rs        # regras caminho/extensão → Category
│       ├── docker.rs          # `docker system df` → DockerUsage (opcional)
│       └── bin/scan.rs        # CLI p/ benchmark e validação
├── src-tauri/
│   ├── Cargo.toml  tauri.conf.json
│   ├── build.rs               # lista de commands → permissões allow-<command>
│   ├── capabilities/default.json
│   └── src/
│       ├── main.rs  lib.rs    # lib.rs: registra AppState e os commands
│       ├── commands.rs        # commands finos, só repassam ao AppState
│       └── state.rs           # AppState + ScanEvent: ciclo do scan, testável sem Tauri
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
    docker: Option<DockerUsage>,          // preenchido depois, por apply_docker (§6.1)
    docker_bytes: u64,                    // parte do restante atribuída ao Docker
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

`Classifier::new(home)` monta uma vez os caminhos derivados do `$HOME`; `classifier.classify(path) -> Category` é pura e roda nas threads do walk. Os prefixos são comparados por bytes e respeitam componentes inteiros (`/usrdata` não é `/usr`). As regras são avaliadas em ordem e **a primeira que casar vence**:

| # | Regra | Categoria |
|---|---|---|
| 1 | Prefixos de sistema e de apps gerenciados: `/usr`, `/opt`, `/boot`, `/etc`, `/bin`, `/sbin`, `/lib*`, `/var/lib/{snapd,flatpak,apt,dpkg}`, `/var/cache/apt` e, em `$HOME`, `~/snap`, `~/.var/app`, `~/.local/share/{flatpak,Steam}` | **Apps/Sistema** (bloqueia limpeza) |
| 2 | `~/.local/share/Trash/**` | **Outros** (conta também em `trash_bytes`) |
| 3 | **App portátil** no `$HOME`: arquivo `*.AppImage`; primeiro componente `Applications`, `Apps`, `Aplicativos` ou `bin`; ou o par de componentes `resources/app`, `resources/app.asar` ou `resources/app.asar.unpacked` (Electron) | **Apps/Sistema** (pode ser limpo) |
| 4 | Dentro de `$HOME` com qualquer componente oculto (`.cache`, `.mozilla`, `.config`…) | **Outros** |
| 5 | **Build/dev**: componente `node_modules`, `__pycache__` ou `site-packages`, ou o par `target/debug` / `target/release` (Cargo) | **Outros** |
| 6 | Dentro de `$HOME`, visível, extensão (minúscula) na tabela abaixo | Fotos / Vídeos / Áudio / Documentos |
| 7 | Dentro de `$HOME`, visível, outra extensão (zip, iso, código, sem extensão) | **Outros** |
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

- `docker_usage(timeout)` roda o comando com timeout de 20 s (`DEFAULT_DOCKER_TIMEOUT`). Se o binário não existir, o daemon estiver parado ou faltar permissão, o resultado é `None` e o app segue sem Docker, sem erro.
- **O `scan()` não espera o Docker.** O comando leva cerca de 5,2 s nesta máquina, mais que o walk inteiro (cerca de 2,2 s). Quem chama roda os dois e depois aplica `ScanResult::apply_docker(usage)`. Rodar os dois juntos deixa o walk mais lento (4 a 5 s), porque o daemon varre `/var/lib/docker` ao mesmo tempo. Por isso o app (Etapa 3) dispara a consulta **depois** do `Done`: a barra fecha em cerca de 2 s e o card Docker chega num evento próprio.
- Os tamanhos vêm como texto em unidades decimais (`"58.68GB"`, `"113.1MB"`, `"0B"`) e são convertidos para bytes. A porcentagem entre parênteses do `Reclaimable` é descartada.
- O Docker relata tamanho lógico, que não é exatamente o espaço alocado. Por isso `apply_docker` limita o valor: `docker_bytes = min(Σ docker, restante)`, e depois `restante −= docker_bytes`. A barra continua somando 100%. Chamar de novo substitui o valor anterior (útil depois de um `docker_prune`).
- Na UI, o Docker é um subitem de Apps/Sistema ("Docker — 97 GB, 75 GB liberáveis"), separado de "Reservado / sem acesso".

---

## 7. Ponte Tauri (`src-tauri`)

| Command | Assinatura (Rust) | Observação |
|---|---|---|
| `disk_info` | `() -> DiskInfo` | Instantâneo: a UI desenha "Livre" antes mesmo do scan |
| `start_scan` | `(on_event: Channel<ScanEvent>) -> ()` | `std::thread::spawn`. Recusa se já houver um scan rodando. Depois do `Done`, consulta o Docker e emite `Docker` |
| `cancel_scan` | `() -> ()` | Seta o `AtomicBool` |
| `category_files` | `(cat: Category) -> Vec<FileEntry>` | Top-N do último resultado |
| `move_to_trash` | `(paths: Vec<PathBuf>) -> TrashReport` | Validação abaixo |
| `empty_trash` | `(confirm: bool) -> u64` | Exige `confirm == true` e devolve os bytes liberados |
| `reveal` | `(path: PathBuf) -> ()` | Só arquivos do último scan (`AppState::contains_file`), depois `tauri_plugin_opener::reveal_item_in_dir` (D-Bus `FileManager1.ShowItems` → Nautilus com o arquivo selecionado). `async`, porque a chamada D-Bus bloqueia |
| `docker_prune` | `(target: DockerTarget, confirm: bool) -> u64` | `DockerTarget` = `BuildCache` \| `UnusedImages`. Devolve os bytes liberados (releitura de `statvfs`) |

Status: `disk_info`, `start_scan`, `cancel_scan` e `category_files` estão ✅ implementados (Etapa 3), e `reveal` (Etapa 5). Os demais entram na Etapa 6.

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScanEvent {
    Progress(ScanProgress),                       // { kind: "progress", bytes, files }
    Done { result: ScanResult },
    Docker { usage: Option<DockerUsage>, docker_bytes: u64, unaccounted_bytes: u64 },
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
- `start_scan(opts, docker, send)` recebe a consulta ao Docker e o destino dos eventos como parâmetros. O command passa `docker_usage` e o `Channel`; os testes passam um Docker falso e um `mpsc`. Assim, o ciclo inteiro é testado com `cargo test`, sem janela (DECISIONS D24).
- `running` volta a `false` logo no `Done`. Um novo scan pode começar enquanto o Docker do anterior ainda responde. `generation` garante que esse resultado antigo seja descartado em vez de sobrescrever o novo.
- `cancel` é zerado **antes** do command retornar, então um `cancel_scan` imediatamente depois já vale.

Depois de `move_to_trash`, cada item sai do top-N e do total da sua categoria e seus bytes vão para `Other`/`trash_bytes`. **Mover para a Lixeira não libera espaço**; só `empty_trash` libera, e aí `livre` é recalculado pelo `statvfs`.

**Segurança (validação no Rust, não na UI):**
- `move_to_trash`: o caminho é canonicalizado (`fs::canonicalize`) e precisa estar sob `$HOME`, **não** casar a regra 1 (`is_app_path`: sistema e apps gerenciados; apps portáteis podem ir para a Lixeira) e constar no último `ScanResult`. Caminhos que falham são rejeitados e listados no `TrashReport`.
- Nunca há `remove_file` direto. O único apagamento permanente é `empty_trash`, que exige confirmação explícita.
- `docker_prune` só executa comandos fixos, sem argumento vindo da UI: `docker builder prune -f` e `docker image prune -a -f`. **Volumes nunca são apagados pelo app**, porque guardam dados (bancos, uploads); eles só aparecem com o tamanho e a dica `docker volume ls`.
- **Permissões:** o `build.rs` declara os commands no `AppManifest`. Com isso, o Tauri passa a exigir a permissão `allow-<command>` de cada um, em vez de liberar todos os commands registrados. `capabilities/default.json` lista só `core:default` e essas permissões. O `opener:default` do template foi removido e nenhuma permissão do opener é dada à janela: o `reveal` é um command próprio, que valida o caminho antes de chamar o plugin pelo Rust (DECISIONS D25, D31).
- **Caminhos não UTF-8** (válidos no Linux) são serializados com perdas (`�`) para não derrubar o resultado inteiro (DECISIONS D26). Na Etapa 6, as ações sobre esses arquivos precisam ser recusadas, já que o caminho recebido de volta não é o real.

---

## 8. Frontend (`src/`)

| Componente | Responsabilidade |
|---|---|
| `routes/+page.svelte` | Orquestra o estado (`idle` / `scanning` / `done` / `cancelled` / `error`), monta os segmentos e alterna entre a tela principal e o detalhe. A tela principal fica escondida, não desmontada, para a barra não refazer a animação na volta. Escaneia ao abrir |
| `StorageBar.svelte` | Barra única, em ordem fixa: as 6 categorias, a faixa "ainda não contado" e "Livre" na ponta direita. Cada largura é um `Tween` (600 ms, `cubicOut`) redirecionado a cada `progress`. Vãos de 2px com a cor do card separam os segmentos. Clicar num segmento abre o detalhe |
| `Legend.svelte` | Linhas com bolinha de cor, nome, tamanho e porcentagem do disco, ordenadas por bytes (`animate:flip`) e com "Livre" por último. Apps/Sistema traz a composição: Docker e reservado/sem acesso |
| `CategoryDetail.svelte` | Aberto ao clicar num segmento da barra ou numa linha da legenda (só com um resultado completo). Mostra o total, a porcentagem e a contagem, e lista o top-N de `category_files`, ordenado por tamanho ou por data. Em Outros aparece o card "Lixeira"; em Apps/Sistema, os cards "Docker" (imagens, containers, volumes e build cache, cada um com tamanho, quantidade e liberável; some sem Docker) e "Reservado / sem acesso". Voltar: botão ou `Esc`. Na Etapa 6 ganha checkbox, "Mover para Lixeira", o rodapé com o total selecionado e as ações dos cards |
| `FileRow.svelte` | Nome, pasta abreviada (`~/…`, via `homeDir()`), tamanho, data relativa e o botão "Abrir na pasta". Desabilitado para nomes não UTF-8, que chegam com `�` (D26) |
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
- Docker em modo rootless (`~/.local/share/docker`, já dentro do `$HOME`) e Podman (`podman system df`).
- Classificação por magic bytes para arquivos sem extensão.
