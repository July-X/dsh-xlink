//! Explicit trusted-LAN selection. Private addressing is not a trust assertion:
//! the owner confirms an interface, address and mask; changes require reconfirmation.
use super::config::HooksConfig;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LanInterface {
    pub name: String,
    pub address: String,
    pub netmask: String,
    pub cidr: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Subnet {
    network: u32,
    mask: u32,
}
impl Subnet {
    fn from_mask(ip: Ipv4Addr, mask: Ipv4Addr) -> Result<Self, String> {
        let mask = u32::from(mask);
        let bits = mask.leading_ones();
        let expected = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits)
        };
        if mask != expected {
            return Err("子网掩码不连续，请重新选择网卡".into());
        }
        Ok(Self {
            network: u32::from(ip) & mask,
            mask,
        })
    }
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        let (ip, bits) = value
            .trim()
            .split_once('/')
            .ok_or("可信网段须使用 CIDR，例如 192.168.11.0/24")?;
        let ip: Ipv4Addr = ip.parse().map_err(|_| "可信网段必须是 IPv4 CIDR")?;
        let bits: u32 = bits.parse().map_err(|_| "可信网段前缀长度无效")?;
        if bits > 32 {
            return Err("可信网段前缀长度必须为 0–32".into());
        }
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits)
        };
        let subnet = Self::from_mask(ip, Ipv4Addr::from(mask))?;
        if !Ipv4Addr::from(subnet.network).is_private()
            || !Ipv4Addr::from(subnet.network | !mask).is_private()
        {
            return Err("只允许完全位于私网范围的可信网段".into());
        }
        Ok(subnet)
    }
    fn contains(&self, ip: Ipv4Addr) -> bool {
        u32::from(ip) & self.mask == self.network
    }
}

#[cfg(test)]
static TEST_INTERFACES: std::sync::Mutex<Option<Vec<LanInterface>>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(crate) struct InterfaceGuard;
#[cfg(test)]
impl InterfaceGuard {
    pub(crate) fn replace(&self, items: Vec<LanInterface>) {
        *TEST_INTERFACES.lock().unwrap() = Some(items);
    }
}
#[cfg(test)]
impl Drop for InterfaceGuard {
    fn drop(&mut self) {
        *TEST_INTERFACES.lock().unwrap() = None;
    }
}
#[cfg(test)]
pub(crate) fn test_interfaces(items: Vec<LanInterface>) -> InterfaceGuard {
    *TEST_INTERFACES.lock().unwrap() = Some(items);
    InterfaceGuard
}

#[cfg(test)]
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// service/server 测试都会替换全局网卡枚举，必须共用同一把锁。
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn usable_interface(interface: &if_addrs::Interface) -> bool {
    interface.is_oper_up() && matches!(interface.ip(), IpAddr::V4(ip) if ip.is_private())
}

pub(crate) fn interfaces() -> Result<Vec<LanInterface>, String> {
    #[cfg(test)]
    if let Some(items) = TEST_INTERFACES.lock().unwrap().clone() {
        return Ok(items);
    }

    let mut result = Vec::new();
    for interface in
        if_addrs::get_if_addrs().map_err(|e| format!("无法读取网络接口：{e}；请检查网络后重试"))?
    {
        if !usable_interface(&interface) {
            continue;
        }
        let if_addrs::IfAddr::V4(address) = interface.addr else {
            continue;
        };
        if !address.ip.is_private() {
            continue;
        }
        let subnet = Subnet::from_mask(address.ip, address.netmask)?;
        let cidr = format!(
            "{}/{}",
            Ipv4Addr::from(subnet.network),
            subnet.mask.count_ones()
        );
        // A malformed/broad interface mask must not implicitly authorize public sources.
        if Subnet::parse(&cidr).is_err() {
            continue;
        }
        result.push(LanInterface {
            name: interface.name,
            address: address.ip.to_string(),
            netmask: address.netmask.to_string(),
            cidr,
        });
    }
    result.sort_by(|a, b| (&a.name, &a.address).cmp(&(&b.name, &b.address)));
    result.dedup();
    Ok(result)
}

#[derive(Debug, Clone)]
pub(crate) struct LanPolicy {
    pub interface: LanInterface,
    subnets: Vec<Subnet>,
    revoked: Arc<AtomicBool>,
}
impl LanPolicy {
    pub(crate) fn resolve(
        config: &HooksConfig,
        interfaces: &[LanInterface],
    ) -> Result<Self, String> {
        let interface = interfaces
            .iter()
            .find(|i| {
                i.name == config.interface_name
                    && i.address == config.bind_ip
                    && i.netmask == config.netmask
            })
            .ok_or("可信局域网尚未选择或地址已变化；请在 Hook 设置重新选择网卡并启用接收")?
            .clone();
        let mut subnets = vec![Subnet::parse(&interface.cidr)?];
        if config.allowed_subnets.len() > 32 {
            return Err("额外可信网段最多 32 项".into());
        }
        for subnet in &config.allowed_subnets {
            subnets.push(Subnet::parse(subnet)?);
        }
        Ok(Self {
            interface,
            subnets,
            revoked: Arc::new(AtomicBool::new(false)),
        })
    }
    pub(crate) fn allows(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ip) => ip.is_loopback() || self.subnets.iter().any(|s| s.contains(ip)),
            _ => false,
        }
    }
    pub(crate) fn available(&self) -> bool {
        if !interfaces().is_ok_and(|items| items.contains(&self.interface)) {
            self.revoked.store(true, Ordering::SeqCst);
        }
        !self.revoked()
    }
    pub(crate) fn revoked(&self) -> bool {
        self.revoked.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn interface() -> LanInterface {
        LanInterface {
            name: "ethernet".into(),
            address: "10.20.1.2".into(),
            netmask: "255.255.254.0".into(),
            cidr: "10.20.0.0/23".into(),
        }
    }
    #[test]
    fn disconnected_interface_with_static_ip_is_not_available() {
        let mut interface = if_addrs::Interface {
            name: "ethernet".into(),
            addr: if_addrs::IfAddr::V4(if_addrs::Ifv4Addr {
                ip: "10.1.1.2".parse().unwrap(),
                netmask: "255.255.255.0".parse().unwrap(),
                prefixlen: 24,
                broadcast: None,
            }),
            index: Some(1),
            oper_status: if_addrs::IfOperStatus::Up,
            #[cfg(windows)]
            adapter_name: "test-adapter".into(),
        };
        assert!(usable_interface(&interface));
        interface.oper_status = if_addrs::IfOperStatus::Down;
        assert!(!usable_interface(&interface));
        interface.oper_status = if_addrs::IfOperStatus::Unknown;
        assert!(!usable_interface(&interface));
    }

    #[test]
    fn private_cidrs_only() {
        for cidr in [
            "0.0.0.0/0",
            "10.0.0.0/7",
            "172.0.0.0/8",
            "192.168.0.0/15",
            "8.8.8.8/32",
            "::1/128",
            "10.1.1.1/33",
        ] {
            assert!(Subnet::parse(cidr).is_err(), "{cidr}");
        }
        for cidr in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "10.2.3.4/32",
        ] {
            assert!(Subnet::parse(cidr).is_ok(), "{cidr}");
        }
    }
    #[test]
    fn explicit_selection_and_sources() {
        let i = interface();
        let mut config = HooksConfig::default();
        assert!(LanPolicy::resolve(&config, std::slice::from_ref(&i)).is_err());
        config.interface_name = i.name.clone();
        config.bind_ip = i.address.clone();
        config.netmask = i.netmask.clone();
        config.allowed_subnets = vec!["192.168.11.0/24".into()];
        let policy = LanPolicy::resolve(&config, std::slice::from_ref(&i)).unwrap();
        for ip in ["10.20.0.9", "10.20.1.9", "192.168.11.4", "127.0.0.1"] {
            assert!(policy.allows(ip.parse().unwrap()));
        }
        for ip in ["10.20.2.9", "192.168.12.4", "8.8.8.8", "::1"] {
            assert!(!policy.allows(ip.parse().unwrap()));
        }
        config.netmask = "255.255.0.0".into();
        assert!(LanPolicy::resolve(&config, &[i]).is_err());
    }
    #[test]
    fn refuses_noncontiguous_mask() {
        assert!(
            Subnet::from_mask("10.1.1.1".parse().unwrap(), "255.0.255.0".parse().unwrap()).is_err()
        );
    }
}
