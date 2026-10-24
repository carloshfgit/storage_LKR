//! Tráfego de rede: `/proc/net/dev`, só nas interfaces físicas.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IfaceStat {
    pub name: String,
    pub rx: u64,
    pub tx: u64,
}

/// `  nome: rx_bytes rx_packets … (8 campos) tx_bytes …`. As duas primeiras
/// linhas são cabeçalho e não têm `:`.
pub(crate) fn parse_net_dev(text: &str) -> Vec<IfaceStat> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let mut f = rest.split_ascii_whitespace().map(|v| v.parse::<u64>().ok());
            let rx = f.next()??;
            let tx = f.nth(7)??;
            Some(IfaceStat {
                name: name.trim().to_string(),
                rx,
                tx,
            })
        })
        .collect()
}

/// Interface física: tem `device/` no sysfs. `lo`, `docker0`, `br-*`,
/// `veth*`, `virbr*` e as de VPN (`tun*`, `wg*`) não têm; a VPN contaria o
/// mesmo tráfego duas vezes.
pub(crate) fn is_physical(root: &Path, name: &str) -> bool {
    root.join("sys/class/net")
        .join(name)
        .join("device")
        .exists()
}

pub(crate) fn is_wireless(root: &Path, name: &str) -> bool {
    let dir = root.join("sys/class/net").join(name);
    dir.join("wireless").exists() || dir.join("phy80211").exists()
}

/// Diferença de um contador. Um contador que voltou (interface que caiu e
/// voltou, ou contador de 32 bits que deu a volta) conta como zero: melhor
/// perder um segundo do que desenhar um pico falso.
pub(crate) fn counter_delta(prev: u64, cur: u64) -> u64 {
    cur.saturating_sub(prev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    const NET_DEV: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 192223102  455996    0    0    0     0          0         0 192223102  455996    0    0    0     0       0          0
enp3s0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0
wlp2s0:5000 50 0 0 0 0 0 0 6000 60 0 0 0 0 0 0
docker0: 7 1 0 0 0 0 0 0 8 1 0 0 0 0 0 0
  wg0: 900 9 0 0 0 0 0 0 900 9 0 0 0 0 0 0
";

    #[test]
    fn parses_rx_and_tx() {
        let s = parse_net_dev(NET_DEV);
        assert_eq!(s.len(), 5);
        assert_eq!(
            s[1],
            IfaceStat {
                name: "enp3s0".into(),
                rx: 1000,
                tx: 2000
            }
        );
        // Nome colado no `:` (interfaces de nome longo, kernel antigo).
        assert_eq!(
            s[2],
            IfaceStat {
                name: "wlp2s0".into(),
                rx: 5000,
                tx: 6000
            }
        );
    }

    #[test]
    fn cable_and_wifi_are_physical_virtual_are_not() {
        let fx = Fixture::new("net");
        fx.dir("sys/class/net/enp3s0/device")
            .dir("sys/class/net/wlp2s0/device")
            .dir("sys/class/net/wlp2s0/wireless")
            .dir("sys/class/net/lo")
            .dir("sys/class/net/docker0/bridge")
            .dir("sys/class/net/wg0");
        let physical: Vec<String> = parse_net_dev(NET_DEV)
            .into_iter()
            .filter(|i| is_physical(fx.path(), &i.name))
            .map(|i| i.name)
            .collect();
        assert_eq!(physical, ["enp3s0", "wlp2s0"]);
        assert!(is_wireless(fx.path(), "wlp2s0"));
        assert!(!is_wireless(fx.path(), "enp3s0"));
    }

    #[test]
    fn reset_counter_is_zero() {
        assert_eq!(counter_delta(100, 150), 50);
        assert_eq!(counter_delta(4_294_967_000, 200), 0);
    }
}
