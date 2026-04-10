use std::{cmp::min, net::Ipv4Addr, ops::Range};

use anyhow::bail;
use ipnet::Ipv4Net;

/// Params for creating a network manager. The manager created will use the smaller of the provided network ranges
#[derive(Debug, Clone)]
pub struct NetworkRanges {
    pub vm_subnet: Ipv4Net,
    pub ssh_port_range: Range<u16>,
}

/// Iterator that yields IP pairs and SSH ports from a NetworkRanges
pub struct NetworkRangesIter {
    current: u32,
    end: u32,
    current_port: u16,
    end_port: u16,
}

impl NetworkRanges {
    /// Create a new NetworkRanges struct. This struct will automatically be minimized; see NetworkRanges::minimize
    pub fn new(vm_subnet: Ipv4Net, ssh_port_range: Range<u16>) -> anyhow::Result<Self> {
        let value = Self {
            vm_subnet,
            ssh_port_range,
        };

        value.minimize()
    }

    /// Compute a new NetworkRanges with the smaller of the provided networks determining its actual size. For instance,
    /// if the IP range supports 128 VMs and the SSH range supports 256, a NetworkRanges supporting 128 VMs will be returned.
    /// Returns an error if a network of size 0 would be returned.
    fn minimize(self) -> anyhow::Result<Self> {
        let subnet_size: usize =
            (self.vm_subnet.network().to_bits()..=self.vm_subnet.broadcast().to_bits()).count();
        let ssh_range_size: usize = self.ssh_port_range.len();

        // Use the number of IP pairs rather than the number of hosts.
        let minimum_size = min(subnet_size / 2, ssh_range_size);
        if minimum_size == 0 {
            bail!("NetworkRanges with size 0 defined");
        }

        // floor(log2(minimum_size))
        let floor_log2: u32 = usize::BITS - 1 - minimum_size.leading_zeros();

        let prefix_len = 31 - floor_log2;
        let num_ports = (2 as u32).pow(floor_log2);

        let vm_subnet = Ipv4Net::new(self.vm_subnet.network(), prefix_len as u8)?;
        let ssh_port_range =
            self.ssh_port_range.start..(self.ssh_port_range.start + num_ports as u16);

        Ok(Self {
            vm_subnet,
            ssh_port_range,
        })
    }

    /// Create an iterator that yields IP pairs and SSH ports
    pub fn iter(&self) -> NetworkRangesIter {
        let start = self.vm_subnet.network().to_bits();
        let end = self.vm_subnet.broadcast().to_bits();

        NetworkRangesIter {
            current: start,
            end,
            current_port: self.ssh_port_range.start,
            end_port: self.ssh_port_range.end,
        }
    }

    /// Returns the number of IP pairs represented by the NetworkRanges. Assumes that self is minimized.
    pub fn get_ip_pair_count(&self) -> usize {
        self.ssh_port_range.len()
    }
}

impl Iterator for NetworkRangesIter {
    type Item = ((Ipv4Net, Ipv4Net), u16);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end || self.current_port >= self.end_port {
            return None;
        }

        // Create IP pair
        let pos = self.current & (0xFFFFFFFE);
        let host_ip =
            Ipv4Net::new(Ipv4Addr::from_bits(pos), 31).expect("prefix len 31 is hardcoded");
        let vm_ip =
            Ipv4Net::new(Ipv4Addr::from_bits(pos + 1), 31).expect("prefix len 31 is hardcoded");

        // Get current port
        let port = self.current_port;

        // Increment for next iteration
        self.current = self.current.saturating_add(2);
        self.current_port += 1;

        Some(((host_ip, vm_ip), port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_basic_network_ranges_creation() {
        let vm_subnet = "192.168.1.0/28".parse::<Ipv4Net>().unwrap(); // 16 IPs, 8 pairs
        let ssh_range = 2222..2230; // 8 ports

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        // Should keep original sizes since they match
        assert_eq!(ranges.vm_subnet.prefix_len(), 28);
        assert_eq!(ranges.ssh_port_range.len(), 8);
    }

    #[test]
    fn test_minimize_by_ip_range() {
        let vm_subnet = "192.168.1.0/30".parse::<Ipv4Net>().unwrap(); // 4 IPs, 2 pairs
        let ssh_range = 2222..2232; // 10 ports

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        // Should be limited by IP range (2 pairs)
        assert_eq!(ranges.vm_subnet.prefix_len(), 30); // Still /30 for 2 pairs
        assert_eq!(ranges.ssh_port_range.len(), 2); // Limited to 2 ports
    }

    #[test]
    fn test_minimize_by_port_range() {
        let vm_subnet = "192.168.1.0/26".parse::<Ipv4Net>().unwrap(); // 64 IPs, 32 pairs
        let ssh_range = 2222..2226; // 4 ports

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        // Should be limited by port range (4 pairs)
        assert_eq!(ranges.vm_subnet.prefix_len(), 29); // Adjusted to /29 for 4 pairs
        assert_eq!(ranges.ssh_port_range.len(), 4); // Keeps all 4 ports
    }

    #[test]
    fn test_single_pair() {
        let vm_subnet = "192.168.1.0/31".parse::<Ipv4Net>().unwrap(); // 2 IPs, 1 pair
        let ssh_range = 2222..2223; // 1 port

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        assert_eq!(ranges.vm_subnet.prefix_len(), 31);
        assert_eq!(ranges.ssh_port_range.len(), 1);
    }

    #[test]
    fn test_empty_port_range_fails() {
        let vm_subnet = "192.168.1.0/28".parse::<Ipv4Net>().unwrap();
        let ssh_range = 2222..2222; // Empty range

        let result = NetworkRanges::new(vm_subnet, ssh_range);
        assert!(result.unwrap_err().to_string().contains("size 0"));
    }

    #[test]
    fn test_single_ip_subnet_fails() {
        let vm_subnet = "192.168.1.1/32".parse::<Ipv4Net>().unwrap(); // 1 IP, 0 pairs
        let ssh_range = 2222..2230;

        let result = NetworkRanges::new(vm_subnet, ssh_range);
        assert!(result.unwrap_err().to_string().contains("size 0"));
    }

    #[test]
    fn test_iterator_basic() {
        let vm_subnet = "192.168.1.0/30".parse::<Ipv4Net>().unwrap(); // 4 IPs, 2 pairs
        let ssh_range = 2222..2224; // 2 ports

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();
        let mut iter = ranges.iter();

        // First pair
        let ((host1, vm1), port1) = iter.next().unwrap();
        assert_eq!(host1.addr(), Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(vm1.addr(), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(host1.prefix_len(), 31);
        assert_eq!(vm1.prefix_len(), 31);
        assert_eq!(port1, 2222);

        // Second pair
        let ((host2, vm2), port2) = iter.next().unwrap();
        assert_eq!(host2.addr(), Ipv4Addr::new(192, 168, 1, 2));
        assert_eq!(vm2.addr(), Ipv4Addr::new(192, 168, 1, 3));
        assert_eq!(port2, 2223);

        // Should be exhausted
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_iterator_collects_correctly() {
        let vm_subnet = "10.0.0.0/29".parse::<Ipv4Net>().unwrap(); // 8 IPs, 4 pairs
        let ssh_range = 3000..3004; // 4 ports

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();
        let pairs: Vec<_> = ranges.iter().collect();

        assert_eq!(pairs.len(), 4);

        // Check that we get sequential IP pairs
        assert_eq!(pairs[0].0.0.addr(), Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(pairs[0].0.1.addr(), Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(pairs[0].1, 3000);

        assert_eq!(pairs[1].0.0.addr(), Ipv4Addr::new(10, 0, 0, 2));
        assert_eq!(pairs[1].0.1.addr(), Ipv4Addr::new(10, 0, 0, 3));
        assert_eq!(pairs[1].1, 3001);

        assert_eq!(pairs[3].0.0.addr(), Ipv4Addr::new(10, 0, 0, 6));
        assert_eq!(pairs[3].0.1.addr(), Ipv4Addr::new(10, 0, 0, 7));
        assert_eq!(pairs[3].1, 3003);
    }

    #[test]
    fn test_iterator_empty_ranges() {
        let vm_subnet = "192.168.1.0/31".parse::<Ipv4Net>().unwrap(); // 2 IPs, 1 pair
        let ssh_range = 2222..2223; // 1 port

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();
        let pairs: Vec<_> = ranges.iter().collect();

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.0.addr(), Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(pairs[0].0.1.addr(), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(pairs[0].1, 2222);
    }

    #[test]
    fn test_power_of_two_alignment() {
        // Test that minimization works with non-power-of-2 ranges
        let vm_subnet = "192.168.1.0/27".parse::<Ipv4Net>().unwrap(); // 32 IPs, 16 pairs
        let ssh_range = 2222..2233; // 11 ports (not power of 2)

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        // Should round down to 8 (largest power of 2 <= 11)
        assert_eq!(ranges.ssh_port_range.len(), 8);
        // Subnet should be adjusted accordingly for 8 pairs
        assert_eq!(ranges.vm_subnet.prefix_len(), 28); // 16 IPs, 8 pairs
    }

    #[test]
    fn test_large_ranges() {
        let vm_subnet = "10.0.0.0/16".parse::<Ipv4Net>().unwrap(); // 65536 IPs, 32768 pairs
        let ssh_range = 1024..2048; // 1024 ports

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        // Should be limited by port range
        assert_eq!(ranges.ssh_port_range.len(), 1024);
        // Should adjust subnet to support exactly 1024 pairs (2048 IPs)
        assert_eq!(ranges.vm_subnet.prefix_len(), 21); // 2048 IPs
    }

    #[test]
    fn test_edge_case_port_ranges() {
        let vm_subnet = "192.168.1.0/28".parse::<Ipv4Net>().unwrap(); // 16 IPs, 8 pairs
        let ssh_range = 65530..65535; // High port numbers

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();

        assert_eq!(ranges.ssh_port_range.start, 65530);
        // Should round down to 4 ports (largest power of 2 <= 5)
        assert_eq!(ranges.ssh_port_range.len(), 4);
    }

    #[test]
    fn test_iterator_handles_boundary_correctly() {
        let vm_subnet = "192.168.1.254/31".parse::<Ipv4Net>().unwrap(); // Edge of subnet
        let ssh_range = 2222..2223;

        let ranges = NetworkRanges::new(vm_subnet, ssh_range).unwrap();
        let mut iter = ranges.iter();

        let ((host, vm), port) = iter.next().unwrap();
        assert_eq!(host.addr(), Ipv4Addr::new(192, 168, 1, 254));
        assert_eq!(vm.addr(), Ipv4Addr::new(192, 168, 1, 255));
        assert_eq!(port, 2222);

        assert!(iter.next().is_none());
    }
}
