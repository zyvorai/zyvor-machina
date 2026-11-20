// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Decode Prometheus Remote Write 1.0 and 2.0 (Snappy block compression).

use prom_remote_api::types::{Label, Sample, TimeSeries, WriteRequest};
use prost::Message;
use std::collections::HashMap;

use crate::prometheus_text::{host_percents_from_samples, PrometheusSample};

/// Prometheus Remote Write 2.0 messages (minimal subset for ingest).
pub mod write_v2 {
    use prost::Message;

    #[derive(Clone, PartialEq, Message)]
    pub struct Request {
        #[prost(string, repeated, tag = "4")]
        pub symbols: Vec<String>,
        #[prost(message, repeated, tag = "5")]
        pub timeseries: Vec<TimeSeries>,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct TimeSeries {
        #[prost(uint32, repeated, tag = "1")]
        pub labels_refs: Vec<u32>,
        #[prost(message, repeated, tag = "2")]
        pub samples: Vec<Sample>,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct Sample {
        #[prost(double, tag = "1")]
        pub value: f64,
        #[prost(int64, tag = "2")]
        pub timestamp: i64,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteWriteDecodeResult {
    pub protocol: &'static str,
    pub timeseries_count: usize,
    pub sample_count: usize,
    pub samples: Vec<PrometheusSample>,
}

/// Decompress Snappy and decode v1 or v2 from `Content-Type` (or auto-detect).
pub fn decode_remote_write_body(
    body: &[u8],
    content_type: Option<&str>,
) -> Result<RemoteWriteDecodeResult, String> {
    let proto = decompress_snappy(body)?;
    let ct = content_type.unwrap_or("");
    if ct.contains("io.prometheus.write.v2.Request") {
        return decode_v2(&proto);
    }
    if let Ok(req) = WriteRequest::decode(proto.as_slice()) {
        if !req.timeseries.is_empty() {
            return Ok(samples_from_write_request(&req));
        }
    }
    decode_v2(&proto)
}

fn decompress_snappy(body: &[u8]) -> Result<Vec<u8>, String> {
    let len = snap::raw::decompress_len(body).map_err(|e| format!("snappy length: {e}"))?;
    let mut out = vec![0u8; len];
    let mut dec = snap::raw::Decoder::new();
    let n = dec
        .decompress(body, &mut out)
        .map_err(|e| format!("snappy decompress: {e}"))?;
    out.truncate(n);
    Ok(out)
}

fn label_value<'a>(labels: &'a [Label], name: &str) -> Option<&'a str> {
    labels
        .iter()
        .find(|l| l.name == name)
        .map(|l| l.value.as_str())
}

fn labels_from_v2_refs(labels_refs: &[u32], symbols: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut i = 0usize;
    while i + 1 < labels_refs.len() {
        let ni = labels_refs[i] as usize;
        let vi = labels_refs[i + 1] as usize;
        if let (Some(n), Some(v)) = (symbols.get(ni), symbols.get(vi)) {
            out.insert(n.clone(), v.clone());
        }
        i += 2;
    }
    out
}

pub fn samples_from_write_request(req: &WriteRequest) -> RemoteWriteDecodeResult {
    let mut by_name: HashMap<String, f64> = HashMap::new();
    let mut sample_count = 0usize;
    for ts in &req.timeseries {
        let Some(metric) = label_value(&ts.labels, "__name__") else {
            continue;
        };
        if let Some(sample) = ts.samples.last() {
            sample_count += ts.samples.len();
            by_name.insert(metric.to_string(), sample.value);
        }
    }
    let samples: Vec<PrometheusSample> = by_name
        .into_iter()
        .map(|(name, value)| PrometheusSample { name, value })
        .collect();
    RemoteWriteDecodeResult {
        protocol: "prometheus.WriteRequest",
        timeseries_count: req.timeseries.len(),
        sample_count,
        samples,
    }
}

fn decode_v2(proto: &[u8]) -> Result<RemoteWriteDecodeResult, String> {
    let req = write_v2::Request::decode(proto).map_err(|e| format!("v2 protobuf: {e}"))?;
    let mut by_name: HashMap<String, f64> = HashMap::new();
    let mut sample_count = 0usize;
    for ts in &req.timeseries {
        let labels = labels_from_v2_refs(&ts.labels_refs, &req.symbols);
        let Some(metric) = labels.get("__name__") else {
            continue;
        };
        if let Some(sample) = ts.samples.last() {
            sample_count += ts.samples.len();
            by_name.insert(metric.clone(), sample.value);
        }
    }
    let samples: Vec<PrometheusSample> = by_name
        .into_iter()
        .map(|(name, value)| PrometheusSample { name, value })
        .collect();
    Ok(RemoteWriteDecodeResult {
        protocol: "io.prometheus.write.v2.Request",
        timeseries_count: req.timeseries.len(),
        sample_count,
        samples,
    })
}

pub fn encode_remote_write_body(req: &WriteRequest) -> Result<Vec<u8>, String> {
    compress_proto(&req.encode_to_vec())
}

pub fn encode_remote_write_v2_body(req: &write_v2::Request) -> Result<Vec<u8>, String> {
    compress_proto(&req.encode_to_vec())
}

fn compress_proto(proto: &[u8]) -> Result<Vec<u8>, String> {
    let max = snap::raw::max_compress_len(proto.len());
    let mut out = vec![0u8; max];
    let mut enc = snap::raw::Encoder::new();
    let n = enc
        .compress(proto, &mut out)
        .map_err(|e| format!("snappy compress: {e}"))?;
    out.truncate(n);
    Ok(out)
}

pub fn write_request_with_gauge(name: &str, value: f64, timestamp_ms: i64) -> WriteRequest {
    WriteRequest {
        timeseries: vec![TimeSeries {
            labels: vec![
                Label {
                    name: "__name__".into(),
                    value: name.into(),
                },
                Label {
                    name: "job".into(),
                    value: "machina".into(),
                },
            ],
            samples: vec![Sample {
                value,
                timestamp: timestamp_ms,
            }],
            exemplars: Vec::new(),
        }],
        metadata: Vec::new(),
    }
}

pub fn write_request_v2_with_gauge(name: &str, value: f64, timestamp_ms: i64) -> write_v2::Request {
    write_v2::Request {
        symbols: vec![
            String::new(),
            "__name__".into(),
            name.into(),
            "job".into(),
            "machina".into(),
        ],
        timeseries: vec![write_v2::TimeSeries {
            labels_refs: vec![1, 2, 3, 4],
            samples: vec![write_v2::Sample {
                value,
                timestamp: timestamp_ms,
            }],
        }],
    }
}

pub fn host_percents_from_remote_write(
    result: &RemoteWriteDecodeResult,
) -> Option<(f64, f64, f64)> {
    host_percents_from_samples(&result.samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_remote_write_v1_gauge() {
        let req = write_request_with_gauge("machina_host_cpu_percent", 55.5, 1_700_000_000_000);
        let body = encode_remote_write_body(&req).unwrap();
        let decoded = decode_remote_write_body(&body, None).unwrap();
        assert_eq!(decoded.protocol, "prometheus.WriteRequest");
        assert!((decoded.samples[0].value - 55.5).abs() < 0.01);
    }

    #[test]
    fn roundtrip_remote_write_v2_gauge() {
        let req = write_request_v2_with_gauge("machina_host_cpu_percent", 77.0, 1_700_000_000_000);
        let body = encode_remote_write_v2_body(&req).unwrap();
        let decoded = decode_remote_write_body(
            &body,
            Some("application/x-protobuf;proto=io.prometheus.write.v2.Request"),
        )
        .unwrap();
        assert_eq!(decoded.protocol, "io.prometheus.write.v2.Request");
        assert!((decoded.samples[0].value - 77.0).abs() < 0.01);
    }

    #[test]
    fn extracts_host_triplet_from_v2() {
        let mut req = write_v2::Request {
            symbols: vec![String::new()],
            timeseries: Vec::new(),
        };
        for (name, val) in [
            ("machina_host_cpu_percent", 10.0),
            ("machina_host_memory_percent", 20.0),
            ("machina_host_disk_percent", 30.0),
        ] {
            let ni = req.symbols.len() as u32;
            req.symbols.push("__name__".into());
            let vi = req.symbols.len() as u32;
            req.symbols.push(name.into());
            req.timeseries.push(write_v2::TimeSeries {
                labels_refs: vec![ni, vi],
                samples: vec![write_v2::Sample {
                    value: val,
                    timestamp: 1,
                }],
            });
        }
        let body = encode_remote_write_v2_body(&req).unwrap();
        let decoded = decode_remote_write_body(
            &body,
            Some("application/x-protobuf;proto=io.prometheus.write.v2.Request"),
        )
        .unwrap();
        let (c, m, d) = host_percents_from_remote_write(&decoded).unwrap();
        assert!((c - 10.0).abs() < 0.01);
        assert!((m - 20.0).abs() < 0.01);
        assert!((d - 30.0).abs() < 0.01);
    }
}
