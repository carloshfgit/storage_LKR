//! Intel (`i915` e `xe`): só a frequência. Uso, memória e consumo vêm do PMU
//! pelo `perf` (o `intel_gpu_top` faz isso), que exige root: nem tentamos.

use std::path::{Path, PathBuf};

use super::{Reading, read_u64};

pub(super) struct Freq {
    /// Frequência real; sem ela, a pedida pelo driver.
    act: PathBuf,
    max_mhz: Option<u32>,
}

impl Freq {
    /// `i915`: `card*/gt_act_freq_mhz` e `gt_max_freq_mhz` (ou o `RP0`, o
    /// máximo do hardware). `xe`: `device/tile0/gt0/freq0/{act,max}_freq`.
    pub fn discover(card: &Path, dev: &Path, driver: &str) -> Option<Self> {
        let (dir, act, cur, max) = if driver == "xe" {
            (
                dev.join("tile0/gt0/freq0"),
                "act_freq",
                "cur_freq",
                ["max_freq", "rp0_freq"],
            )
        } else {
            (
                card.to_path_buf(),
                "gt_act_freq_mhz",
                "gt_cur_freq_mhz",
                ["gt_max_freq_mhz", "gt_RP0_freq_mhz"],
            )
        };
        let act = [act, cur]
            .iter()
            .map(|f| dir.join(f))
            .find(|p| p.exists())?;
        Some(Self {
            act,
            max_mhz: max
                .iter()
                .find_map(|f| read_u64(&dir.join(f)))
                .map(|v| v as u32),
        })
    }

    pub fn read(&self) -> Reading {
        Reading {
            freq_mhz: read_u64(&self.act).map(|v| v as u32),
            freq_max_mhz: self.max_mhz,
            ..Reading::default()
        }
    }
}
