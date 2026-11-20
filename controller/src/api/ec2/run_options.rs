// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The `RunInstances` parameters beyond image, type, count, key, user data and subnet. Each one is either applied
//! or refused with a clear error; none is dropped silently, because a client (Terraform above all) records what it
//! asked for and would otherwise show a permanent difference from what was built.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::state::AppState;

use super::Ec2Error;

type Params = BTreeMap<String, String>;

fn unsupported(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("UnsupportedOperation", msg)
}

/// An extra volume from `BlockDeviceMapping` (anything but the root device).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ExtraVolume {
    /// `vdb`…`vdz`.
    pub device: String,
    pub size_gib: i64,
    pub delete_on_termination: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunOptions {
    pub preemptible: bool,
    pub volumes: Vec<ExtraVolume>,
    /// Tags from `TagSpecification` of `ResourceType=volume`, for the volumes above.
    pub volume_tags: BTreeMap<String, String>,
    /// Per-instance attributes the call sets (`ec2_instance_attrs`), written once the instance exists.
    pub attrs: super::instance_attrs::Attrs,
}

fn truthy(v: Option<&String>) -> bool {
    v.is_some_and(|s| s == "true")
}

/// Device names clients use → (`vdX`, is the root device).
pub fn normalize_device(name: &str) -> Option<(String, bool)> {
    let d = name.strip_prefix("/dev/").unwrap_or(name);
    let letter = ["sd", "xvd", "vd"].iter().find_map(|p| d.strip_prefix(p))?;
    let mut chars = letter.chars();
    let c = chars.next()?;
    if !c.is_ascii_lowercase() {
        return None;
    }
    // `/dev/sda1` is the root partition of the root device `a`
    let rest: String = chars.collect();
    if !rest.is_empty() && !rest.chars().all(|x| x.is_ascii_digit()) {
        return None;
    }
    Some((format!("vd{c}"), c == 'a'))
}

/// Parameters that cannot be honoured, refused before anything is created.
pub fn reject_unsupported(p: &Params) -> Result<(), Ec2Error> {
    for key in p.keys() {
        let k = key.as_str();
        if k.starts_with("IamInstanceProfile") {
            return Err(unsupported("IamInstanceProfile is not supported: instances have no IAM roles"));
        }
        if k == "PrivateIpAddress" || k.starts_with("Ipv6") || k.starts_with("PrivateIpAddresses") {
            return Err(unsupported(format!("{k} is not supported when launching: assign addresses through a subnet or network interface after launch")));
        }
        if k.starts_with("SecurityGroup.") {
            return Err(unsupported("SecurityGroup names are not supported: use SecurityGroupId.N"));
        }
        if k.starts_with("NetworkInterface.") {
            return Err(unsupported("NetworkInterface at launch is not supported: use SubnetId and SecurityGroupId"));
        }
        if k.starts_with("CpuOptions") || k.starts_with("HibernationOptions") || k.starts_with("EnclaveOptions") || k.starts_with("LicenseSpecification") || k.starts_with("CapacityReservationSpecification") || k.starts_with("ElasticGpuSpecification") || k.starts_with("ElasticInferenceAccelerator") {
            return Err(unsupported(format!("{k} is not supported")));
        }
    }
    if p.get("MetadataOptions.HttpTokens").is_some_and(|v| v == "required") {
        return Err(unsupported("MetadataOptions.HttpTokens=required is not supported: the metadata service answers without a token"));
    }
    if p.get("MetadataOptions.InstanceMetadataTags").is_some_and(|v| v == "enabled") || p.get("MetadataOptions.HttpProtocolIpv6").is_some_and(|v| v == "enabled") {
        return Err(unsupported("MetadataOptions.InstanceMetadataTags and HttpProtocolIpv6 are not supported"));
    }
    if p.get("InstanceInitiatedShutdownBehavior").is_some_and(|v| v == "terminate") {
        return Err(unsupported("InstanceInitiatedShutdownBehavior=terminate is not supported: a guest shutdown always leaves the instance stopped"));
    }
    for k in ["Placement.HostId", "Placement.HostResourceGroupArn", "Placement.PartitionNumber", "Placement.Affinity"] {
        if p.get(k).is_some_and(|v| !v.is_empty()) {
            return Err(unsupported(format!("{k} is not supported")));
        }
    }
    if p.get("Placement.Tenancy").is_some_and(|v| v != "default") {
        return Err(unsupported("Placement.Tenancy other than default is not supported"));
    }
    Ok(())
}

/// `InstanceMarketOptions` → preemptible (a spot instance is the cluster's preemptible instance).
pub fn spot(p: &Params) -> Result<bool, Ec2Error> {
    let Some(kind) = p.get("InstanceMarketOptions.MarketType") else {
        if p.keys().any(|k| k.starts_with("InstanceMarketOptions.")) {
            return Err(Ec2Error::bad("MissingParameter", "InstanceMarketOptions.MarketType is required"));
        }
        return Ok(false);
    };
    if kind != "spot" {
        return Err(Ec2Error::bad("InvalidParameterValue", "InstanceMarketOptions.MarketType must be spot"));
    }
    if p.get("InstanceMarketOptions.SpotOptions.SpotInstanceType").is_some_and(|v| v != "one-time") {
        return Err(unsupported("only one-time spot instances are supported"));
    }
    if p.get("InstanceMarketOptions.SpotOptions.InstanceInterruptionBehavior").is_some_and(|v| v != "terminate") {
        return Err(unsupported("a preempted instance is terminated: InstanceInterruptionBehavior must be terminate"));
    }
    if p.contains_key("InstanceMarketOptions.SpotOptions.BlockDurationMinutes") || p.contains_key("InstanceMarketOptions.SpotOptions.ValidUntil") {
        return Err(unsupported("spot block durations and expiry are not supported"));
    }
    Ok(true)
}

/// `TagSpecification.N` of resource types other than instance and volume carry tags we cannot place.
fn check_tag_specifications(p: &Params) -> Result<BTreeMap<String, String>, Ec2Error> {
    let mut volume_tags = BTreeMap::new();
    for n in 1..=10 {
        let Some(rt) = p.get(&format!("TagSpecification.{n}.ResourceType")) else { break };
        let has_tags = p.keys().any(|k| k.starts_with(&format!("TagSpecification.{n}.Tag.")));
        match rt.as_str() {
            "instance" => {}
            "volume" => {
                for m in 1..=50 {
                    let Some(k) = p.get(&format!("TagSpecification.{n}.Tag.{m}.Key")) else { break };
                    volume_tags.insert(k.clone(), p.get(&format!("TagSpecification.{n}.Tag.{m}.Value")).cloned().unwrap_or_default());
                }
            }
            other if has_tags => {
                return Err(unsupported(format!("TagSpecification for {other} is not supported at launch: tag it after launch")));
            }
            _ => {}
        }
    }
    Ok(volume_tags)
}

/// `BlockDeviceMapping.N` → the extra volumes to create and attach.
pub fn block_devices(p: &Params) -> Result<Vec<ExtraVolume>, Ec2Error> {
    let mut out: Vec<ExtraVolume> = Vec::new();
    for n in 1..=24 {
        let pre = format!("BlockDeviceMapping.{n}");
        let Some(name) = p.get(&format!("{pre}.DeviceName")) else {
            if p.keys().any(|k| k.starts_with(&format!("{pre}."))) {
                return Err(Ec2Error::bad("MissingParameter", format!("{pre}.DeviceName is required")));
            }
            break;
        };
        if p.contains_key(&format!("{pre}.NoDevice")) || p.contains_key(&format!("{pre}.VirtualName")) {
            return Err(unsupported(format!("{pre}: NoDevice and VirtualName (ephemeral disks) are not supported")));
        }
        for k in ["Ebs.SnapshotId", "Ebs.Iops", "Ebs.Throughput", "Ebs.KmsKeyId", "Ebs.OutpostArn"] {
            if p.contains_key(&format!("{pre}.{k}")) {
                return Err(unsupported(format!("{pre}.{k} is not supported")));
            }
        }
        if truthy(p.get(&format!("{pre}.Ebs.Encrypted"))) {
            return Err(unsupported(format!("{pre}.Ebs.Encrypted is not supported")));
        }
        let (device, root) = normalize_device(name).ok_or_else(|| Ec2Error::bad("InvalidBlockDeviceMapping", format!("The device name '{name}' is not valid")))?;
        let size = p.get(&format!("{pre}.Ebs.VolumeSize"));
        let dot = p.get(&format!("{pre}.Ebs.DeleteOnTermination")).map(|v| v == "true");
        if root {
            if size.is_some() {
                return Err(unsupported(format!("{pre}.Ebs.VolumeSize: the root disk takes its size from the image; resize it after launch")));
            }
            if dot == Some(false) {
                return Err(unsupported(format!("{pre}.Ebs.DeleteOnTermination=false for the root disk is not supported")));
            }
            continue;
        }
        let size: i64 = size
            .ok_or_else(|| Ec2Error::bad("MissingParameter", format!("{pre}.Ebs.VolumeSize is required")))?
            .parse()
            .map_err(|_| Ec2Error::bad("InvalidParameterValue", format!("{pre}.Ebs.VolumeSize must be a number of GiB")))?;
        if !(1..=16384).contains(&size) {
            return Err(Ec2Error::bad("InvalidParameterValue", format!("{pre}.Ebs.VolumeSize must be between 1 and 16384")));
        }
        if out.iter().any(|v| v.device == device) {
            return Err(Ec2Error::bad("InvalidBlockDeviceMapping", format!("The device '{name}' is mapped twice")));
        }
        out.push(ExtraVolume { device, size_gib: size, delete_on_termination: dot.unwrap_or(true) });
    }
    Ok(out)
}

/// Everything except the placement lookup, which needs the database.
pub fn parse(p: &Params) -> Result<RunOptions, Ec2Error> {
    reject_unsupported(p)?;
    Ok(RunOptions { preemptible: spot(p)?, volumes: block_devices(p)?, volume_tags: check_tag_specifications(p)?, attrs: attrs(p)? })
}

/// The attributes a launch sets: termination protection, EBS-optimized and monitoring flags (recorded), the metadata
/// endpoint switch (enforced by the agent) and the placement group (checked against the groups that exist at launch).
pub fn attrs(p: &Params) -> Result<super::instance_attrs::Attrs, Ec2Error> {
    let mut a = super::instance_attrs::Attrs {
        disable_api_termination: truthy(p.get("DisableApiTermination")),
        ebs_optimized: truthy(p.get("EbsOptimized")),
        monitoring: truthy(p.get("Monitoring.Enabled")),
        metadata_endpoint: p.get("MetadataOptions.HttpEndpoint").map_or(true, |v| v != "disabled"),
        ..Default::default()
    };
    if let Some(v) = p.get("MetadataOptions.HttpPutResponseHopLimit") {
        a.metadata_hop_limit = v
            .parse::<i64>()
            .ok()
            .filter(|n| (1..=64).contains(n))
            .ok_or_else(|| Ec2Error::bad("InvalidParameterValue", "MetadataOptions.HttpPutResponseHopLimit must be between 1 and 64"))?;
    }
    a.placement_group = p.get("Placement.GroupName").filter(|g| !g.is_empty()).cloned();
    Ok(a)
}

/// `Placement.AvailabilityZone` names a host (a zone is a host).
pub async fn host_for_zone(state: &AppState, p: &Params) -> Result<Option<Uuid>, Ec2Error> {
    let Some(zone) = p.get("Placement.AvailabilityZone").filter(|z| !z.is_empty()) else { return Ok(None) };
    let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM hosts WHERE hostname = ?").bind(zone).fetch_optional(&state.pool).await?;
    id.map(Some).ok_or_else(|| Ec2Error::bad("InvalidParameterValue", format!("Invalid availability zone: '{zone}'")))
}

/// The launch template's data for a `RunInstances` call: `LaunchTemplate.LaunchTemplateId|LaunchTemplateName` plus
/// `Version` (`$Latest`, `$Default` or a number). Parameters the request sets win, family by family.
pub async fn with_launch_template(state: &AppState, p: &Params) -> Result<Params, Ec2Error> {
    match super::launch_templates::run_params(state, p, "LaunchTemplate.").await? {
        Some(data) => Ok(super::launch_templates::merge_under(p, &data)),
        None => Ok(p.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn device_names_from_every_client_convention() {
        assert_eq!(normalize_device("/dev/sdb"), Some(("vdb".into(), false)));
        assert_eq!(normalize_device("/dev/xvdc"), Some(("vdc".into(), false)));
        assert_eq!(normalize_device("vdd"), Some(("vdd".into(), false)));
        assert_eq!(normalize_device("/dev/sda1"), Some(("vda".into(), true)));
        assert_eq!(normalize_device("/dev/xvda"), Some(("vda".into(), true)));
        assert_eq!(normalize_device("/dev/nvme1n1"), None);
        assert_eq!(normalize_device("/dev/sdb-x"), None);
    }

    #[test]
    fn unsupported_parameters_are_refused_not_dropped() {
        for (k, v) in [
            ("IamInstanceProfile.Name", "role"),
            ("PrivateIpAddress", "10.0.0.5"),
            ("NetworkInterface.1.DeviceIndex", "0"),
            ("MetadataOptions.HttpTokens", "required"),
            ("InstanceInitiatedShutdownBehavior", "terminate"),
            ("Placement.PartitionNumber", "1"),
            ("Placement.Tenancy", "dedicated"),
            ("CpuOptions.CoreCount", "2"),
        ] {
            let e = reject_unsupported(&p(&[(k, v)])).unwrap_err();
            assert_eq!(e.code, "UnsupportedOperation", "{k}");
        }
        // values that match what the platform does anyway pass
        assert!(reject_unsupported(&p(&[("DisableApiTermination", "false"), ("MetadataOptions.HttpTokens", "optional"), ("Placement.Tenancy", "default"), ("Monitoring.Enabled", "false")])).is_ok());
    }

    #[test]
    fn attributes_a_launch_sets_are_applied_not_refused() {
        let a = attrs(&p(&[
            ("DisableApiTermination", "true"),
            ("Monitoring.Enabled", "true"),
            ("EbsOptimized", "true"),
            ("MetadataOptions.HttpEndpoint", "disabled"),
            ("MetadataOptions.HttpPutResponseHopLimit", "2"),
            ("Placement.GroupName", "pg1"),
        ]))
        .unwrap();
        assert!(a.disable_api_termination && a.monitoring && a.ebs_optimized && !a.metadata_endpoint);
        assert_eq!((a.metadata_hop_limit, a.placement_group.as_deref()), (2, Some("pg1")));
        assert_eq!(attrs(&p(&[])).unwrap(), crate::api::ec2::instance_attrs::Attrs::default());
        assert!(attrs(&p(&[("MetadataOptions.HttpPutResponseHopLimit", "0")])).is_err());
        assert!(reject_unsupported(&p(&[("DisableApiTermination", "true"), ("Monitoring.Enabled", "true"), ("MetadataOptions.HttpEndpoint", "disabled"), ("Placement.GroupName", "pg")])).is_ok());
    }

    #[test]
    fn spot_becomes_preemptible() {
        assert!(!spot(&p(&[])).unwrap());
        assert!(spot(&p(&[("InstanceMarketOptions.MarketType", "spot")])).unwrap());
        assert!(spot(&p(&[("InstanceMarketOptions.MarketType", "spot"), ("InstanceMarketOptions.SpotOptions.MaxPrice", "0.1")])).unwrap());
        assert_eq!(spot(&p(&[("InstanceMarketOptions.MarketType", "capacity-block")])).unwrap_err().code, "InvalidParameterValue");
        assert_eq!(spot(&p(&[("InstanceMarketOptions.SpotOptions.MaxPrice", "1")])).unwrap_err().code, "MissingParameter");
        assert_eq!(
            spot(&p(&[("InstanceMarketOptions.MarketType", "spot"), ("InstanceMarketOptions.SpotOptions.SpotInstanceType", "persistent")])).unwrap_err().code,
            "UnsupportedOperation"
        );
    }

    #[test]
    fn block_devices_extra_volumes_and_root_rules() {
        let ok = p(&[
            ("BlockDeviceMapping.1.DeviceName", "/dev/sda1"),
            ("BlockDeviceMapping.1.Ebs.DeleteOnTermination", "true"),
            ("BlockDeviceMapping.1.Ebs.VolumeType", "gp3"),
            ("BlockDeviceMapping.2.DeviceName", "/dev/sdb"),
            ("BlockDeviceMapping.2.Ebs.VolumeSize", "50"),
            ("BlockDeviceMapping.3.DeviceName", "/dev/xvdc"),
            ("BlockDeviceMapping.3.Ebs.VolumeSize", "10"),
            ("BlockDeviceMapping.3.Ebs.DeleteOnTermination", "false"),
        ]);
        assert_eq!(
            block_devices(&ok).unwrap(),
            vec![
                ExtraVolume { device: "vdb".into(), size_gib: 50, delete_on_termination: true },
                ExtraVolume { device: "vdc".into(), size_gib: 10, delete_on_termination: false },
            ]
        );
        let root_size = p(&[("BlockDeviceMapping.1.DeviceName", "/dev/sda1"), ("BlockDeviceMapping.1.Ebs.VolumeSize", "100")]);
        assert_eq!(block_devices(&root_size).unwrap_err().code, "UnsupportedOperation");
        let keep_root = p(&[("BlockDeviceMapping.1.DeviceName", "/dev/sda1"), ("BlockDeviceMapping.1.Ebs.DeleteOnTermination", "false")]);
        assert_eq!(block_devices(&keep_root).unwrap_err().code, "UnsupportedOperation");
        let no_size = p(&[("BlockDeviceMapping.1.DeviceName", "/dev/sdb")]);
        assert_eq!(block_devices(&no_size).unwrap_err().code, "MissingParameter");
        let snap = p(&[("BlockDeviceMapping.1.DeviceName", "/dev/sdb"), ("BlockDeviceMapping.1.Ebs.VolumeSize", "5"), ("BlockDeviceMapping.1.Ebs.SnapshotId", "snap-1")]);
        assert_eq!(block_devices(&snap).unwrap_err().code, "UnsupportedOperation");
        let enc = p(&[("BlockDeviceMapping.1.DeviceName", "/dev/sdb"), ("BlockDeviceMapping.1.Ebs.VolumeSize", "5"), ("BlockDeviceMapping.1.Ebs.Encrypted", "true")]);
        assert_eq!(block_devices(&enc).unwrap_err().code, "UnsupportedOperation");
        let twice = p(&[
            ("BlockDeviceMapping.1.DeviceName", "/dev/sdb"),
            ("BlockDeviceMapping.1.Ebs.VolumeSize", "5"),
            ("BlockDeviceMapping.2.DeviceName", "/dev/xvdb"),
            ("BlockDeviceMapping.2.Ebs.VolumeSize", "5"),
        ]);
        assert_eq!(block_devices(&twice).unwrap_err().code, "InvalidBlockDeviceMapping");
        let bad_size = p(&[("BlockDeviceMapping.1.DeviceName", "/dev/sdb"), ("BlockDeviceMapping.1.Ebs.VolumeSize", "0")]);
        assert!(block_devices(&bad_size).is_err());
    }

    #[test]
    fn tag_specifications_for_volumes_are_kept_and_others_refused() {
        let ok = p(&[
            ("TagSpecification.1.ResourceType", "instance"),
            ("TagSpecification.1.Tag.1.Key", "Name"),
            ("TagSpecification.1.Tag.1.Value", "web"),
            ("TagSpecification.2.ResourceType", "volume"),
            ("TagSpecification.2.Tag.1.Key", "Env"),
            ("TagSpecification.2.Tag.1.Value", "prod"),
        ]);
        let o = parse(&ok).unwrap();
        assert_eq!(o.volume_tags.get("Env").map(String::as_str), Some("prod"));
        assert!(!o.volume_tags.contains_key("Name"));
        let eni = p(&[("TagSpecification.1.ResourceType", "network-interface"), ("TagSpecification.1.Tag.1.Key", "A"), ("TagSpecification.1.Tag.1.Value", "b")]);
        assert_eq!(parse(&eni).unwrap_err().code, "UnsupportedOperation");
    }
}
