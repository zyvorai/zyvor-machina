// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The uplink XDP dispatcher (`mn_xdp_uplink`): one XDP program per
//! interface, with the DDoS shield inline and the NodePort fast path behind
//! a tail call. Features toggle bits in `XDP_CFG`.

use super::*;

#[derive(Default)]
pub(super) struct UplinkRuntime {
    /// Interface carrying the dispatcher and its feature bits.
    pub iface: Option<String>,
    pub flags: u32,
}

impl Engine {
    /// Turn one dispatcher feature on or off for `iface`; the dispatcher is
    /// attached while any feature is on and detached when none is.
    pub(super) fn xdp_uplink_set(&mut self, iface: &str, bit: u32, on: bool) -> Result<()> {
        if let Some(cur) = self.uplink.iface.clone() {
            if cur != iface && self.uplink.flags != 0 {
                return Err(anyhow!(
                    "uplink XDP already runs on {cur}; one uplink per host"
                ));
            }
        }
        if if_nametoindex(iface).is_none() {
            return Err(anyhow!("interface {iface} not found"));
        }
        let flags = if on {
            self.uplink.flags | bit
        } else {
            self.uplink.flags & !bit
        };
        if bit == XDP_F_NODEPORT && on {
            self.dp.xdp_set_slot(XDP_SLOT_NODEPORT, "mn_xdp_nodeport")?;
        }
        if bit == XDP_F_QUICLB && on {
            self.dp.xdp_set_slot(XDP_SLOT_QUICLB, "mn_xdp_quiclb")?;
        }
        self.dp.xdp_set_cfg(XdpCfg { flags, _pad: 0 })?;
        if flags == 0 {
            if self.dp.xdp_attached(iface) == Some("mn_xdp_uplink") {
                self.dp.detach_xdp(iface);
            }
        } else {
            match self.dp.xdp_attached(iface) {
                Some("mn_xdp_uplink") => {}
                Some(other) => {
                    return Err(anyhow!(
                        "{iface} already has XDP program {other}; detach it first"
                    ));
                }
                None => self.dp.attach_xdp_prog(iface, "mn_xdp_uplink")?,
            }
        }
        self.uplink.iface = Some(iface.to_string());
        self.uplink.flags = flags;
        Ok(())
    }

    pub(super) fn xdp_uplink_refresh(&mut self, iface: &str, nodeport: bool) -> Result<()> {
        self.xdp_uplink_set(iface, XDP_F_NODEPORT, nodeport)
    }
}
