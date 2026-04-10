#[cfg(target_os = "windows")]
compile_error!("'orch_wg' is only compatible with not-windows currently.");

use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::UNIX_EPOCH,
};

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use defguard_wireguard_rs::{
    InterfaceConfiguration, WireguardInterfaceApi, host::Peer, key::Key, net::IpAddrMask,
};

pub use defguard_wireguard_rs::error::WireguardInterfaceError;
use serde::Serialize;
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Clone)]
pub struct WG(Arc<InnerWG>);

#[cfg(target_os = "linux")]
type WGAPI = defguard_wireguard_rs::WGApi<defguard_wireguard_rs::Kernel>;

#[cfg(target_os = "macos")]
type WGAPI = defguard_wireguard_rs::WGApi<defguard_wireguard_rs::Userspace>;

pub struct InnerWG {
    wg: WGAPI,
}

pub struct WgPeer {
    pub endpoint_ip: IpAddr,
    pub pub_key: String,
    pub remote_ipv6: Ipv6Addr,
    pub port: u16,
}

impl WgPeer {
    pub fn endpoint(&self) -> SocketAddr {
        SocketAddr::new(self.endpoint_ip.clone(), self.port)
    }
}

impl WG {
    // This should prob be 64, but it works.
    const WG_IPV6_CIDR: u8 = 128;

    pub fn new(
        ifname: &str,
        wg_ipv6: Ipv6Addr,
        prvkey: String,
        port: u16,
    ) -> Result<Self, WireguardInterfaceError> {
        Self::new_with_peers(ifname, wg_ipv6, prvkey, port, vec![])
    }

    pub fn new_with_peers(
        #[allow(unused)] ifname: &str,
        wg_ipv6: Ipv6Addr,
        prvkey: String,
        port: u16,
        peers: Vec<WgPeer>,
    ) -> Result<Self, WireguardInterfaceError> {
        #[cfg(target_vendor = "apple")]
        let ifname = "utun7";
        let wgapi = WGAPI::new(ifname.to_string()).unwrap();

        wgapi.create_interface().unwrap();

        let _peers: Vec<Result<Peer, WireguardInterfaceError>> =
            peers.into_iter().map(Self::into_peer).collect();
        let mut peers = Vec::with_capacity(_peers.len());

        for peer in _peers {
            peers.push(peer?);
        }

        let interface_config = InterfaceConfiguration {
            name: ifname.to_string(),
            prvkey,
            addresses: [IpAddrMask::new(IpAddr::V6(wg_ipv6), Self::WG_IPV6_CIDR)].to_vec(),

            // defguard_wireguard_rs thinks it is u32, but ports are only u16
            port: port as u32,
            peers,
            mtu: None,
        };

        wgapi.configure_interface(&interface_config)?;
        wgapi.configure_peer_routing(&interface_config.peers)?;

        Ok(Self(Arc::new(InnerWG { wg: wgapi })))
    }

    fn into_pub_key(pub_key: &str) -> Result<Key, WireguardInterfaceError> {
        let pub_key = B64.decode(pub_key)?;
        let len = pub_key.len();
        let bytes: [u8; 32] = pub_key.try_into().map_err(|_| {
            WireguardInterfaceError::KeyDecode(base64::DecodeError::InvalidLength(len))
        })?;

        Ok(Key::new(bytes))
    }

    fn into_peer(wg_peer: WgPeer) -> Result<Peer, WireguardInterfaceError> {
        let mut peer = Peer::new(Self::into_pub_key(&wg_peer.pub_key)?);

        peer.set_endpoint(&wg_peer.endpoint().to_string())?;

        peer.set_allowed_ips(vec![IpAddrMask::new(
            IpAddr::V6(wg_peer.remote_ipv6),
            Self::WG_IPV6_CIDR,
        )]);
        peer.persistent_keepalive_interval = Some(45);

        Ok(peer)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn peer_ensure(&self, wg_peer: WgPeer) -> Result<(), WireguardInterfaceError> {
        let remote_ipv6 = wg_peer.remote_ipv6;
        let peer = Self::into_peer(wg_peer)?;

        if let Err(err) = self.0.wg.configure_peer(&peer) {
            tracing::error!(?err, "configuring endpoint");
            return Err(err);
        }

        if let Err(err) = self.0.wg.configure_peer_routing(&[peer]) {
            tracing::error!(?err, "configuring peer routing");
            return Err(err);
        }

        tracing::info!(remote_ipv6 = ?remote_ipv6, "added peer");

        Ok(())
    }

    pub fn list_peers(&self) -> Result<Vec<WireGuardPeerInfo>, WireguardInterfaceError> {
        let host = self.0.wg.read_interface_data()?;
        let peers = host
            .peers
            .values()
            .map(|peer| WireGuardPeerInfo {
                public_key: peer.public_key.to_string(),
                preshared_key: peer.preshared_key.as_ref().map(|key| key.to_string()),
                endpoint: peer.endpoint.as_ref().map(|addr| addr.to_string()),
                allowed_ips: peer.allowed_ips.iter().map(|ip| ip.to_string()).collect(),
                last_handshake_epoch_secs: peer.last_handshake.and_then(|handshake| {
                    handshake
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|duration| duration.as_secs())
                }),
                rx_bytes: peer.rx_bytes,
                tx_bytes: peer.tx_bytes,
                persistent_keepalive_interval: peer.persistent_keepalive_interval,
                protocol_version: peer.protocol_version,
            })
            .collect();
        Ok(peers)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn clear(&self) {
        let _ = self.0.wg.remove_interface();
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn peer_remove(&self, pubkey: &str) -> Result<(), WireguardInterfaceError> {
        if let Err(err) = self.0.wg.remove_peer(&Self::into_pub_key(pubkey)?) {
            tracing::error!(?err, "configuring peer routing");
            return Err(err);
        };

        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct WireGuardPeerInfo {
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub last_handshake_epoch_secs: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub persistent_keepalive_interval: Option<u16>,
    pub protocol_version: Option<u32>,
}

impl Drop for InnerWG {
    fn drop(&mut self) {
        tracing::info!("removing interface");
        if let Err(err) = self.wg.remove_interface() {
            tracing::warn!(?err, "wg error while removing interface");
        };
    }
}

pub fn gen_private_key() -> String {
    B64.encode(StaticSecret::random())
}

#[derive(thiserror::Error, Debug)]
#[error("Invalid private key")]
pub struct InvalidPrivateKey;

pub fn gen_public_key(private_key: &String) -> Result<String, InvalidPrivateKey> {
    let private_key_bytes: [u8; 32] = B64
        .decode(private_key)
        .map_err(|_| InvalidPrivateKey)?
        .try_into()
        .map_err(|_| InvalidPrivateKey)?;

    let private_key = StaticSecret::from(private_key_bytes);

    Ok(B64.encode(PublicKey::from(&private_key)))
}

#[test]
fn priv_pub_gen_key() {
    let _priv = gen_private_key();
    let _pub = gen_public_key(&_priv);
}
