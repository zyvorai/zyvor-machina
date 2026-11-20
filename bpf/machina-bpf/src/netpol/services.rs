// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `kubectl get services,endpoints -A -o json` → [`NetpolService`]s.

use std::collections::BTreeMap;

use serde_json::Value;

use super::compile::{NetpolService, ServiceEndpoint};

fn proto(v: &Value) -> u8 {
    match v.as_str().unwrap_or("TCP").to_ascii_uppercase().as_str() {
        "UDP" => 17,
        "SCTP" => 132,
        _ => 6,
    }
}

fn ports(v: &Value) -> Vec<(u16, u8)> {
    let out: Vec<(u16, u8)> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                u16::try_from(p["port"].as_u64()?).ok()?,
                proto(&p["protocol"]),
            ))
        })
        .collect();
    if out.is_empty() {
        vec![(0, 0)]
    } else {
        out
    }
}

fn strs(v: &Value) -> impl Iterator<Item = &str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str)
}

fn labels(meta: &Value) -> BTreeMap<String, String> {
    meta["labels"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect()
}

/// Services with their frontends (cluster IPs, external IPs, load-balancer
/// ingress) and the backend addresses of the same-named Endpoints.
pub fn from_k8s(list: &Value) -> Vec<NetpolService> {
    let mut out: BTreeMap<(String, String), NetpolService> = BTreeMap::new();
    for item in list["items"].as_array().into_iter().flatten() {
        let meta = &item["metadata"];
        let (Some(name), Some(ns)) = (meta["name"].as_str(), meta["namespace"].as_str()) else {
            continue;
        };
        let mut addrs: Vec<(String, Vec<(u16, u8)>)> = Vec::new();
        match item["kind"].as_str() {
            Some("Service") => {
                let spec = &item["spec"];
                let mut fronts: Vec<&str> = strs(&spec["clusterIPs"]).collect();
                fronts.extend(spec["clusterIP"].as_str());
                fronts.extend(strs(&spec["externalIPs"]));
                fronts.extend(
                    item["status"]["loadBalancer"]["ingress"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|i| i["ip"].as_str()),
                );
                let p = ports(&spec["ports"]);
                for f in fronts.into_iter().filter(|f| !f.is_empty() && *f != "None") {
                    addrs.push((f.to_string(), p.clone()));
                }
            }
            Some("Endpoints") => {
                for sub in item["subsets"].as_array().into_iter().flatten() {
                    let p = ports(&sub["ports"]);
                    for a in sub["addresses"].as_array().into_iter().flatten() {
                        if let Some(ip) = a["ip"].as_str() {
                            addrs.push((ip.to_string(), p.clone()));
                        }
                    }
                }
            }
            _ => continue,
        }
        let svc = out
            .entry((ns.to_string(), name.to_string()))
            .or_insert_with(|| NetpolService {
                name: name.to_string(),
                namespace: ns.to_string(),
                ..Default::default()
            });
        if item["kind"] == "Service" || svc.labels.is_empty() {
            svc.labels = labels(meta);
        }
        for (address, ps) in addrs {
            for (port, proto) in ps {
                svc.endpoints.push(ServiceEndpoint {
                    address: address.clone(),
                    port,
                    proto,
                });
            }
        }
    }
    out.into_values()
        .map(|mut s| {
            s.endpoints.sort();
            s.endpoints.dedup();
            s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn services_and_endpoints() {
        let list = json!({"kind": "List", "items": [
            {"kind": "Service", "metadata": {"name": "web", "namespace": "shop", "labels": {"app": "web"}},
             "spec": {"clusterIP": "10.96.0.10", "clusterIPs": ["10.96.0.10"], "externalIPs": ["192.0.2.7"],
                      "ports": [{"port": 80, "protocol": "TCP"}, {"port": 53, "protocol": "UDP"}]},
             "status": {"loadBalancer": {"ingress": [{"ip": "198.51.100.1"}]}}},
            {"kind": "Endpoints", "metadata": {"name": "web", "namespace": "shop"},
             "subsets": [{"addresses": [{"ip": "10.0.0.5"}], "ports": [{"port": 8080}]}]},
            {"kind": "Service", "metadata": {"name": "db", "namespace": "shop"},
             "spec": {"clusterIP": "None"}},
        ]});
        let s = from_k8s(&list);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "db");
        assert!(s[0].endpoints.is_empty());
        let web = &s[1];
        assert_eq!(web.labels["app"], "web");
        let ep = |a: &str, port, proto| ServiceEndpoint {
            address: a.into(),
            port,
            proto,
        };
        for e in [
            ep("10.96.0.10", 80, 6),
            ep("10.96.0.10", 53, 17),
            ep("192.0.2.7", 80, 6),
            ep("198.51.100.1", 53, 17),
            ep("10.0.0.5", 8080, 6),
        ] {
            assert!(web.endpoints.contains(&e), "{e:?}");
        }
        assert_eq!(web.endpoints.len(), 7);
    }
}
