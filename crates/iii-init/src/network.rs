use std::net::Ipv4Addr;

use crate::error::InitError;

/// Write `/etc/resolv.conf` with the nameserver IP.
///
/// Priority: `III_INIT_DNS` env var > `/proc/net/route` gateway detection > `10.0.2.2` fallback.
pub fn write_resolv_conf() -> Result<(), InitError> {
    let nameserver = std::env::var("III_INIT_DNS")
        .unwrap_or_else(|_| detect_gateway().unwrap_or_else(|| "10.0.2.2".to_string()));

    std::fs::create_dir_all("/etc").map_err(|e| InitError::WriteFile {
        path: "/etc".into(),
        source: e,
    })?;

    std::fs::write("/etc/resolv.conf", format!("nameserver {nameserver}\n")).map_err(|e| {
        InitError::WriteFile {
            path: "/etc/resolv.conf".into(),
            source: e,
        }
    })?;

    Ok(())
}

/// Configure the guest network interface using ioctl syscalls.
///
/// Reads `III_INIT_IP`, `III_INIT_GW`, and `III_INIT_CIDR` from the environment.
/// If `III_INIT_IP` is not set, network configuration is skipped (backward compat).
///
/// Sequence:
/// 1. Assign IP and netmask to eth0
/// 2. Bring up eth0
/// 3. Add default route via gateway
/// 4. Write /etc/hosts mapping localhost to the gateway IP
///
/// The loopback interface is intentionally left down so that traffic to
/// `127.0.0.1` falls through to the default route, reaching the host via
/// the smoltcp TCP proxy.
pub fn configure_network() -> Result<(), InitError> {
    let ip_str = match std::env::var("III_INIT_IP") {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let gw_str = std::env::var("III_INIT_GW")
        .unwrap_or_else(|_| detect_gateway().unwrap_or_else(|| "10.0.2.2".to_string()));
    let cidr: u8 = std::env::var("III_INIT_CIDR")
        .unwrap_or_else(|_| "30".to_string())
        .parse()
        .map_err(|_| InitError::InvalidCidr(std::env::var("III_INIT_CIDR").unwrap_or_default()))?;

    let ip: Ipv4Addr = ip_str.parse().map_err(|_| InitError::InvalidAddr {
        var: "III_INIT_IP".into(),
        value: ip_str.clone(),
    })?;
    let gw: Ipv4Addr = gw_str.parse().map_err(|_| InitError::InvalidAddr {
        var: "III_INIT_GW".into(),
        value: gw_str.clone(),
    })?;
    let mask = cidr_to_mask(cidr);

    let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
    if sock < 0 {
        return Err(InitError::NetSocket(std::io::Error::last_os_error()));
    }

    let result = (|| {
        // Skip bringing up lo so that 127.0.0.1 traffic goes through eth0
        // via the default route, reaching the host through the smoltcp proxy.
        set_ip_address(sock, b"eth0\0", ip)?;
        set_netmask(sock, b"eth0\0", mask)?;
        set_interface_up(sock, b"eth0\0")?;
        add_default_route(sock, gw)?;
        Ok(())
    })();

    unsafe { libc::close(sock) };

    // Write /etc/hosts mapping localhost to the gateway IP so DNS resolution
    // of "localhost" returns the gateway (reachable via the virtual network)
    // instead of 127.0.0.1 (unreachable guest loopback).
    //
    // Defensive create_dir_all: most rootfs images ship /etc, but a
    // half-populated managed_dir (see local_worker.rs rootfs-clone path)
    // can have the VM booting against a tree where /etc hasn't been
    // created yet. Matches the pattern in `write_resolv_conf` above so
    // both network writes behave consistently. Swallow errors — we're
    // in best-effort territory and the subsequent write warning is the
    // real diagnostic.
    let _ = std::fs::create_dir_all("/etc");
    if let Err(e) = std::fs::write("/etc/hosts", format!("{gw}\tlocalhost\n")) {
        eprintln!("iii-init: warning: failed to write /etc/hosts: {e}");
    }

    result
}

/// Cast ioctl request constant to the platform-specific Ioctl type.
/// glibc uses `u64`, musl uses `i32`.
#[allow(clippy::unnecessary_cast)]
const fn ioctl_req(req: u64) -> libc::Ioctl {
    req as libc::Ioctl
}

fn set_ip_address(sock: libc::c_int, iface: &[u8], addr: Ipv4Addr) -> Result<(), InitError> {
    let mut ifr = new_ifreq(iface);
    let sa = make_sockaddr_in(addr);
    unsafe {
        std::ptr::copy_nonoverlapping(
            &sa as *const libc::sockaddr_in as *const u8,
            &mut ifr.ifr_ifru as *mut _ as *mut u8,
            std::mem::size_of::<libc::sockaddr_in>(),
        );
        if libc::ioctl(sock, ioctl_req(libc::SIOCSIFADDR as u64), &ifr) < 0 {
            return Err(InitError::NetIoctl {
                iface: iface_name(iface),
                op: "SIOCSIFADDR",
                source: std::io::Error::last_os_error(),
            });
        }
    }
    Ok(())
}

fn set_netmask(sock: libc::c_int, iface: &[u8], mask: Ipv4Addr) -> Result<(), InitError> {
    let mut ifr = new_ifreq(iface);
    let sa = make_sockaddr_in(mask);
    unsafe {
        std::ptr::copy_nonoverlapping(
            &sa as *const libc::sockaddr_in as *const u8,
            &mut ifr.ifr_ifru as *mut _ as *mut u8,
            std::mem::size_of::<libc::sockaddr_in>(),
        );
        if libc::ioctl(sock, ioctl_req(libc::SIOCSIFNETMASK as u64), &ifr) < 0 {
            return Err(InitError::NetIoctl {
                iface: iface_name(iface),
                op: "SIOCSIFNETMASK",
                source: std::io::Error::last_os_error(),
            });
        }
    }
    Ok(())
}

fn set_interface_up(sock: libc::c_int, iface: &[u8]) -> Result<(), InitError> {
    let mut ifr = new_ifreq(iface);

    unsafe {
        if libc::ioctl(sock, ioctl_req(libc::SIOCGIFFLAGS as u64), &mut ifr) < 0 {
            return Err(InitError::NetIoctl {
                iface: iface_name(iface),
                op: "SIOCGIFFLAGS",
                source: std::io::Error::last_os_error(),
            });
        }

        let flags = ifr.ifr_ifru.ifru_flags;
        ifr.ifr_ifru.ifru_flags = flags | libc::IFF_UP as i16 | libc::IFF_RUNNING as i16;

        if libc::ioctl(sock, ioctl_req(libc::SIOCSIFFLAGS as u64), &ifr) < 0 {
            return Err(InitError::NetIoctl {
                iface: iface_name(iface),
                op: "SIOCSIFFLAGS",
                source: std::io::Error::last_os_error(),
            });
        }
    }
    Ok(())
}

fn add_default_route(sock: libc::c_int, gateway: Ipv4Addr) -> Result<(), InitError> {
    unsafe {
        let mut rt: libc::rtentry = std::mem::zeroed();

        let dst = make_sockaddr_in(Ipv4Addr::UNSPECIFIED);
        std::ptr::copy_nonoverlapping(
            &dst as *const libc::sockaddr_in as *const u8,
            &mut rt.rt_dst as *mut libc::sockaddr as *mut u8,
            std::mem::size_of::<libc::sockaddr_in>(),
        );

        let gw = make_sockaddr_in(gateway);
        std::ptr::copy_nonoverlapping(
            &gw as *const libc::sockaddr_in as *const u8,
            &mut rt.rt_gateway as *mut libc::sockaddr as *mut u8,
            std::mem::size_of::<libc::sockaddr_in>(),
        );

        let mask = make_sockaddr_in(Ipv4Addr::UNSPECIFIED);
        std::ptr::copy_nonoverlapping(
            &mask as *const libc::sockaddr_in as *const u8,
            &mut rt.rt_genmask as *mut libc::sockaddr as *mut u8,
            std::mem::size_of::<libc::sockaddr_in>(),
        );

        rt.rt_flags = libc::RTF_UP | libc::RTF_GATEWAY;

        if libc::ioctl(sock, ioctl_req(libc::SIOCADDRT as u64), &rt) < 0 {
            return Err(InitError::NetRoute(std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

fn new_ifreq(iface: &[u8]) -> libc::ifreq {
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    let len = iface.len().min(libc::IFNAMSIZ);
    unsafe {
        std::ptr::copy_nonoverlapping(iface.as_ptr(), ifr.ifr_name.as_mut_ptr() as *mut u8, len);
    }
    ifr
}

fn make_sockaddr_in(addr: Ipv4Addr) -> libc::sockaddr_in {
    let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    sa.sin_family = libc::AF_INET as u16;
    sa.sin_addr.s_addr = u32::from(addr).to_be();
    sa
}

pub fn cidr_to_mask(prefix: u8) -> Ipv4Addr {
    if prefix == 0 {
        return Ipv4Addr::new(0, 0, 0, 0);
    }
    if prefix >= 32 {
        return Ipv4Addr::new(255, 255, 255, 255);
    }
    Ipv4Addr::from(!0u32 << (32 - prefix))
}

fn iface_name(iface: &[u8]) -> String {
    String::from_utf8_lossy(&iface[..iface.iter().position(|&b| b == 0).unwrap_or(iface.len())])
        .into_owned()
}

/// Parse `/proc/net/route` to find the default gateway IP.
///
/// The default route has destination `00000000`. The gateway field is
/// a hex-encoded little-endian IPv4 address.
fn detect_gateway() -> Option<String> {
    let contents = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in contents.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[1] == "00000000" {
            let hex = fields[2];
            let bytes = u32::from_str_radix(hex, 16).ok()?;
            let ip = std::net::Ipv4Addr::from(bytes.to_be());
            return Some(ip.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_gateway_does_not_panic() {
        let _ = detect_gateway();
    }

    #[test]
    fn test_gateway_hex_parsing() {
        let hex = "0100000A";
        let bytes = u32::from_str_radix(hex, 16).unwrap();
        let ip = std::net::Ipv4Addr::from(bytes.to_be());
        assert_eq!(ip.to_string(), "10.0.0.1");
    }

    #[test]
    fn test_gateway_fallback_ip() {
        let fallback = detect_gateway().unwrap_or_else(|| "10.0.2.2".to_string());
        if detect_gateway().is_none() {
            assert_eq!(fallback, "10.0.2.2");
        }
    }

    #[test]
    fn test_cidr_to_mask_30() {
        assert_eq!(cidr_to_mask(30), Ipv4Addr::new(255, 255, 255, 252));
    }

    #[test]
    fn test_cidr_to_mask_24() {
        assert_eq!(cidr_to_mask(24), Ipv4Addr::new(255, 255, 255, 0));
    }

    #[test]
    fn test_cidr_to_mask_0() {
        assert_eq!(cidr_to_mask(0), Ipv4Addr::new(0, 0, 0, 0));
    }

    #[test]
    fn test_cidr_to_mask_32() {
        assert_eq!(cidr_to_mask(32), Ipv4Addr::new(255, 255, 255, 255));
    }

    #[test]
    fn test_new_ifreq_name() {
        let ifr = new_ifreq(b"eth0\0");
        let name = unsafe {
            std::ffi::CStr::from_ptr(ifr.ifr_name.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(name, "eth0");
    }

    #[test]
    fn test_make_sockaddr_in() {
        let sa = make_sockaddr_in(Ipv4Addr::new(100, 96, 0, 2));
        assert_eq!(sa.sin_family, libc::AF_INET as u16);
        assert_eq!(
            sa.sin_addr.s_addr,
            u32::from(Ipv4Addr::new(100, 96, 0, 2)).to_be()
        );
    }

    #[test]
    fn test_iface_name() {
        assert_eq!(iface_name(b"eth0\0"), "eth0");
        assert_eq!(iface_name(b"lo\0"), "lo");
    }
}
