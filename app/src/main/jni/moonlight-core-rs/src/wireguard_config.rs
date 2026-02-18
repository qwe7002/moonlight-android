//! WireGuard configuration module
//!
//! This module contains the configuration structures and utilities for WireGuard tunnels.
//! Separated from the main wireguard module for better modularity and reusability.

use std::net::{IpAddr, SocketAddr};
use std::io;

/// Configuration for the WireGuard tunnel
#[derive(Clone, Debug)]
pub struct WireGuardConfig {
    /// Local private key (32 bytes, raw)
    pub private_key: [u8; 32],
    /// Peer public key (32 bytes, raw)
    pub peer_public_key: [u8; 32],
    /// Optional preshared key (32 bytes, raw)
    pub preshared_key: Option<[u8; 32]>,
    /// Peer endpoint (IP:port)
    pub endpoint: SocketAddr,
    /// Local tunnel IP address (the virtual IP assigned to this client)
    pub tunnel_address: IpAddr,
    /// Keepalive interval in seconds (0 = disabled)
    pub keepalive_secs: u16,
    /// MTU for the tunnel
    pub mtu: u16,
}

impl WireGuardConfig {
    /// Default keepalive interval in seconds
    pub const DEFAULT_KEEPALIVE_SECS: u16 = 25;

    /// Default MTU for the tunnel
    pub const DEFAULT_MTU: u16 = 1420;

    /// Create a new WireGuard configuration with the minimum required parameters.
    ///
    /// # Arguments
    /// * `private_key` - Local private key (32 bytes)
    /// * `peer_public_key` - Peer's public key (32 bytes)
    /// * `endpoint` - Peer endpoint address (IP:port)
    /// * `tunnel_address` - Local tunnel IP address
    pub fn new(
        private_key: [u8; 32],
        peer_public_key: [u8; 32],
        endpoint: SocketAddr,
        tunnel_address: IpAddr,
    ) -> Self {
        WireGuardConfig {
            private_key,
            peer_public_key,
            preshared_key: None,
            endpoint,
            tunnel_address,
            keepalive_secs: Self::DEFAULT_KEEPALIVE_SECS,
            mtu: Self::DEFAULT_MTU,
        }
    }

    /// Create a new WireGuard configuration from base64-encoded keys.
    ///
    /// # Arguments
    /// * `private_key_b64` - Base64-encoded private key
    /// * `peer_public_key_b64` - Base64-encoded peer public key
    /// * `endpoint` - Peer endpoint address (IP:port)
    /// * `tunnel_address` - Local tunnel IP address
    pub fn from_base64(
        private_key_b64: &str,
        peer_public_key_b64: &str,
        endpoint: SocketAddr,
        tunnel_address: IpAddr,
    ) -> io::Result<Self> {
        let private_key = decode_base64_key(private_key_b64)?;
        let peer_public_key = decode_base64_key(peer_public_key_b64)?;

        Ok(WireGuardConfig::new(
            private_key,
            peer_public_key,
            endpoint,
            tunnel_address,
        ))
    }

    /// Set the preshared key from raw bytes.
    pub fn with_preshared_key(mut self, psk: [u8; 32]) -> Self {
        self.preshared_key = Some(psk);
        self
    }

    /// Set the preshared key from a base64-encoded string.
    pub fn with_preshared_key_b64(mut self, psk_b64: &str) -> io::Result<Self> {
        let psk = decode_base64_key(psk_b64)?;
        self.preshared_key = Some(psk);
        Ok(self)
    }

    /// Set the keepalive interval in seconds.
    pub fn with_keepalive(mut self, secs: u16) -> Self {
        self.keepalive_secs = secs;
        self
    }

    /// Set the MTU for the tunnel.
    pub fn with_mtu(mut self, mtu: u16) -> Self {
        self.mtu = mtu;
        self
    }

    /// Validate the configuration.
    pub fn validate(&self) -> io::Result<()> {
        // Check that keys are not all zeros
        if self.private_key == [0u8; 32] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Private key cannot be all zeros",
            ));
        }
        if self.peer_public_key == [0u8; 32] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Peer public key cannot be all zeros",
            ));
        }

        // Check MTU is reasonable
        if self.mtu < 576 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MTU must be at least 576",
            ));
        }

        Ok(())
    }
}

impl Default for WireGuardConfig {
    fn default() -> Self {
        WireGuardConfig {
            private_key: [0u8; 32],
            peer_public_key: [0u8; 32],
            preshared_key: None,
            endpoint: "0.0.0.0:0".parse().unwrap(),
            tunnel_address: "10.0.0.2".parse().unwrap(),
            keepalive_secs: Self::DEFAULT_KEEPALIVE_SECS,
            mtu: Self::DEFAULT_MTU,
        }
    }
}

/// Decode a base64-encoded WireGuard key to a 32-byte array.
pub fn decode_base64_key(key_b64: &str) -> io::Result<[u8; 32]> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let decoded = STANDARD
        .decode(key_b64.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Invalid base64: {}", e)))?;

    if decoded.len() != 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Key must be 32 bytes, got {}", decoded.len()),
        ));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

/// Encode a 32-byte WireGuard key to base64.
pub fn encode_base64_key(key: &[u8; 32]) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    STANDARD.encode(key)
}

/// Generate a new WireGuard private key.
///
/// Note: This uses ring's secure random number generator.
pub fn generate_private_key() -> io::Result<[u8; 32]> {
    use ring::rand::{SecureRandom, SystemRandom};

    let rng = SystemRandom::new();
    let mut key = [0u8; 32];
    rng.fill(&mut key)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Failed to generate random key"))?;

    // Apply X25519 clamping as per RFC 7748
    key[0] &= 248;
    key[31] &= 127;
    key[31] |= 64;

    Ok(key)
}

/// Derive the public key from a private key.
pub fn derive_public_key(private_key: &[u8; 32]) -> [u8; 32] {
    use x25519_dalek::{PublicKey, StaticSecret};

    let secret = StaticSecret::from(*private_key);
    let public = PublicKey::from(&secret);
    *public.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_config_builder() {
        let private_key = [1u8; 32];
        let public_key = [2u8; 32];
        let endpoint: SocketAddr = "192.168.1.1:51820".parse().unwrap();
        let tunnel_addr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        let config = WireGuardConfig::new(private_key, public_key, endpoint, tunnel_addr)
            .with_keepalive(30)
            .with_mtu(1400);

        assert_eq!(config.keepalive_secs, 30);
        assert_eq!(config.mtu, 1400);
        assert!(config.preshared_key.is_none());
    }

    #[test]
    fn test_base64_key_roundtrip() {
        let original_key = [42u8; 32];
        let encoded = encode_base64_key(&original_key);
        let decoded = decode_base64_key(&encoded).unwrap();
        assert_eq!(original_key, decoded);
    }

    #[test]
    fn test_key_generation_and_derivation() {
        let private_key = generate_private_key().unwrap();
        let public_key = derive_public_key(&private_key);

        // Public key should not be all zeros
        assert_ne!(public_key, [0u8; 32]);
        // Public key should be different from private key
        assert_ne!(private_key, public_key);
    }

    #[test]
    fn test_config_validation() {
        let mut config = WireGuardConfig::default();

        // Should fail with zero keys
        assert!(config.validate().is_err());

        // Set valid keys
        config.private_key = [1u8; 32];
        config.peer_public_key = [2u8; 32];
        assert!(config.validate().is_ok());

        // Invalid MTU
        config.mtu = 100;
        assert!(config.validate().is_err());
    }
}

