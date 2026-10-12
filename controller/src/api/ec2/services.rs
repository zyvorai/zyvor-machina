// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The AWS query-protocol services behind the one signed endpoint.
//!
//! A request is signed for exactly one service (`Credential=…/<date>/<region>/<service>/aws4_request`);
//! that scope, not the URL path, picks the action table, the XML namespace, the API version and the
//! error envelope. `POST /ec2`, `/monitoring`, `/autoscaling` and `/elbv2` all reach the same handler,
//! so a client may point every service at one `endpoint_url` or at the matching alias.
//!
//! | scope                  | actions                                                      |
//! |------------------------|--------------------------------------------------------------|
//! | `ec2`                  | everything in `mod.rs::dispatch` (CloudWatch-style actions too, for older clients) |
//! | `monitoring`           | the CloudWatch-style alarm and statistics actions (`monitoring.rs`) |
//! | `autoscaling`          | `autoscaling_dispatch` below (`autoscaling.rs`) |
//! | `elasticloadbalancing` | `elb_dispatch` below (empty until the ELBv2 package lands) |
//!
//! Adding actions to a service: add match arms to that service's `*_dispatch` function. A handler for
//! `autoscaling` or `elasticloadbalancing` returns the **final** body of the `<Action>Result` element
//! (PascalCase tags, lists as `<member>`), an empty string for an action with no result element.

use std::collections::BTreeMap;

use axum::http::StatusCode;

use super::{monitoring, xml_escape, Ec2Error};
use crate::auth::AuthUser;
use crate::state::AppState;

type Params = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Ec2,
    Monitoring,
    AutoScaling,
    ElasticLoadBalancing,
}

impl Service {
    /// The service name inside a SigV4 credential scope. Anything else is refused.
    pub fn from_scope(scope: &str) -> Option<Service> {
        match scope {
            "ec2" => Some(Service::Ec2),
            "monitoring" => Some(Service::Monitoring),
            "autoscaling" => Some(Service::AutoScaling),
            "elasticloadbalancing" => Some(Service::ElasticLoadBalancing),
            _ => None,
        }
    }

    pub fn xmlns(self) -> &'static str {
        match self {
            Service::Ec2 => "http://ec2.amazonaws.com/doc/2016-11-15/",
            Service::Monitoring => "http://monitoring.amazonaws.com/doc/2010-08-01/",
            Service::AutoScaling => "http://autoscaling.amazonaws.com/doc/2011-01-01/",
            Service::ElasticLoadBalancing => "http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/",
        }
    }

    /// The `Version` parameter the service's SDKs send.
    pub fn version(self) -> &'static str {
        match self {
            Service::Ec2 => "2016-11-15",
            Service::Monitoring => "2010-08-01",
            Service::AutoScaling => "2011-01-01",
            Service::ElasticLoadBalancing => "2015-12-01",
        }
    }

    /// `<Error><Type>Sender</Type>…` envelope of the query services; ec2 keeps its own `<Response><Errors>` shape.
    pub fn error_xml(self, e: &Ec2Error, request_id: &str) -> String {
        if self == Service::Ec2 {
            return super::error_xml(e, request_id);
        }
        let kind = if e.status.is_server_error() { "Receiver" } else { "Sender" };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ErrorResponse xmlns=\"{}\"><Error><Type>{kind}</Type><Code>{}</Code><Message>{}</Message></Error><RequestId>{request_id}</RequestId></ErrorResponse>",
            self.xmlns(),
            e.code,
            xml_escape(&e.message)
        )
    }

    /// Wraps a handler's body. `inner` is what the handler returned: for `ec2` the whole body, for
    /// `monitoring` ec2-shaped markup that is converted to CloudWatch's shape, for the other
    /// services the final `<Action>Result` content.
    pub fn response_xml(self, action: &str, request_id: &str, inner: &str) -> String {
        if self == Service::Ec2 {
            return super::xml_response(action, request_id, inner);
        }
        let body = if self == Service::Monitoring { cloudwatch_body(inner) } else { inner.to_string() };
        let result = if body.is_empty() { String::new() } else { format!("<{action}Result>{body}</{action}Result>") };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><{action}Response xmlns=\"{}\">{result}<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata></{action}Response>",
            self.xmlns()
        )
    }
}

/// A `Version` the service does not speak is refused (a classic-ELB `2012-06-01` client must not get ELBv2 answers).
/// `ec2` is lenient, as before.
pub fn check_version(service: Service, p: &Params) -> Result<(), Ec2Error> {
    match p.get("Version") {
        Some(v) if service != Service::Ec2 && v != service.version() => Err(Ec2Error::bad(
            "InvalidParameterValue",
            format!("API version {v} is not supported for this service; use {}", service.version()),
        )),
        _ => Ok(()),
    }
}

fn invalid_action(action: &str) -> Ec2Error {
    Ec2Error::new(StatusCode::BAD_REQUEST, "InvalidAction", format!("The action {action} is not valid for this web service."))
}

/// Action tables of the services other than `ec2`.
pub async fn dispatch_other(service: Service, state: &AppState, actor: &AuthUser, p: &Params, action: &str) -> Result<String, Ec2Error> {
    match service {
        Service::Monitoring => monitoring_dispatch(state, actor, p, action).await,
        Service::AutoScaling => autoscaling_dispatch(state, actor, p, action).await,
        Service::ElasticLoadBalancing => elb_dispatch(state, actor, p, action).await,
        Service::Ec2 => Err(invalid_action(action)),
    }
}

async fn monitoring_dispatch(state: &AppState, actor: &AuthUser, p: &Params, action: &str) -> Result<String, Ec2Error> {
    Ok(match action {
        "DescribeAlarms" => monitoring::describe_alarms(state, actor, p).await?,
        "PutMetricAlarm" => monitoring::put_metric_alarm(state, actor, p).await?,
        "DeleteAlarms" => monitoring::delete_alarms(state, actor, p).await?,
        "EnableAlarmActions" => monitoring::set_alarm_actions(state, actor, p, true).await?,
        "DisableAlarmActions" => monitoring::set_alarm_actions(state, actor, p, false).await?,
        "GetMetricStatistics" => monitoring::get_metric_statistics(state, actor, p).await?,
        _ => return Err(invalid_action(action)),
    })
}

/// Auto Scaling actions (`autoscaling.rs`).
async fn autoscaling_dispatch(state: &AppState, actor: &AuthUser, p: &Params, action: &str) -> Result<String, Ec2Error> {
    super::autoscaling::dispatch(state, actor, p, action).await
}

/// ELBv2 actions (`elbv2.rs`).
async fn elb_dispatch(state: &AppState, actor: &AuthUser, p: &Params, action: &str) -> Result<String, Ec2Error> {
    match super::elbv2::dispatch(state, actor, p, action).await {
        Some(r) => r,
        None => Err(invalid_action(action)),
    }
}

// ---- CloudWatch shape ------------------------------------------------------

/// Converts the ec2-shaped markup `monitoring.rs` produces (camelCase tags, `<item>` lists, epoch
/// timestamps, `<return>true</return>`) into CloudWatch's (PascalCase tags, `<member>` lists, ISO 8601).
/// Text content is already XML-escaped, so a `<` always starts a tag.
fn cloudwatch_body(inner: &str) -> String {
    if inner == "<return>true</return>" {
        return String::new();
    }
    let mut out = String::with_capacity(inner.len() + 16);
    let mut rest = inner;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tag_start = i + 1;
        let closing = rest[tag_start..].starts_with('/');
        let name_start = tag_start + usize::from(closing);
        let name_len = rest[name_start..].find(|c: char| c == '>' || c == '/' || c.is_whitespace()).unwrap_or(rest.len() - name_start);
        let name = &rest[name_start..name_start + name_len];
        out.push('<');
        if closing {
            out.push('/');
        }
        match name {
            "item" => out.push_str("member"),
            _ => {
                let mut chars = name.chars();
                if let Some(f) = chars.next() {
                    out.extend(f.to_uppercase());
                    out.push_str(chars.as_str());
                }
            }
        }
        rest = &rest[name_start + name_len..];
    }
    out.push_str(rest);
    iso_timestamps(&out)
}

/// `<Timestamp>1699999999</Timestamp>` becomes `<Timestamp>2023-11-14T22:13:19Z</Timestamp>`.
fn iso_timestamps(s: &str) -> String {
    const OPEN: &str = "<Timestamp>";
    const CLOSE: &str = "</Timestamp>";
    let mut out = String::with_capacity(s.len() + 16);
    let mut rest = s;
    while let Some(i) = rest.find(OPEN) {
        let body = i + OPEN.len();
        let Some(j) = rest[body..].find(CLOSE) else { break };
        out.push_str(&rest[..body]);
        let raw = &rest[body..body + j];
        match raw.parse::<i64>().ok().and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
            Some(dt) => out.push_str(&dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            None => out.push_str(raw),
        }
        rest = &rest[body + j..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_map_to_services_and_unknown_scopes_are_refused() {
        assert_eq!(Service::from_scope("ec2"), Some(Service::Ec2));
        assert_eq!(Service::from_scope("monitoring"), Some(Service::Monitoring));
        assert_eq!(Service::from_scope("autoscaling"), Some(Service::AutoScaling));
        assert_eq!(Service::from_scope("elasticloadbalancing"), Some(Service::ElasticLoadBalancing));
        for other in ["iam", "s3", "sts", "EC2", "elbv2", "", "ec2 "] {
            assert_eq!(Service::from_scope(other), None, "{other:?}");
        }
    }

    #[test]
    fn each_service_has_its_namespace_and_version() {
        assert_eq!(Service::Ec2.xmlns(), "http://ec2.amazonaws.com/doc/2016-11-15/");
        assert_eq!(Service::Monitoring.xmlns(), "http://monitoring.amazonaws.com/doc/2010-08-01/");
        assert_eq!(Service::AutoScaling.xmlns(), "http://autoscaling.amazonaws.com/doc/2011-01-01/");
        assert_eq!(Service::ElasticLoadBalancing.xmlns(), "http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/");
        for s in [Service::Ec2, Service::Monitoring, Service::AutoScaling, Service::ElasticLoadBalancing] {
            assert!(s.xmlns().ends_with(&format!("/{}/", s.version())), "{s:?}");
        }
    }

    #[test]
    fn version_is_checked_for_query_services_only() {
        let with = |v: &str| BTreeMap::from([("Version".to_string(), v.to_string())]);
        assert!(check_version(Service::ElasticLoadBalancing, &with("2015-12-01")).is_ok());
        assert!(check_version(Service::ElasticLoadBalancing, &with("2012-06-01")).is_err(), "classic ELB is not ELBv2");
        assert!(check_version(Service::AutoScaling, &with("2011-01-01")).is_ok());
        assert!(check_version(Service::Monitoring, &with("2009-05-15")).is_err());
        assert!(check_version(Service::Monitoring, &BTreeMap::new()).is_ok(), "a missing Version is tolerated");
        assert!(check_version(Service::Ec2, &with("2013-10-15")).is_ok(), "ec2 stays lenient");
    }

    #[test]
    fn error_envelopes_follow_the_service() {
        let e = Ec2Error::bad("InvalidAction", "no <such> action");
        let ec2 = Service::Ec2.error_xml(&e, "rid");
        assert!(ec2.contains("<Response><Errors><Error><Code>InvalidAction</Code>") && ec2.contains("<RequestID>rid</RequestID>"));
        for s in [Service::Monitoring, Service::AutoScaling, Service::ElasticLoadBalancing] {
            let x = s.error_xml(&e, "rid");
            assert!(x.contains(&format!("<ErrorResponse xmlns=\"{}\">", s.xmlns())), "{x}");
            assert!(x.contains("<Error><Type>Sender</Type><Code>InvalidAction</Code><Message>no &lt;such&gt; action</Message></Error>"), "{x}");
            assert!(x.ends_with("<RequestId>rid</RequestId></ErrorResponse>"), "{x}");
        }
        let server = Ec2Error::new(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", "boom");
        assert!(Service::AutoScaling.error_xml(&server, "r").contains("<Type>Receiver</Type>"));
    }

    #[test]
    fn query_services_wrap_a_result_and_response_metadata() {
        let x = Service::AutoScaling.response_xml("DescribeAutoScalingGroups", "rid", "<AutoScalingGroups/>");
        assert_eq!(
            x,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><DescribeAutoScalingGroupsResponse xmlns=\"http://autoscaling.amazonaws.com/doc/2011-01-01/\"><DescribeAutoScalingGroupsResult><AutoScalingGroups/></DescribeAutoScalingGroupsResult><ResponseMetadata><RequestId>rid</RequestId></ResponseMetadata></DescribeAutoScalingGroupsResponse>"
        );
        let none = Service::ElasticLoadBalancing.response_xml("DeleteLoadBalancer", "rid", "");
        assert!(!none.contains("Result>") && none.contains("<ResponseMetadata><RequestId>rid</RequestId></ResponseMetadata>"), "{none}");
        let ec2 = Service::Ec2.response_xml("DescribeVpcs", "rid", "<vpcSet/>");
        assert!(ec2.contains("<requestId>rid</requestId><vpcSet/>"), "{ec2}");
    }

    #[test]
    fn cloudwatch_shape_for_alarms_datapoints_and_acks() {
        let alarms = "<metricAlarms><item><alarmName>a&amp;b</alarmName><stateValue>OK</stateValue></item></metricAlarms>";
        assert_eq!(
            cloudwatch_body(alarms),
            "<MetricAlarms><member><AlarmName>a&amp;b</AlarmName><StateValue>OK</StateValue></member></MetricAlarms>"
        );
        let points = "<label>cpu</label><datapoints><item><timestamp>1699999999</timestamp><sampleCount>3</sampleCount></item></datapoints>";
        assert_eq!(
            cloudwatch_body(points),
            "<Label>cpu</Label><Datapoints><member><Timestamp>2023-11-14T22:13:19Z</Timestamp><SampleCount>3</SampleCount></member></Datapoints>"
        );
        assert_eq!(cloudwatch_body("<return>true</return>"), "", "PutMetricAlarm and friends have no result element");
        let x = Service::Monitoring.response_xml("PutMetricAlarm", "rid", "<return>true</return>");
        assert!(!x.contains("Result>"), "{x}");
        assert_eq!(cloudwatch_body("<metricAlarms></metricAlarms>"), "<MetricAlarms></MetricAlarms>");
    }

    #[tokio::test]
    async fn other_services_answer_invalid_action_until_their_actions_exist() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let actor = AuthUser { username: "u".into(), role: "admin".into(), auth_source: None };
        for (svc, action) in [
            (Service::AutoScaling, "NoSuchAutoScalingAction"),
            (Service::ElasticLoadBalancing, "ConfigureHealthCheck"),
            (Service::Monitoring, "RunInstances"),
            (Service::Ec2, "DescribeInstances"),
        ] {
            let e = dispatch_other(svc, &state, &actor, &BTreeMap::new(), action).await.unwrap_err();
            assert_eq!((e.status, e.code), (StatusCode::BAD_REQUEST, "InvalidAction"), "{svc:?} {action}");
        }
    }

    // ---- signed requests through the real endpoint ----

    use axum::body::{to_bytes, Bytes};
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue, Uri};

    const KEY: &str = "AKIDE4TESTSERVICES";
    const SECRET: &str = "secretsecretsecretsecretsecretsecretsecret00";

    async fn seeded() -> AppState {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let stored = crate::engine::ai::crypto::store_api_key(SECRET).unwrap();
        crate::db::query("INSERT INTO ec2_access_keys (access_key_id, secret_enc, username, role, description) VALUES (?, ?, 'tester', 'admin', 'test')")
            .bind(KEY)
            .bind(stored)
            .execute(&state.pool)
            .await
            .unwrap();
        state
    }

    /// One signed form POST for `service` (the scope in the signature) at `path`.
    async fn call(state: &AppState, path: &str, service: &str, form: &str) -> (StatusCode, String) {
        let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let date = &amz_date[..8];
        let hdrs = vec![
            ("content-type".to_string(), "application/x-www-form-urlencoded; charset=utf-8".to_string()),
            ("host".to_string(), "controller.test".to_string()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        let probe = format!(
            "AWS4-HMAC-SHA256 Credential={KEY}/{date}/machina/{service}/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=00"
        );
        let parsed = super::super::sigv4::parse_authorization(&probe).unwrap();
        let sig = super::super::sigv4::expected_signature(&parsed, SECRET, "POST", path, "", &hdrs, form.as_bytes(), &amz_date).unwrap();
        let mut headers = HeaderMap::new();
        for (k, v) in &hdrs {
            headers.insert(axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(), HeaderValue::from_str(v).unwrap());
        }
        headers.insert("authorization", HeaderValue::from_str(&probe.replace("Signature=00", &format!("Signature={sig}"))).unwrap());
        let resp = super::super::query(State(state.clone()), headers, path.parse::<Uri>().unwrap(), Bytes::from(form.to_string())).await;
        let status = resp.status();
        (status, String::from_utf8(to_bytes(resp.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_signed_ec2_request_still_works() {
        let state = seeded().await;
        let (st, body) = call(&state, "/ec2", "ec2", "Action=DescribeKeyPairs&Version=2016-11-15").await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("<DescribeKeyPairsResponse xmlns=\"http://ec2.amazonaws.com/doc/2016-11-15/\">"), "{body}");
    }

    #[tokio::test]
    async fn a_signed_monitoring_request_answers_in_cloudwatch_shape_on_every_alias() {
        let state = seeded().await;
        for path in ["/monitoring", "/ec2", "/elbv2"] {
            let (st, body) = call(&state, path, "monitoring", "Action=DescribeAlarms&Version=2010-08-01").await;
            assert_eq!(st, StatusCode::OK, "{path}: {body}");
            assert!(body.contains("<DescribeAlarmsResponse xmlns=\"http://monitoring.amazonaws.com/doc/2010-08-01/\">"), "{body}");
            assert!(body.contains("<DescribeAlarmsResult><MetricAlarms></MetricAlarms></DescribeAlarmsResult><ResponseMetadata><RequestId>"), "{body}");
        }
    }

    #[tokio::test]
    async fn signed_autoscaling_and_elb_requests_reach_their_tables() {
        let state = seeded().await;
        let (st, body) = call(&state, "/autoscaling", "autoscaling", "Action=NoSuchAutoScalingAction&Version=2011-01-01").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("<ErrorResponse xmlns=\"http://autoscaling.amazonaws.com/doc/2011-01-01/\"><Error><Type>Sender</Type><Code>InvalidAction</Code>"), "{body}");
        // a classic-ELB action is not ELBv2
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", "Action=ConfigureHealthCheck&Version=2015-12-01").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("<ErrorResponse xmlns=\"http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/\"><Error><Type>Sender</Type><Code>InvalidAction</Code>"), "{body}");
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", "Action=DescribeLoadBalancers&Version=2012-06-01").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("<Code>InvalidParameterValue</Code>"), "{body}");
    }

    #[tokio::test]
    async fn a_signed_elbv2_flow_answers_in_elbv2_shape() {
        let state = seeded().await;
        crate::engine::test_support::seed_host(&state.pool, uuid::Uuid::new_v4()).await;
        let v = "Version=2015-12-01";
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", &format!("Action=CreateLoadBalancer&{v}&Name=sig-lb&Type=network")).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("<CreateLoadBalancerResponse xmlns=\"http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/\"><CreateLoadBalancerResult><LoadBalancers><member>"), "{body}");
        assert!(body.contains("<LoadBalancerName>sig-lb</LoadBalancerName>") && body.contains("<State><Code>active</Code></State>"), "{body}");
        // the same name again, and a name nobody has, are errors in ELBv2's envelope
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", &format!("Action=CreateLoadBalancer&{v}&Name=sig-lb&Type=network")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(body.contains("<ErrorResponse xmlns=\"http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/\"><Error><Type>Sender</Type><Code>DuplicateLoadBalancerName</Code>"), "{body}");
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", &format!("Action=DescribeLoadBalancers&{v}&Names.member.1=nope")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(body.contains("<Code>LoadBalancerNotFound</Code>"), "{body}");
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", &format!("Action=DescribeLoadBalancers&{v}")).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("<DescribeLoadBalancersResult><LoadBalancers><member>") && body.contains("<ResponseMetadata><RequestId>"), "{body}");
        // an action with no output members answers without a Result element
        let (st, body) = call(&state, "/elbv2", "elasticloadbalancing", &format!("Action=DeleteLoadBalancer&{v}&LoadBalancerArn=arn:aws:elasticloadbalancing:machina:000000000000:loadbalancer/net/gone/0123456789abcdef")).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("<DeleteLoadBalancerResponse xmlns=") && !body.contains("<DeleteLoadBalancerResult>"), "{body}");
    }

    #[tokio::test]
    async fn signed_autoscaling_requests_create_and_describe_launch_configurations() {
        let state = seeded().await;
        let form = "Action=CreateLaunchConfiguration&Version=2011-01-01&LaunchConfigurationName=lc-signed&ImageId=ubuntu-24.04";
        let (st, body) = call(&state, "/autoscaling", "autoscaling", form).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(
            body.contains("<CreateLaunchConfigurationResponse xmlns=\"http://autoscaling.amazonaws.com/doc/2011-01-01/\"><ResponseMetadata><RequestId>"),
            "an action with no result has no result element: {body}"
        );
        let (st, body) = call(&state, "/ec2", "autoscaling", "Action=DescribeLaunchConfigurations&Version=2011-01-01").await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("<DescribeLaunchConfigurationsResult><LaunchConfigurations><member><LaunchConfigurationName>lc-signed</LaunchConfigurationName>"), "{body}");
        let (st, body) = call(&state, "/autoscaling", "autoscaling", "Action=CreateLaunchConfiguration&Version=2011-01-01&LaunchConfigurationName=x&ImageId=y&SpotPrice=1").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("<Code>UnsupportedOperation</Code>") && body.contains("SpotPrice"), "{body}");
        // the same action is not an EC2 action
        let (st, body) = call(&state, "/ec2", "ec2", "Action=DescribeAutoScalingGroups&Version=2016-11-15").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    }

    #[tokio::test]
    async fn other_scopes_and_bad_signatures_are_refused_in_the_right_envelope() {
        let state = seeded().await;
        let (st, body) = call(&state, "/ec2", "iam", "Action=ListUsers&Version=2010-05-08").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{body}");
        assert!(body.contains("<Code>AuthFailure</Code>") && body.contains("<Response><Errors>"), "{body}");
        // a wrong signature under the autoscaling scope is refused in autoscaling's envelope
        let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={KEY}/{}/machina/autoscaling/aws4_request, SignedHeaders=host;x-amz-date, Signature={}",
            &amz_date[..8],
            "0".repeat(64)
        );
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_str(&auth).unwrap());
        headers.insert("host", HeaderValue::from_static("controller.test"));
        headers.insert("x-amz-date", HeaderValue::from_str(&amz_date).unwrap());
        let resp = super::super::query(State(state), headers, "/autoscaling".parse().unwrap(), Bytes::from_static(b"Action=X")).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = String::from_utf8(to_bytes(resp.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap();
        assert!(body.contains("<ErrorResponse xmlns=\"http://autoscaling.amazonaws.com/doc/2011-01-01/\"><Error><Type>Sender</Type><Code>AuthFailure</Code>"), "{body}");
    }
}
