//! Baterias: `/sys/class/power_supply/*` com `type = Battery`.
//!
//! O kernel informa em dois formatos, conforme o driver: energia (`energy_*`,
//! µWh, e `power_now`, µW) ou carga (`charge_*`, µAh, e `current_now`, µA).
//! Tudo é convertido para Wh e W.

use std::fs;
use std::path::Path;

use crate::model::{BatterySample, BatteryStatus, BatteryTotal};

/// Abaixo disso a potência é ruído (bateria cheia na tomada): sem tempo restante.
const MIN_POWER_W: f64 = 0.1;

fn status(s: &str) -> BatteryStatus {
    match s {
        "Charging" => BatteryStatus::Charging,
        "Discharging" => BatteryStatus::Discharging,
        "Full" => BatteryStatus::Full,
        "Not charging" => BatteryStatus::NotCharging,
        _ => BatteryStatus::Unknown,
    }
}

/// Uma bateria, ou `None` se `dir` não é bateria do sistema (carregador,
/// mouse ou fone Bluetooth, que têm `scope = Device`).
pub(crate) fn read_battery(dir: &Path) -> Option<BatterySample> {
    let text = |f: &str| {
        fs::read_to_string(dir.join(f))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let num = |f: &str| text(f)?.parse::<f64>().ok();
    if text("type")? != "Battery" || text("scope").as_deref() == Some("Device") {
        return None;
    }
    if text("present").as_deref() == Some("0") {
        // Slot de bateria removível vazio.
        return None;
    }

    // µAh → Wh pela tensão nominal (a do momento varia com a carga).
    let volts = num("voltage_min_design")
        .or_else(|| num("voltage_now"))
        .map(|uv| uv / 1e6);
    let wh = |energy: &str, charge: &str| {
        num(energy)
            .map(|uwh| uwh / 1e6)
            .or_else(|| Some(num(charge)? / 1e6 * volts?))
    };
    let now = wh("energy_now", "charge_now");
    let full = wh("energy_full", "charge_full");
    let design = wh("energy_full_design", "charge_full_design");
    // Alguns drivers mandam a corrente com sinal (negativa ao descarregar). A
    // tensão é a mesma da energia, como o UPower: tempo restante = Wh / W fecha.
    let power_w = num("power_now")
        .map(|uw| uw.abs() / 1e6)
        .or_else(|| Some(num("current_now")?.abs() / 1e6 * volts?));

    let pct = num("capacity")
        .map(|c| c as f32)
        .or_else(|| Some((now? / full.filter(|&f| f > 0.0)? * 100.0) as f32))
        .map(|p| p.clamp(0.0, 100.0));
    let cycles = num("cycle_count").map(|c| c as u32).filter(|&c| c > 0);

    Some(BatterySample {
        name: dir.file_name()?.to_string_lossy().into_owned(),
        model: text("model_name"),
        pct,
        status: text("status").map_or(BatteryStatus::Unknown, |s| status(&s)),
        energy_now_wh: now,
        energy_full_wh: full,
        energy_full_design_wh: design,
        power_w,
        health_pct: full
            .zip(design.filter(|&d| d > 0.0))
            .map(|(f, d)| (f / d * 100.0) as f32),
        cycles,
    })
}

/// Todas as baterias do sistema, em ordem de nome.
pub(crate) fn batteries(root: &Path) -> Vec<BatterySample> {
    let Ok(entries) = fs::read_dir(root.join("sys/class/power_supply")) else {
        return Vec::new();
    };
    let mut out: Vec<BatterySample> = entries
        .flatten()
        .filter_map(|e| read_battery(&e.path()))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Soma das baterias, como o UPower: nível ponderado pela energia de cada uma.
/// `with_time` falso esconde o tempo restante (primeiros segundos, ainda instável).
pub(crate) fn total(batteries: &[BatterySample], with_time: bool) -> Option<BatteryTotal> {
    if batteries.is_empty() {
        return None;
    }
    let sum = |f: fn(&BatterySample) -> Option<f64>| -> Option<f64> {
        batteries.iter().map(f).sum::<Option<f64>>()
    };
    let now = sum(|b| b.energy_now_wh);
    let full = sum(|b| b.energy_full_wh);
    let power = sum(|b| b.power_w);

    let pct = match (now, full) {
        (Some(n), Some(f)) if f > 0.0 => Some(((n / f) * 100.0).clamp(0.0, 100.0) as f32),
        _ => {
            let known: Vec<f32> = batteries.iter().filter_map(|b| b.pct).collect();
            (!known.is_empty()).then(|| known.iter().sum::<f32>() / known.len() as f32)
        }
    };
    let has = |s: BatteryStatus| batteries.iter().any(|b| b.status == s);
    let status = if has(BatteryStatus::Discharging) {
        BatteryStatus::Discharging
    } else if has(BatteryStatus::Charging) {
        BatteryStatus::Charging
    } else if batteries.iter().all(|b| b.status == BatteryStatus::Full) {
        BatteryStatus::Full
    } else {
        batteries[0].status
    };
    let time_left_s = with_time
        .then(|| {
            let p = power.filter(|&p| p >= MIN_POWER_W)?;
            let hours = match status {
                BatteryStatus::Discharging => now? / p,
                BatteryStatus::Charging => (full? - now?).max(0.0) / p,
                _ => return None,
            };
            Some((hours * 3600.0) as u64)
        })
        .flatten();

    Some(BatteryTotal {
        pct,
        status,
        power_w: power,
        time_left_s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    fn battery(fx: &Fixture, name: &str, files: &[(&str, &str)]) {
        fx.write(&format!("sys/class/power_supply/{name}/type"), "Battery\n");
        for (f, v) in files {
            fx.write(
                &format!("sys/class/power_supply/{name}/{f}"),
                &format!("{v}\n"),
            );
        }
    }

    #[test]
    fn charge_format_like_dell() {
        // Bateria desta máquina de desenvolvimento: cheia, na tomada.
        let fx = Fixture::new("bat-charge");
        battery(
            &fx,
            "BAT0",
            &[
                ("status", "Full"),
                ("capacity", "100"),
                ("charge_now", "2018000"),
                ("charge_full", "2018000"),
                ("charge_full_design", "3684000"),
                ("current_now", "1000"),
                ("voltage_now", "12909000"),
                ("voltage_min_design", "11400000"),
                ("cycle_count", "0"),
                ("model_name", "DELL FW8KR49"),
            ],
        );
        let b = &batteries(fx.path())[0];
        assert_eq!(b.status, BatteryStatus::Full);
        assert!((b.energy_full_wh.unwrap() - 2.018 * 11.4).abs() < 1e-9);
        assert!((b.health_pct.unwrap() - 54.78).abs() < 0.01);
        assert_eq!(b.cycles, None, "cycle_count = 0 é \"não informado\"");
        let t = total(std::slice::from_ref(b), true).unwrap();
        assert_eq!(t.pct, Some(100.0));
        assert_eq!(t.time_left_s, None, "na tomada e cheia");
    }

    #[test]
    fn energy_format_discharging() {
        let fx = Fixture::new("bat-energy");
        battery(
            &fx,
            "BAT0",
            &[
                ("status", "Discharging"),
                ("capacity", "50"),
                ("energy_now", "25000000"),
                ("energy_full", "50000000"),
                ("energy_full_design", "57000000"),
                ("power_now", "10000000"),
                ("cycle_count", "312"),
            ],
        );
        let bats = batteries(fx.path());
        assert_eq!(bats[0].power_w, Some(10.0));
        assert_eq!(bats[0].cycles, Some(312));
        let t = total(&bats, true).unwrap();
        assert_eq!(t.status, BatteryStatus::Discharging);
        assert_eq!(t.time_left_s, Some(9000), "25 Wh a 10 W = 2,5 h");
        assert_eq!(total(&bats, false).unwrap().time_left_s, None);
    }

    #[test]
    fn charging_time_to_full_and_signed_current() {
        let fx = Fixture::new("bat-charging");
        battery(
            &fx,
            "BAT1",
            &[
                ("status", "Charging"),
                ("charge_now", "1000000"),
                ("charge_full", "4000000"),
                ("current_now", "-2000000"),
                ("voltage_now", "12000000"),
            ],
        );
        let bats = batteries(fx.path());
        let b = &bats[0];
        // Sem voltage_min_design: a tensão do momento.
        assert_eq!(b.energy_now_wh, Some(12.0));
        assert_eq!(b.power_w, Some(24.0));
        assert_eq!(b.pct, Some(25.0), "sem `capacity`: energia / cheia");
        assert_eq!(b.health_pct, None, "sem full_design");
        assert_eq!(total(&bats, true).unwrap().time_left_s, Some(5400));
    }

    #[test]
    fn two_batteries_weighted_by_energy() {
        // ThinkPad: interna pequena vazia, externa grande cheia.
        let fx = Fixture::new("bat-two");
        battery(
            &fx,
            "BAT0",
            &[
                ("status", "Discharging"),
                ("capacity", "0"),
                ("energy_now", "0"),
                ("energy_full", "20000000"),
                ("power_now", "0"),
            ],
        );
        battery(
            &fx,
            "BAT1",
            &[
                ("status", "Discharging"),
                ("capacity", "100"),
                ("energy_now", "60000000"),
                ("energy_full", "60000000"),
                ("power_now", "15000000"),
            ],
        );
        let bats = batteries(fx.path());
        assert_eq!(bats.len(), 2);
        let t = total(&bats, true).unwrap();
        assert_eq!(t.pct, Some(75.0), "60 de 80 Wh, não a média de 0% e 100%");
        assert_eq!(t.power_w, Some(15.0));
        assert_eq!(t.time_left_s, Some(14400));
    }

    #[test]
    fn ignores_ac_bluetooth_mouse_and_empty_slot() {
        let fx = Fixture::new("bat-ignore");
        fx.write("sys/class/power_supply/AC/type", "Mains\n")
            .write("sys/class/power_supply/AC/online", "1\n");
        battery(
            &fx,
            "hidpp_battery_0",
            &[("scope", "Device"), ("capacity", "80")],
        );
        battery(&fx, "BAT1", &[("present", "0")]);
        battery(
            &fx,
            "CMB0",
            &[("status", "Not charging"), ("capacity", "80")],
        );
        let bats = batteries(fx.path());
        let names: Vec<&str> = bats.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["CMB0"]);
        assert_eq!(bats[0].status, BatteryStatus::NotCharging);
        let t = total(&bats, true).unwrap();
        assert_eq!(t.pct, Some(80.0));
        assert_eq!(t.time_left_s, None);
    }

    #[test]
    fn desktop_without_battery() {
        let fx = Fixture::new("bat-none");
        fx.write("sys/class/power_supply/AC/type", "Mains\n");
        assert!(batteries(fx.path()).is_empty());
        assert!(total(&[], true).is_none());
    }
}
