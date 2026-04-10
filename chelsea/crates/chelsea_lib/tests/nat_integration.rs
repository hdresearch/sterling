use std::net::Ipv4Addr;
use std::process::Command;

use anyhow::{Context, Result};
use chelsea_lib::network::linux::nat::{
    add_inbound_ssh_nat_rule, add_outbound_masquerade_nat_rule, check_inbound_ssh_nat_rule_exists,
    check_outbound_masquerade_nat_rule_exists, delete_inbound_ssh_nat_rule,
    delete_outbound_masquerade_nat_rule,
};
use ipnet::Ipv4Net;
use nftables::{
    helper,
    schema::{NfListObject, NfObject},
    types::NfFamily,
};

const TABLE_NAME: &str = "chelsea_nat";
const POSTROUTING_CHAIN: &str = "chelsea_postrouting";
const PREROUTING_CHAIN: &str = "chelsea_prerouting";

struct NftCleanup;

impl Drop for NftCleanup {
    fn drop(&mut self) {
        let _ = Command::new("nft")
            .args(["delete", "table", "ip", TABLE_NAME])
            .status();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn nftables_nat_lifecycle() -> Result<()> {
    if !running_as_root()? {
        eprintln!("skipping nftables_nat_lifecycle: CAP_NET_ADMIN required");
        return Ok(());
    }

    if nft_table_exists().await? {
        eprintln!("skipping nftables_nat_lifecycle: table ip {TABLE_NAME} already exists on host");
        return Ok(());
    }
    let _cleanup = NftCleanup;

    let source_net = Ipv4Net::new(Ipv4Addr::new(10, 123, 45, 0), 24)?;
    let outbound_iface = "lo";
    let ssh_port = 22222;
    let target_addr = Ipv4Addr::new(192, 0, 2, 123);

    // Outbound MASQUERADE lifecycle
    add_outbound_masquerade_nat_rule(&source_net, outbound_iface).await?;
    assert_eq!(
        chain_rule_count(POSTROUTING_CHAIN).await?,
        1,
        "expected exactly one outbound NAT rule"
    );
    assert!(
        check_outbound_masquerade_nat_rule_exists(&source_net, outbound_iface).await?,
        "outbound rule should exist after creation"
    );

    // Idempotent re-apply
    add_outbound_masquerade_nat_rule(&source_net, outbound_iface).await?;
    assert_eq!(
        chain_rule_count(POSTROUTING_CHAIN).await?,
        1,
        "outbound rule should not duplicate on repeated add"
    );

    delete_outbound_masquerade_nat_rule(&source_net, outbound_iface).await?;
    assert!(
        !check_outbound_masquerade_nat_rule_exists(&source_net, outbound_iface).await?,
        "outbound rule should be absent after delete"
    );
    assert_eq!(
        chain_rule_count(POSTROUTING_CHAIN).await?,
        0,
        "chain should be empty after deleting outbound rule"
    );

    // Inbound DNAT lifecycle
    add_inbound_ssh_nat_rule(ssh_port, &target_addr).await?;
    assert!(
        check_inbound_ssh_nat_rule_exists(ssh_port, &target_addr).await?,
        "inbound rule should exist after creation"
    );
    assert_eq!(
        chain_rule_count(PREROUTING_CHAIN).await?,
        1,
        "expected exactly one inbound NAT rule"
    );

    // Idempotent re-apply
    add_inbound_ssh_nat_rule(ssh_port, &target_addr).await?;
    assert_eq!(
        chain_rule_count(PREROUTING_CHAIN).await?,
        1,
        "inbound rule should not duplicate on repeated add"
    );

    delete_inbound_ssh_nat_rule(ssh_port, &target_addr).await?;
    assert!(
        !check_inbound_ssh_nat_rule_exists(ssh_port, &target_addr).await?,
        "inbound rule should be absent after delete"
    );
    assert_eq!(
        chain_rule_count(PREROUTING_CHAIN).await?,
        0,
        "chain should be empty after deleting inbound rule"
    );

    Ok(())
}

fn running_as_root() -> Result<bool> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("checking effective uid via id -u")?;

    if !output.status.success() {
        anyhow::bail!("id -u failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(String::from_utf8(output.stdout)
        .context("id -u output is not valid UTF-8")?
        .trim()
        == "0")
}

async fn nft_table_exists() -> Result<bool> {
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing nftables ruleset")?;
    Ok(ruleset.objects.iter().any(|obj| {
        matches!(
            obj,
            NfObject::ListObject(NfListObject::Table(table))
                if table.family == NfFamily::IP && table.name == TABLE_NAME
        )
    }))
}

async fn chain_rule_count(chain_name: &str) -> Result<usize> {
    let ruleset = helper::get_current_ruleset_async()
        .await
        .context("listing nftables ruleset")?;
    Ok(ruleset
        .objects
        .iter()
        .filter(|obj| {
            matches!(
                obj,
                NfObject::ListObject(NfListObject::Rule(rule))
                    if rule.family == NfFamily::IP
                        && rule.table == TABLE_NAME
                        && rule.chain == chain_name
            )
        })
        .count())
}
