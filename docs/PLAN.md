# PLAN — Etapas de desenvolvimento

Cada etapa termina com algo **executável e verificável**. Detalhes técnicos estão em [ARCHITECTURE.md](ARCHITECTURE.md), e a justificativa da stack em [STACK_CHOICE.md](STACK_CHOICE.md). O porquê de cada decisão está em [DECISIONS.md](DECISIONS.md).

**Escopo do MVP:**
- Barra com 7 segmentos: Fotos, Vídeos, Áudio, Documentos, Apps/Sistema, Outros e Livre.
- Detalhe por categoria.
- Abrir na pasta, mover para a Lixeira e esvaziar a Lixeira com confirmação.

---

## Etapa 0 — Ambiente
- [x] Instalar Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- [x] Instalar as dependências do sistema:
  ```bash
  sudo apt install build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev \
    libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```
- [x] Gerar o projeto: `npm create tauri-app@latest` (Svelte + TypeScript), com saída na raiz do repositório.
- [x] Converter para workspace Cargo (`[workspace] members = ["crates/scanner", "src-tauri"]`) e criar o `crates/scanner` vazio.
- [x] `git init` + `.gitignore` (`target/`, `node_modules/`, `dist/`).

**Pronto quando:** `npm run tauri dev` abre a janela padrão.

---

## Etapa 1 — Motor de scan (CLI)
- [x] `model.rs`: `Category`, `FileEntry`, `CategoryStats`, `ScanResult`.
- [x] `disk.rs`: `disk_info("/")` via `rustix::fs::statvfs`.
- [x] `walk.rs`:
  - jwalk paralelo, sem cruzar filesystem (`st_dev`), sem seguir symlinks
  - `st_blocks * 512` e dedupe de hardlinks
  - top-N com heap
  - progresso com rate-limit e cancelamento
- [x] Na Etapa 1, `classify` é provisório e só olha a extensão.
- [x] `bin/scan.rs`: imprime a tabela por categoria, o restante, os diretórios negados e o tempo.

**Pronto quando:**
- `cargo run --release -p scanner --bin scan` termina e total + restante bate com o `df /` (±1%).
- Scan apontado para `$HOME` ≈ `du -sx ~` (±1%).
- `/` com cache quente leva **menos de 5 s**.

**Resultado (17/10/2026, SSD, 4 núcleos):**

| Medida | Scanner | Referência |
|---|---|---|
| `$HOME` | 56 397 565 952 B, 742 mil arquivos, 1,45 s | `du -sxB1 ~` = 56 397 565 952 B (idêntico) |
| `/` (sem root) | 79,66 GB, 1,07 milhão de arquivos | `du -sxB1 /` = 79,66 GB (diferença de 86 KB) |
| `/`, cache quente | 2,1 a 2,6 s | meta < 5 s |
| `/`, cache frio | 11,9 s | — |

Total + restante = `df` por construção. O restante (106 GB) é real: são diretórios só de root, principalmente o Docker (`docker system df`: cerca de 97 GB entre imagens, volumes e build cache). 8 threads (2× núcleos) foi o melhor valor; 4 e 16 ficaram iguais ou piores.

---

## Etapa 2 — Classificação definitiva
- [x] Implementar as 6 regras ordenadas e a tabela de extensões de ARCHITECTURE §5.
- [x] Testes unitários (pelo menos estes casos):

| Caminho | Esperado |
|---|---|
| `/usr/share/icons/x.png` | Apps/Sistema |
| `~/.cache/mozilla/a.jpg` | Outros |
| `~/.local/share/Trash/files/v.mp4` | Outros (conta em `trash_bytes`) |
| `~/Vídeos/Férias.MKV` | Vídeos |
| `~/snap/spotify/foo.mp3` | Apps/Sistema |
| `~/Downloads/ubuntu.iso` | Outros |
| `~/Documentos/cv.pdf` | Documentos |

- [x] `docker.rs` (ARCHITECTURE §6.1): `docker_usage(timeout)` roda `docker system df --format json` com timeout de 20 s e devolve `Option<DockerUsage>`. O `scan()` não espera por ele.
- [x] Descontar o Docker do restante com `ScanResult::apply_docker`: `docker_bytes = min(Σ docker, restante)`.
- [x] Testes do parser de tamanhos (`"58.68GB"`, `"113.1MB"`, `"0B"`, `"39.88GB (68%)"`) e do caso sem Docker (binário ausente ou daemon inacessível vira `None`, sem erro).
- [x] O CLI mostra a linha "Docker" separada de "Restante (sem acesso)".

**Pronto quando:**
- `cargo test -p scanner` passa, e o CLI mostra uma distribuição plausível na máquina real.
- Nesta máquina, o CLI mostra cerca de 97 GB de Docker, e o "sem acesso" cai de 106 GB para menos de 15 GB.

**Resultado (17/10/2026):**

| Linha | Antes (Etapa 1) | Agora |
|---|---|---|
| Fotos | 1,35 GB | 0,31 GB (os JPG/PNG de caches e de `/usr` saíram) |
| Apps/Sistema | 0 GB | 130,2 GB = 23,9 escaneado + **97,15 Docker** + 9,15 reservado/sem acesso |
| Outros | 75,6 GB | 54,0 GB, dos quais 5,77 GB na Lixeira |
| "Sem acesso" | 106,3 GB | **9,15 GB** |

- 21 testes passam (`cargo test -p scanner`).
- Walk de `/` sem Docker: 2,0 a 2,5 s, sem perda com as novas regras.
- `docker system df` sozinho: cerca de 5,2 s. Junto com o walk, os dois disputam o disco (walk de 4 a 5 s, total de cerca de 6,5 s). Na Etapa 3, a consulta ao Docker vai rodar depois do `Done`.
- **Refinamento (DECISIONS D21–D23):**
  - Regras de app portátil (AppImage, `~/Applications`, `resources/app` do Electron) e de build/dev (`node_modules`, `target/debug|release`, `__pycache__`, `site-packages`).
  - `.ts` saiu de Vídeos.
  - Resultado: Vídeos caiu de 37.285 para 18 arquivos, e o tempo do walk não mudou.
  - 28 testes passam.
  - Resta como limitação (D23): mídia de projetos próprios conta como mídia.

---

## Etapa 3 — Ponte Tauri
- [ ] `state.rs` (`AppState`) e `commands.rs`: `disk_info`, `start_scan` (Channel), `cancel_scan`, `category_files`.
- [ ] `capabilities/default.json` expondo só esses commands.
- [ ] `api.ts` com os tipos espelhando `ScanEvent`/`ScanResult`.

**Pronto quando:** o DevTools da janela mostra eventos `progress` chegando e um `done` final, e `cancel_scan` interrompe o scan.

---

## Etapa 4 — Tela principal
- [ ] `theme.css`: tokens de cor, claro/escuro e tipografia.
- [ ] `format.ts`: bytes decimais em pt-BR.
- [ ] Header com nome do disco e "X GB de Y GB usados".
- [ ] `StorageBar`: 7 segmentos com `tweened`, que crescem ao vivo durante o scan, com "Livre" desenhado logo após `disk_info`.
- [ ] `Legend` ordenada por tamanho e botão "Reescanear".
- [ ] Estados: escaneando (com contador de arquivos), pronto e erro.

**Pronto quando:** a barra anima durante o scan, soma exatamente 100% no fim e alterna corretamente entre tema claro e escuro.

---

## Etapa 5 — Detalhe da categoria
- [ ] `CategoryDetail` + `FileRow`: top-N ordenado por tamanho, com alternância para ordenar por data.
- [ ] "Abrir na pasta" via `reveal` (`tauri-plugin-opener`).
- [ ] Cards especiais: "Docker" e "Reservado / sem acesso" (Apps/Sistema) e "Lixeira" (Outros).
- [ ] O card Docker mostra imagens, containers, volumes e build cache, cada um com tamanho e quanto é liberável. Quando não há Docker, o card não aparece.

**Pronto quando:** clicar no segmento "Vídeos" lista os maiores vídeos, e "Abrir na pasta" abre o Nautilus com o arquivo selecionado.

---

## Etapa 6 — Ações de limpeza
- [ ] `move_to_trash` com validação no Rust (canônico, sob `$HOME`, fora de apps, presente no último scan), usando o crate `trash`.
- [ ] Seleção múltipla, rodapé com o total e atualização da barra: bytes saem da categoria e entram em "Lixeira".
- [ ] `empty_trash` com diálogo de confirmação ("Apagar permanentemente X GB?"), seguido de nova leitura de `disk_info`.
- [ ] Teste manual: tentar mandar para a lixeira um caminho fora de `$HOME` via DevTools precisa ser rejeitado.
- [ ] `docker_prune` no card Docker, com diálogo de confirmação mostrando o tamanho:
  - "Limpar build cache" → `docker builder prune -f`
  - "Remover imagens não usadas" → `docker image prune -a -f` (avisar que vão precisar ser baixadas de novo)
  - Volumes: só exibidos, **sem botão**, porque guardam dados.

**Pronto quando:**
- O arquivo aparece na Lixeira do Ubuntu e pode ser restaurado pelo Nautilus.
- Esvaziar a Lixeira aumenta o "Livre" na mesma quantidade vista no `df`.
- Limpar o build cache do Docker aumenta o "Livre" no valor que o `df` confirma, e o card Docker é atualizado.

---

## Etapa 7 — Polimento e pacote
- [ ] Cache do último scan em `~/.cache/storage-lkr/last.json`: abre instantâneo e reescaneia em background.
- [ ] Ícone do app, título da janela e tamanho mínimo.
- [ ] Animações finas: entrada do detalhe e hover nos segmentos com tooltip.
- [ ] `npm run tauri build` → `.deb` → `sudo apt install ./src-tauri/target/release/bundle/deb/*.deb`.

**Pronto quando:** o app aparece no menu do GNOME, abre em menos de 1 s mostrando o último scan e atualiza sozinho.

---

## Backlog (pós-MVP)
1. Detalhar Apps por Snap, Flatpak e APT, com tamanho por app.
2. Cards de sugestão: "Downloads antigos", "Caches", "10 maiores arquivos".
3. Atualização incremental (inotify ou fanotify).
4. Modo privilegiado via `pkexec` para escanear o que hoje é "sem acesso".
5. Detecção por magic bytes para arquivos sem extensão.
6. Suporte a múltiplos discos e partições (seletor no header).
7. Docker rootless e Podman (`podman system df`).
