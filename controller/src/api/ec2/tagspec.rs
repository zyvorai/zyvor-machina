// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `TagSpecification.N` for the resource types a call creates, and reading a resource's tags back as `<tagSet>`.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::resource_ids::Kind;
use crate::state::AppState;

use super::{xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

/// Tags of `TagSpecification.N` whose `ResourceType` is `resource_type` (plus none from the plain `Tag.N`, which belong
/// to the call's main resource and are read by the caller).
pub fn tags_for(p: &Params, resource_type: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for n in 1..=10 {
        let Some(rt) = p.get(&format!("TagSpecification.{n}.ResourceType")) else { break };
        if rt != resource_type {
            continue;
        }
        for m in 1..=50 {
            let Some(k) = p.get(&format!("TagSpecification.{n}.Tag.{m}.Key")) else { break };
            out.insert(k.clone(), p.get(&format!("TagSpecification.{n}.Tag.{m}.Value")).cloned().unwrap_or_default());
        }
    }
    out
}

/// Refuses a `TagSpecification` for a resource type the call cannot tag, instead of dropping it.
pub fn only_types(p: &Params, allowed: &[&str]) -> Result<(), Ec2Error> {
    for n in 1..=10 {
        let Some(rt) = p.get(&format!("TagSpecification.{n}.ResourceType")) else { break };
        let has_tags = p.keys().any(|k| k.starts_with(&format!("TagSpecification.{n}.Tag.")));
        if has_tags && !allowed.contains(&rt.as_str()) {
            return Err(Ec2Error::bad("UnsupportedOperation", format!("TagSpecification for {rt} is not supported here: tag it after creating it")));
        }
    }
    Ok(())
}

pub async fn apply(state: &AppState, kind: Kind, id: Uuid, tags: &BTreeMap<String, String>) -> Result<(), Ec2Error> {
    if tags.is_empty() {
        return Ok(());
    }
    let mut tx = state.pool.begin().await?;
    match crate::api::tags::put_tag_map(&mut tx, kind, id, tags).await? {
        crate::api::tags::PutOutcome::Ok(_) => {}
        crate::api::tags::PutOutcome::TooMany(n) => {
            return Err(Ec2Error::bad("TagLimitExceeded", format!("a resource may have at most {} tags, this would be {n}", crate::api::tags::MAX_TAGS)))
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn tags_of(state: &AppState, kind: Kind, id: Uuid) -> Result<BTreeMap<String, String>, Ec2Error> {
    let mut conn = state.pool.acquire().await?;
    Ok(crate::api::tags::get_tag_map(&mut conn, kind, id).await?)
}

pub fn tag_set_xml(tags: &BTreeMap<String, String>) -> String {
    let items: String = tags.iter().map(|(k, v)| format!("<item><key>{}</key><value>{}</value></item>", xml_escape(k), xml_escape(v))).collect();
    format!("<tagSet>{items}</tagSet>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn tags_are_picked_by_resource_type() {
        let q = p(&[
            ("TagSpecification.1.ResourceType", "instance"),
            ("TagSpecification.1.Tag.1.Key", "a"),
            ("TagSpecification.1.Tag.1.Value", "1"),
            ("TagSpecification.2.ResourceType", "placement-group"),
            ("TagSpecification.2.Tag.1.Key", "b"),
            ("TagSpecification.2.Tag.1.Value", "2"),
            ("TagSpecification.2.Tag.2.Key", "c"),
        ]);
        assert_eq!(tags_for(&q, "placement-group").into_iter().collect::<Vec<_>>(), [("b".to_string(), "2".to_string()), ("c".to_string(), String::new())]);
        assert!(tags_for(&q, "volume").is_empty());
    }

    #[test]
    fn other_types_with_tags_are_refused() {
        let q = p(&[("TagSpecification.1.ResourceType", "snapshot"), ("TagSpecification.1.Tag.1.Key", "a")]);
        assert!(only_types(&q, &["placement-group"]).is_err());
        assert!(only_types(&q, &["snapshot"]).is_ok());
        // a type with no tags is harmless
        let empty = p(&[("TagSpecification.1.ResourceType", "snapshot")]);
        assert!(only_types(&empty, &["placement-group"]).is_ok());
    }

    #[test]
    fn tag_set_escapes() {
        let t: BTreeMap<String, String> = [("k<".to_string(), "v&".to_string())].into_iter().collect();
        assert_eq!(tag_set_xml(&t), "<tagSet><item><key>k&lt;</key><value>v&amp;</value></item></tagSet>");
    }
}
