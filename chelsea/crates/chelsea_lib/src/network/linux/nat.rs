use std::{borrow::Cow, net::Ipv4Addr};

use anyhow::{Context, anyhow, bail};
use ipnet::Ipv4Net;
use nftables::expr::{Expression, Meta, MetaKey, NamedExpression, Payload, PayloadField, Prefix};
use nftables::stmt::NATFamily;
use nftables::types::{NfChainPolicy, NfChainType, NfFamily, NfHook};
use nftables::{
    batch::Batch,
    helper,
    schema::{Chain, NfCmd, NfListObject, NfObject, Nftables, Rule, Table},
    stmt::{Match, NAT, Operator, Statement},
};

const NAT_TABLE_NAME: &str = "chelsea_nat";
const POSTROUTING_CHAIN_NAME: &str = "chelsea_postrouting";
const PREROUTING_CHAIN_NAME: &str = "chelsea_prerouting";
const POSTROUTING_PRIORITY: i32 = 100; // nftables "srcnat" priority
const PREROUTING_PRIORITY: i32 = -100; // nftables "dstnat" priority
const SSH_TARGET_PORT: u16 = 22;

/// Adds an outgoing NAT rule MASQUERADING packets from {source} through {output_interface}
pub async fn add_outbound_masquerade_nat_rule(
    source_net: &Ipv4Net,
    output_interface: impl AsRef<str>,
) -> anyhow::Result<()> {
    ensure_nat_environment().await?;

    if check_outbound_masquerade_nat_rule_exists(source_net, output_interface.as_ref()).await? {
        return Ok(());
    }

    let rule = outbound_masquerade_rule(source_net, output_interface.as_ref());
    let mut batch = Batch::new();
    batch.add(NfListObject::Rule(rule));
    let payload = batch.to_nftables();
    helper::apply_ruleset_async(&payload)
        .await
        .context("adding outbound masquerade rule via nftables")?;

    Ok(())
}

/// Deletes an outgoing NAT rule MASQUERADING packets from {source} through {output_interface}
pub async fn delete_outbound_masquerade_nat_rule(
    source_net: &Ipv4Net,
    output_interface: impl AsRef<str>,
) -> anyhow::Result<()> {
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing current nftables ruleset")?;
    let handle = find_outbound_rule_handle(&ruleset, source_net, output_interface.as_ref())?
        .ok_or_else(|| anyhow!("outbound masquerade rule not found"))?;
    delete_rule_by_handle(POSTROUTING_CHAIN_NAME, handle)
        .await
        .context("deleting outbound masquerade rule")?;
    Ok(())
}

/// Checks if an outgoing NAT rule MASQUERADING packets from {source} through {output_interface} exists
pub async fn check_outbound_masquerade_nat_rule_exists(
    source_net: &Ipv4Net,
    output_interface: impl AsRef<str>,
) -> anyhow::Result<bool> {
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing current nftables ruleset")?;
    Ok(find_outbound_rule_handle(&ruleset, source_net, output_interface.as_ref())?.is_some())
}

/// Sets up an inbound DNAT rule routing {port} to {target_addr}:22
pub async fn add_inbound_ssh_nat_rule(port: u16, target_addr: &Ipv4Addr) -> anyhow::Result<()> {
    ensure_nat_environment().await?;

    if check_inbound_ssh_nat_rule_exists(port, target_addr).await? {
        return Ok(());
    }

    let rule = inbound_dnat_rule(port, target_addr);
    let mut batch = Batch::new();
    batch.add(NfListObject::Rule(rule));
    let payload = batch.to_nftables();
    helper::apply_ruleset_async(&payload)
        .await
        .context("adding inbound DNAT rule via nftables")?;

    Ok(())
}

/// Deletes an inbound DNAT rule routing {port} to {target_addr}:22
pub async fn delete_inbound_ssh_nat_rule(port: u16, target_addr: &Ipv4Addr) -> anyhow::Result<()> {
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing current nftables ruleset")?;
    let handle = find_inbound_rule_handle(&ruleset, port, target_addr)?
        .ok_or_else(|| anyhow!("inbound DNAT rule not found"))?;
    delete_rule_by_handle(PREROUTING_CHAIN_NAME, handle)
        .await
        .context("deleting inbound DNAT rule")?;
    Ok(())
}

/// Check if an inbound DNAT rule routing {port} to {target_addr}:22 exists
pub async fn check_inbound_ssh_nat_rule_exists(
    port: u16,
    target_addr: &Ipv4Addr,
) -> anyhow::Result<bool> {
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing current nftables ruleset")?;
    Ok(find_inbound_rule_handle(&ruleset, port, target_addr)?.is_some())
}

async fn ensure_nat_environment() -> anyhow::Result<()> {
    // nftables 'add' command is idempotent for tables and chains - it will succeed
    // even if they already exist. This is more robust than checking first, as it
    // eliminates TOCTOU races and handles the case where rules exist but the
    // database was cleared.
    let mut batch = Batch::new();

    batch.add(NfListObject::Table(Table {
        family: NfFamily::IP,
        name: Cow::Borrowed(NAT_TABLE_NAME),
        handle: None,
    }));

    batch.add(NfListObject::Chain(Chain {
        family: NfFamily::IP,
        table: Cow::Borrowed(NAT_TABLE_NAME),
        name: Cow::Borrowed(POSTROUTING_CHAIN_NAME),
        newname: None,
        handle: None,
        _type: Some(NfChainType::NAT),
        hook: Some(NfHook::Postrouting),
        prio: Some(POSTROUTING_PRIORITY),
        dev: None,
        policy: Some(NfChainPolicy::Accept),
    }));

    batch.add(NfListObject::Chain(Chain {
        family: NfFamily::IP,
        table: Cow::Borrowed(NAT_TABLE_NAME),
        name: Cow::Borrowed(PREROUTING_CHAIN_NAME),
        newname: None,
        handle: None,
        _type: Some(NfChainType::NAT),
        hook: Some(NfHook::Prerouting),
        prio: Some(PREROUTING_PRIORITY),
        dev: None,
        policy: Some(NfChainPolicy::Accept),
    }));

    let payload = batch.to_nftables();
    helper::apply_ruleset_async(&payload)
        .await
        .context("creating nftables nat table/chains")?;

    Ok(())
}

async fn delete_rule_by_handle(chain_name: &str, handle: u32) -> anyhow::Result<()> {
    let mut batch = Batch::new();
    batch.add_cmd(NfCmd::Delete(NfListObject::Rule(Rule {
        family: NfFamily::IP,
        table: Cow::Borrowed(NAT_TABLE_NAME),
        chain: Cow::Borrowed(chain_name),
        expr: Cow::Borrowed(&[]),
        handle: Some(handle),
        index: None,
        comment: None,
    })));

    helper::apply_ruleset_async(&batch.to_nftables())
        .await
        .context("deleting nftables rule by handle")?;
    Ok(())
}

fn outbound_masquerade_rule(source_net: &Ipv4Net, output_interface: &str) -> Rule<'static> {
    Rule {
        family: NfFamily::IP,
        table: Cow::Borrowed(NAT_TABLE_NAME),
        chain: Cow::Borrowed(POSTROUTING_CHAIN_NAME),
        expr: Cow::Owned(vec![
            Statement::Match(Match {
                left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                    PayloadField {
                        protocol: Cow::Borrowed("ip"),
                        field: Cow::Borrowed("saddr"),
                    },
                ))),
                right: cidr_expression(source_net),
                op: Operator::EQ,
            }),
            Statement::Match(Match {
                left: Expression::Named(NamedExpression::Meta(Meta {
                    key: MetaKey::Oifname,
                })),
                right: Expression::String(Cow::Owned(output_interface.to_string())),
                op: Operator::EQ,
            }),
            Statement::Masquerade(None),
        ]),
        handle: None,
        index: None,
        comment: None,
    }
}

fn inbound_dnat_rule(port: u16, target_addr: &Ipv4Addr) -> Rule<'static> {
    Rule {
        family: NfFamily::IP,
        table: Cow::Borrowed(NAT_TABLE_NAME),
        chain: Cow::Borrowed(PREROUTING_CHAIN_NAME),
        expr: Cow::Owned(vec![
            Statement::Match(Match {
                left: Expression::Named(NamedExpression::Meta(Meta {
                    key: MetaKey::L4proto,
                })),
                right: Expression::String(Cow::Borrowed("tcp")),
                op: Operator::EQ,
            }),
            Statement::Match(Match {
                left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                    PayloadField {
                        protocol: Cow::Borrowed("tcp"),
                        field: Cow::Borrowed("dport"),
                    },
                ))),
                right: Expression::Number(port.into()),
                op: Operator::EQ,
            }),
            Statement::DNAT(Some(NAT {
                addr: Some(Expression::String(Cow::Owned(target_addr.to_string()))),
                family: Some(NATFamily::IP),
                port: Some(Expression::Number(SSH_TARGET_PORT.into())),
                flags: None,
            })),
        ]),
        handle: None,
        index: None,
        comment: None,
    }
}

fn cidr_expression(net: &Ipv4Net) -> Expression<'static> {
    Expression::Named(NamedExpression::Prefix(Prefix {
        addr: Box::new(Expression::String(Cow::Owned(net.network().to_string()))),
        len: net.prefix_len().into(),
    }))
}

fn find_outbound_rule_handle(
    ruleset: &Nftables<'_>,
    source_net: &Ipv4Net,
    output_interface: &str,
) -> anyhow::Result<Option<u32>> {
    find_rule_handle(ruleset, POSTROUTING_CHAIN_NAME, |rule| {
        outbound_rule_matches(rule, source_net, output_interface)
    })
}

fn find_inbound_rule_handle(
    ruleset: &Nftables<'_>,
    port: u16,
    target_addr: &Ipv4Addr,
) -> anyhow::Result<Option<u32>> {
    find_rule_handle(ruleset, PREROUTING_CHAIN_NAME, |rule| {
        inbound_rule_matches(rule, port, target_addr)
    })
}

fn find_rule_handle<F>(
    ruleset: &Nftables<'_>,
    chain_name: &str,
    predicate: F,
) -> anyhow::Result<Option<u32>>
where
    F: Fn(&Rule<'_>) -> anyhow::Result<bool>,
{
    for obj in ruleset.objects.iter() {
        if let NfObject::ListObject(NfListObject::Rule(rule)) = obj {
            if rule.family != NfFamily::IP
                || rule.table != NAT_TABLE_NAME
                || rule.chain != chain_name
            {
                continue;
            }

            if predicate(rule)? {
                let handle = rule
                    .handle
                    .ok_or_else(|| anyhow!("rule matched without a handle"))?;
                return Ok(Some(handle));
            }
        }
    }

    Ok(None)
}

fn outbound_rule_matches(
    rule: &Rule<'_>,
    source_net: &Ipv4Net,
    output_interface: &str,
) -> anyhow::Result<bool> {
    let mut saw_source = false;
    let mut saw_interface = false;
    let mut saw_masquerade = false;

    for stmt in rule.expr.iter() {
        match stmt {
            Statement::Match(m) if m.op == Operator::EQ => {
                if is_ip_saddr_match(&m.left, &m.right, source_net)? {
                    saw_source = true;
                } else if is_oifname_match(&m.left, &m.right, output_interface) {
                    saw_interface = true;
                }
            }
            Statement::Masquerade(_) => saw_masquerade = true,
            _ => {}
        }
    }

    Ok(saw_source && saw_interface && saw_masquerade)
}

fn inbound_rule_matches(
    rule: &Rule<'_>,
    port: u16,
    target_addr: &Ipv4Addr,
) -> anyhow::Result<bool> {
    let mut saw_port = false;
    let mut saw_dnat = false;

    for stmt in rule.expr.iter() {
        match stmt {
            Statement::Match(m) if m.op == Operator::EQ => {
                if is_l4proto_tcp(&m.left, &m.right) {
                    continue;
                }

                if is_tcp_dport_match(&m.left, &m.right, port) {
                    saw_port = true;
                }
            }
            Statement::DNAT(Some(nat)) => {
                if dnat_matches(nat, target_addr) {
                    saw_dnat = true;
                }
            }
            _ => {}
        }
    }

    Ok(saw_port && saw_dnat)
}

fn is_ip_saddr_match(
    left: &Expression<'_>,
    right: &Expression<'_>,
    source_net: &Ipv4Net,
) -> anyhow::Result<bool> {
    let expected_addr = source_net.network().to_string();
    let expected_len: u32 = source_net.prefix_len().into();

    if !matches!(
        left,
        Expression::Named(NamedExpression::Payload(Payload::PayloadField(field)))
            if field.protocol == "ip" && field.field == "saddr"
    ) {
        return Ok(false);
    }

    match right {
        Expression::Named(NamedExpression::Prefix(prefix)) => {
            let addr = match prefix.addr.as_ref() {
                Expression::String(s) => s.as_ref(),
                _ => bail!("unexpected ip prefix address representation"),
            };
            let len = prefix.len;
            Ok(addr == expected_addr && len == expected_len)
        }
        _ => Ok(false),
    }
}

fn is_oifname_match(left: &Expression<'_>, right: &Expression<'_>, iface: &str) -> bool {
    matches!(
        (left, right),
        (
            Expression::Named(NamedExpression::Meta(Meta {
                key: MetaKey::Oifname
            })),
            Expression::String(value)
        ) if value.as_ref() == iface
    )
}

fn is_l4proto_tcp(left: &Expression<'_>, right: &Expression<'_>) -> bool {
    matches!(
        (left, right),
        (
            Expression::Named(NamedExpression::Meta(Meta {
                key: MetaKey::L4proto
            })),
            Expression::String(proto)
        ) if proto == "tcp"
    )
}

fn is_tcp_dport_match(left: &Expression<'_>, right: &Expression<'_>, port: u16) -> bool {
    matches!(
        (left, right),
        (
            Expression::Named(NamedExpression::Payload(Payload::PayloadField(field))),
            Expression::Number(value)
        ) if field.protocol == "tcp" && field.field == "dport" && *value == port as u32
    )
}

fn dnat_matches(nat: &NAT<'_>, target_addr: &Ipv4Addr) -> bool {
    let target = target_addr.to_string();

    let addr_matches = nat
        .addr
        .as_ref()
        .and_then(|expr| match expr {
            Expression::String(s) => Some(s.as_ref() == target),
            _ => None,
        })
        .unwrap_or(false);
    let port_matches = nat
        .port
        .as_ref()
        .and_then(|expr| match expr {
            Expression::Number(val) => Some(*val == SSH_TARGET_PORT as u32),
            Expression::String(s) => s
                .parse::<u32>()
                .ok()
                .map(|val| val == SSH_TARGET_PORT as u32),
            _ => None,
        })
        .unwrap_or(false);
    let family_ok = nat.family.map_or(true, |family| family == NATFamily::IP);
    addr_matches && port_matches && family_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_rule_builder_sets_expected_matches() {
        let net: Ipv4Net = "10.20.30.0/24".parse().unwrap();
        let rule = outbound_masquerade_rule(&net, "eth0");

        assert_eq!(rule.chain, POSTROUTING_CHAIN_NAME);
        assert_eq!(rule.table, NAT_TABLE_NAME);
        assert_eq!(rule.family, NfFamily::IP);
        assert_eq!(rule.expr.len(), 3);

        match (&rule.expr[0], &rule.expr[1], &rule.expr[2]) {
            (Statement::Match(first), Statement::Match(second), Statement::Masquerade(None)) => {
                assert!(matches!(
                    &first.left,
                    Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                        PayloadField { protocol, field }
                    ))) if protocol == "ip" && field == "saddr"
                ));
                assert!(matches!(
                    &first.right,
                    Expression::Named(NamedExpression::Prefix(Prefix { len, .. })) if *len == 24
                ));
                assert!(matches!(
                    &second.left,
                    Expression::Named(NamedExpression::Meta(Meta {
                        key: MetaKey::Oifname
                    }))
                ));
                match &second.right {
                    Expression::String(iface) => assert_eq!(iface.as_ref(), "eth0"),
                    other => panic!("unexpected interface match rhs: {:?}", other),
                }
            }
            _ => panic!("unexpected expression layout for outbound rule"),
        }
    }

    #[test]
    fn outbound_rule_match_helpers_detect_expected_rule() {
        let net: Ipv4Net = "10.200.0.0/24".parse().unwrap();
        let rule = outbound_masquerade_rule(&net, "wan0");

        assert!(
            outbound_rule_matches(&rule, &net, "wan0").expect("outbound matcher should succeed")
        );
        assert!(
            !outbound_rule_matches(&rule, &net, "lan0").expect("outbound matcher should succeed")
        );
    }

    #[test]
    fn inbound_rule_builder_and_match_helpers_cover_kernel_formats() {
        let target = Ipv4Addr::new(198, 51, 100, 7);
        let port = 30123;
        let rule = inbound_dnat_rule(port, &target);
        assert!(
            inbound_rule_matches(&rule, port, &target)
                .expect("builder-generated rule should match")
        );

        let kernel_rule = Rule {
            family: NfFamily::IP,
            table: Cow::Borrowed(NAT_TABLE_NAME),
            chain: Cow::Borrowed(PREROUTING_CHAIN_NAME),
            expr: Cow::Owned(vec![
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(
                        PayloadField {
                            protocol: Cow::Borrowed("tcp"),
                            field: Cow::Borrowed("dport"),
                        },
                    ))),
                    right: Expression::Number(port.into()),
                    op: Operator::EQ,
                }),
                Statement::DNAT(Some(NAT {
                    addr: Some(Expression::String(Cow::Owned(target.to_string()))),
                    family: None,
                    port: Some(Expression::String(Cow::Borrowed("22"))),
                    flags: None,
                })),
            ]),
            handle: Some(42),
            index: None,
            comment: None,
        };

        assert!(
            inbound_rule_matches(&kernel_rule, port, &target)
                .expect("kernel-style DNAT rule should match")
        );
    }

    #[test]
    fn find_rule_handle_returns_matching_handle() {
        let net: Ipv4Net = "10.10.0.0/24".parse().unwrap();
        let mut rule = outbound_masquerade_rule(&net, "eth99");
        rule.handle = Some(555);

        let ruleset = Nftables {
            objects: Cow::Owned(vec![NfObject::ListObject(NfListObject::Rule(rule))]),
        };

        let handle = find_rule_handle(&ruleset, POSTROUTING_CHAIN_NAME, |candidate| {
            outbound_rule_matches(candidate, &net, "eth99")
        })
        .expect("handle lookup should succeed");

        assert_eq!(handle, Some(555));
    }

    #[test]
    fn cidr_expression_produces_expected_prefix() {
        let net: Ipv4Net = "192.168.0.128/25".parse().unwrap();
        let expr = cidr_expression(&net);
        match expr {
            Expression::Named(NamedExpression::Prefix(Prefix { addr, len })) => {
                let rendered = match *addr {
                    Expression::String(ref s) => s.clone(),
                    _ => panic!("unexpected prefix address expression"),
                };
                assert_eq!(rendered, "192.168.0.128");
                assert_eq!(len, 25);
            }
            other => panic!("unexpected expression: {:?}", other),
        }
    }
}

/// Batch add multiple SSH NAT rules using nftables batch API for performance
/// This is much faster than adding rules one-by-one (128 rules in ~50ms vs ~5-10 seconds)
///
/// Note: This function checks for existing rules and only adds rules that don't already exist,
/// preventing duplicate rules even if the database is cleared but nftables rules remain.
pub async fn batch_add_inbound_ssh_nat_rules(
    rules: impl IntoIterator<Item = (u16, Ipv4Addr)>, // (port, target_addr) pairs
) -> anyhow::Result<()> {
    ensure_nat_environment().await?;

    // Get current ruleset to check for existing rules
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing current nftables ruleset")?;

    // Build a batch of rules, skipping any that already exist
    let mut batch = Batch::new();
    let mut added_count = 0;
    let mut skipped_count = 0;

    for (port, target_addr) in rules {
        // Check if this rule already exists
        if find_inbound_rule_handle(&ruleset, port, &target_addr)?.is_some() {
            skipped_count += 1;
            continue;
        }

        let rule = inbound_dnat_rule(port, &target_addr);
        batch.add(NfListObject::Rule(rule));
        added_count += 1;
    }

    // Only apply if we have rules to add
    if added_count > 0 {
        let payload = batch.to_nftables();
        helper::apply_ruleset_async(&payload)
            .await
            .context("batch adding inbound SSH NAT rules via nftables")?;
    }

    if skipped_count > 0 {
        tracing::debug!(
            "Batch add: added {} new rules, skipped {} existing rules",
            added_count,
            skipped_count
        );
    }

    Ok(())
}
