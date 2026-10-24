//! AMD (`amdgpu`): uso, memória de vídeo e, nas integradas, a memória do
//! sistema mapeada para a GPU (GTT). Temperatura, consumo e frequência vêm do
//! hwmon da placa (`Hwmon`, em `mod.rs`).

use std::path::Path;

use super::{Reading, read_u64};

/// Só chamar com a placa ativa: com ela suspensa, cada arquivo destes a
/// acordaria (kernel 5.15) ou falharia com EBUSY (kernels novos).
pub(super) fn read(dev: &Path, integrated: bool) -> Reading {
    let get = |f: &str| read_u64(&dev.join(f));
    Reading {
        busy_pct: get("gpu_busy_percent").map(|p| p.min(100) as f32),
        vram_used: get("mem_info_vram_used"),
        gtt_used: integrated.then(|| get("mem_info_gtt_used")).flatten(),
        gtt_total: integrated.then(|| get("mem_info_gtt_total")).flatten(),
        ..Reading::default()
    }
}

/// O `amdgpu` só cria a tensão `vddnb` (`in1_*`) do hwmon nas APUs.
pub(super) fn is_integrated(hwmon: &Path) -> bool {
    hwmon.join("in1_input").exists()
}
