# Ideia — Aba de Recursos do Sistema

Documento conceitual sobre a proposta de adicionar uma aba **"Recursos"** ao Storage Looker, fornecendo uma visão em tempo real da saúde e do consumo de hardware do computador. Essa aba ficara acessivel por um botao centralizado no topo da pagina inicial armazenamento/recursos.

---

## 1. Proposta Geral

Expandir o valor do Storage Looker com um painel de telemetria moderno, leve e direto ao ponto. Além de analisar o espaço ocupado em disco, o usuário poderá acompanhar como o computador está trabalhando em tempo real (especialmente durante tarefas pesadas, scans ou no dia a dia).

A proposta visual segue a mesma filosofia do app: **limpo, elegante e sem sobrecarga visual**.

---

## 2. Métricas Principais (Gráficos em Tempo Real)

Gráficos com histórico contínuo (ex: últimos 60 segundos), com cards destacados:

*   **Processador (CPU):**
    *   Carga total de uso (%) e frequência/clock.
    *   Visão geral por núcleos/threads.
    *   Temperatura da CPU.
*   **Memória (RAM & Swap):**
    *   Uso em tempo real vs. total instalado (GB e %).
    *   Uso de Swap / zRAM (indicativo importante se a máquina estiver sob pressão de memória).
*   **Leitura e escrita (SSD / HD):**
    *   **Velocidade de I/O em tempo real:** taxas de leitura e gravação (MB/s).
    *   **Temperatura do disco (especialmente importante para NVMe).**
*   **Placa de Vídeo (GPU):**
    *   Uso do chip gráfico (%) e consumo de memória de vídeo (VRAM).
    *   Temperatura e consumo energético (quando suportado pelo driver).
    *   *Nota:* Implementação com fallback gracioso (NVIDIA / AMD / Intel / Integrada), ocultando ou informando caso a GPU não exponha métricas.

---

## 3. Ideias de Recursos Complementares

*   **Tráfego de Rede:**
    *   Gráficos simples de velocidade de Download e Upload (KB/s, MB/s) e total transferido na sessão.
*   **"Top 5 Vilões" (Processos):**
    *   Pequena listagem dinâmica mostrando apenas os 5 processos que mais estão consumindo CPU, RAM ou gravando no disco naquele instante (para diagnóstico rápido sem precisar abrir o terminal ou `htop`).
*   **Bateria e Energia (Notebooks):**
    *   Nível de carga, tempo estimado restante, taxa de consumo (Watts) e saúde da bateria (% da capacidade original restante vs desgaste).
*   **Ficha Técnica Resumida:**
    *   Modelo do processador e da GPU.
    *   Tempo de atividade da máquina (uptime).
    *   Versão do Kernel Linux e distribuição.

---

## 4. Diretrizes Técnicas e de Experiência

*   **Impacto Próximo de Zero na CPU:**
    *   A coleta de dados via Rust (`sysinfo` / leitura direta de `/proc` e `/sys`) só deve rodar enquanto a aba "Recursos" estiver em foco no frontend.
    *   Ao mudar de aba ou minimizar a janela, o loop de monitoramento deve pausar para economizar bateria e ciclos de processamento.
*   **Renderização Leve:**
    *   Gráficos renderizados diretamente em SVG nativo reativo com Svelte ou biblioteca ultraleve em Canvas (como `uPlot`), sem introduzir lentidão na interface.
