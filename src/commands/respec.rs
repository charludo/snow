use crate::{
    Result, SnowError,
    commands::{
        runners::SnowCommand,
        select_hosts,
        util::{HaConfig, SnowConfig, VmConfigResolved, read_from_repl},
    },
};
use inquire::Confirm;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

// Proxmox VM config as it ends up in the VMA's qemu-server.conf when provisioning
const NIX_SPEC_EXPR: &str = "c: let p = (c.extendModules { modules = [ c.config.image.modules.proxmox ]; }).config.proxmox; in p.qemuConf // p.qemuExtraConf";

// Only used when building the image (additionalSpace, bootSize, diskSize), handled separately (virtio0), or never touched
const IGNORED_KEYS: [&str; 5] = ["additionalSpace", "bootSize", "diskSize", "net0", "virtio0"];

const RED: anstyle::Style = anstyle::AnsiColor::Red.on_default();
const GREEN: anstyle::Style = anstyle::AnsiColor::Green.on_default();

#[derive(Deserialize)]
struct ClusterResource {
    vmid: usize,
    node: String,
}

#[derive(Deserialize)]
struct HaResource {
    sid: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NodeAffinityRule {
    rule: String,
    nodes: String,
    resources: String,
    strict: u8,
    disable: u8,
    affinity: Option<String>,
}

/// The node affinity rule the VM belongs in, with nodes as `node[:priority]`
struct WantedRule {
    affinity: &'static str,
    strict: bool,
    nodes: BTreeSet<String>,
}

impl WantedRule {
    /// Positive nodes are in order of preference: `[ node1 node2 ]` is `node1:2,node2:1`.
    fn new(ha: &HaConfig) -> Option<Self> {
        let (affinity, nodes) = match (&ha.affinity.positive, &ha.affinity.negative) {
            (Some(nodes), _) => (
                "positive",
                nodes
                    .iter()
                    .rev()
                    .zip(1..)
                    .map(|(node, priority)| format!("{node}:{priority}"))
                    .collect(),
            ),
            (None, Some(nodes)) => ("negative", nodes.iter().cloned().collect()),
            (None, None) => return None,
        };
        Some(Self {
            affinity,
            strict: ha.strict,
            nodes,
        })
    }

    fn matches(&self, rule: &NodeAffinityRule) -> bool {
        rule.disable == 0
            && rule.affinity.as_deref().unwrap_or("positive") == self.affinity
            && (rule.strict == 1) == self.strict
            && list(&rule.nodes)
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                == self.nodes
    }
}

/// Human-readable changes, and the commands applying them on the Proxmox host
#[derive(Default)]
struct Plan {
    lines: Vec<(char, String, String, String)>,
    commands: Vec<String>,
}

impl Plan {
    /// A change of `label` from `old` to `new`, either of which may be empty
    fn line(&mut self, marker: char, label: impl Into<String>, old: &str, new: &str) {
        self.lines
            .push((marker, label.into(), old.to_string(), new.to_string()));
    }

    fn log(&self) {
        let width = |column: fn(&(char, String, String, String)) -> &String| {
            self.lines
                .iter()
                .map(|line| column(line).chars().count())
                .max()
                .unwrap_or(0)
        };
        let (label_width, old_width) = (width(|line| &line.1), width(|line| &line.2));
        for (marker, label, old, new) in &self.lines {
            let arrow = if old.is_empty() || new.is_empty() {
                "  "
            } else {
                "->"
            };
            log::info!(
                "  {marker} {label:>label_width$}: {RED}{old:>old_width$}{RED:#} {arrow} {GREEN}{new}{GREEN:#}"
            );
        }
    }
}

/// Entries of a comma-separated Proxmox list
fn list(entries: &str) -> impl Iterator<Item = &str> {
    entries.split(',').filter(|entry| !entry.is_empty())
}

fn to_config_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
        other => other.to_string(),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Parse a Proxmox size string ("16G", "17352M", "512") into bytes.
fn parse_size(size: &str) -> Option<u64> {
    let (number, unit) = size.split_at(
        size.find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(size.len()),
    );
    let shift = match unit {
        "" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        "T" => 40,
        _ => return None,
    };
    Some((number.parse::<f64>().ok()? * (1u64 << shift) as f64) as u64)
}

fn ssh(proxmox_host: &str, command: &str) -> Result<String> {
    SnowCommand::new("ssh".to_string(), vec![proxmox_host, command], false).run_checked()
}

fn pvesh_get<T: DeserializeOwned>(proxmox_host: &str, path: &str) -> Result<T> {
    Ok(serde_json::from_str(&ssh(
        proxmox_host,
        &format!("pvesh get {path} --output-format json"),
    )?)?)
}

fn locate_vm(proxmox_host: &str, id: usize) -> Result<String> {
    pvesh_get::<Vec<ClusterResource>>(proxmox_host, "/cluster/resources --type vm")?
        .into_iter()
        .find(|r| r.vmid == id)
        .map(|r| r.node)
        .ok_or_else(|| {
            SnowError::Proxmox(format!(
                "VM {id} does not exist in the cluster of \"{proxmox_host}\""
            ))
        })
}

/// Start, reboot, ... a VM on whichever node it currently is, e.g. where HA placed or moved it.
pub(crate) fn set_vm_status(proxmox_host: &str, id: usize, action: &str) -> Result<()> {
    let node = locate_vm(proxmox_host, id)?;
    ssh(
        proxmox_host,
        &format!("pvesh create /nodes/{node}/qemu/{id}/status/{action}"),
    )?;
    Ok(())
}

fn plan_config(
    plan: &mut Plan,
    api_path: &str,
    nix: &Map<String, Value>,
    live: &Map<String, Value>,
    resize_disk_to: &str,
) -> Result<()> {
    let mut args = Vec::new();
    for (key, value) in nix {
        if IGNORED_KEYS.contains(&key.as_str()) {
            continue;
        }
        let wanted = to_config_string(value);
        match live.get(key).map(to_config_string) {
            Some(current) if current == wanted => continue,
            Some(current) => plan.line('~', key, &current, &wanted),
            None => plan.line('+', key, "", &wanted),
        }
        args.push(format!("--{key} {}", shell_quote(&wanted)));
    }
    if !args.is_empty() {
        // The digest makes Proxmox reject the change if the config was modified since we read it
        let digest = live.get("digest").map(to_config_string).unwrap_or_default();
        plan.commands.push(format!(
            "pvesh set {api_path}/config --digest {} {}",
            shell_quote(&digest),
            args.join(" ")
        ));
    }

    // Disks can only grow, and are usually a bit larger than configured after provisioning
    let current = live
        .get("virtio0")
        .and_then(Value::as_str)
        .and_then(|disk| disk.split(',').find_map(|part| part.strip_prefix("size=")))
        .unwrap_or("0");
    let (Some(current_bytes), Some(wanted_bytes)) =
        (parse_size(current), parse_size(resize_disk_to))
    else {
        return Err(SnowError::Proxmox(format!(
            "could not compare disk sizes \"{current}\" and \"{resize_disk_to}\""
        )));
    };
    if wanted_bytes > current_bytes {
        plan.line('~', "virtio0 size", current, resize_disk_to);
        plan.commands.push(format!(
            "pvesh set {api_path}/resize --disk virtio0 --size {}",
            shell_quote(resize_disk_to)
        ));
    }

    Ok(())
}

/// Rules are managed in the UI and snow only moves the VM between existing ones.
fn plan_ha(
    plan: &mut Plan,
    id: usize,
    ha: &HaConfig,
    resources: &[HaResource],
    rules: &[NodeAffinityRule],
) -> Result<()> {
    let sid = format!("vm:{id}");

    // Not HA-managed yet: added as ignored.
    let add = !resources.iter().any(|resource| resource.sid == sid);
    if add {
        plan.line('+', format!("HA resource {sid}"), "", "started");
        plan.commands
            .push(format!("ha-manager add {sid} --state ignored"));
    }

    let current = rules
        .iter()
        .find(|rule| list(&rule.resources).any(|r| r == sid));
    let target = match WantedRule::new(ha) {
        None => None,
        Some(wanted) if current.is_some_and(|rule| wanted.matches(rule)) => current,
        Some(wanted) => Some(rules.iter().find(|rule| wanted.matches(rule)).ok_or_else(|| {
            SnowError::Proxmox(format!(
                "there is no enabled {} {} HA node affinity rule with nodes \"{}\" for {sid}; create one first",
                if wanted.strict { "strict" } else { "non-strict" },
                wanted.affinity,
                Vec::from_iter(wanted.nodes.iter().map(String::as_str)).join(",")
            ))
        })?),
    };

    // Proxmox allows a resource in only one node affinity rule, so leave it before joining another
    if current.map(|rule| &rule.rule) != target.map(|rule| &rule.rule) {
        if let Some(rule) = current {
            let remaining: Vec<&str> = list(&rule.resources).filter(|r| *r != sid).collect();
            if remaining.is_empty() {
                return Err(SnowError::Proxmox(format!(
                    "{sid} is the last resource of HA rule \"{}\", which Proxmox cannot keep empty; remove the rule first",
                    rule.rule
                )));
            }
            plan.line('-', "HA rule", &format!("{} (leave)", rule.rule), "");
            plan.commands.push(format!(
                "ha-manager rules set node-affinity {} --resources {}",
                rule.rule,
                remaining.join(",")
            ));
        }
        if let Some(rule) = target {
            let joined: Vec<&str> = list(&rule.resources).chain([sid.as_str()]).collect();
            plan.line('+', "HA rule", "", &format!("{} (join)", rule.rule));
            plan.commands.push(format!(
                "ha-manager rules set node-affinity {} --resources {}",
                rule.rule,
                joined.join(",")
            ));
        }
    }

    if add {
        plan.commands
            .push(format!("ha-manager set {sid} --state started"));
    }

    Ok(())
}

pub(crate) fn respec(
    selectors: &[String],
    exclude: &[String],
    reboot: bool,
    yes: bool,
) -> Result<()> {
    let (hosts, single) = select_hosts(selectors, exclude)?;
    if single {
        let (name, config) = hosts.into_iter().next().expect("one host is selected");
        return respec_host(&name, config, reboot, yes);
    }

    let (vms, others): (Vec<_>, Vec<_>) = hosts.into_iter().partition(|(_, c)| c.vm.is_some());
    if !others.is_empty() {
        log::info!(
            "Skipping hosts without VM settings: {}",
            Vec::from_iter(others.iter().map(|(name, _)| name.as_str())).join(", ")
        );
    }
    if vms.is_empty() {
        return Err(SnowError::SnowConfig("no VMs are selected".to_string()));
    }

    let mut failures = Vec::new();
    for (name, config) in vms {
        if let Err(e) = respec_host(&name, config, reboot, yes) {
            log::error!("Could not respec {name}: {}", e.message());
            failures.push(name);
        }
    }
    match failures.is_empty() {
        true => Ok(()),
        false => Err(SnowError::SnowConfig(format!(
            "could not respec {}",
            failures.join(", ")
        ))),
    }
}

pub(crate) fn respec_host(
    vm_configuration: &str,
    snow_config: SnowConfig,
    reboot: bool,
    yes: bool,
) -> Result<()> {
    let vm_config: VmConfigResolved = snow_config
        .vm
        .ok_or_else(|| {
            SnowError::SnowConfig(format!(
                "VM settings are not configured for host \"{vm_configuration}\""
            ))
        })?
        .try_into()?;
    let host = &vm_config.proxmox_host;

    log::info!("Evaluating the Proxmox spec of {vm_configuration}...");
    let nix: Map<String, Value> = serde_json::from_str(&read_from_repl(
        &format!("nixosConfigurations.{vm_configuration}"),
        vec!["--json", "--apply", NIX_SPEC_EXPR],
    )?)?;

    let node = locate_vm(host, vm_config.id)?;
    let api_path = format!("/nodes/{node}/qemu/{}", vm_config.id);
    let mut plan = Plan::default();
    plan_config(
        &mut plan,
        &api_path,
        &nix,
        &pvesh_get(host, &format!("{api_path}/config"))?,
        &vm_config.resize_disk_to,
    )?;
    if let Some(ha) = &vm_config.ha {
        plan_ha(
            &mut plan,
            vm_config.id,
            ha,
            &pvesh_get::<Vec<_>>(host, "/cluster/ha/resources")?,
            &pvesh_get::<Vec<_>>(host, "/cluster/ha/rules --type node-affinity")?,
        )?;
    }

    if plan.commands.is_empty() {
        log::info!(
            "{vm_configuration} (VM {}) on {node} already matches its nix spec.",
            vm_config.id
        );
    } else {
        log::info!(
            "Changes to {vm_configuration} (VM {}) on {node}:",
            vm_config.id
        );
        plan.log();
        let confirmed = yes
            || Confirm::new("Apply these changes?")
                .with_default(false)
                .prompt()?;
        if !confirmed {
            return Ok(());
        }
        log::info!("Applying the changes...");
        for command in &plan.commands {
            ssh(host, command)?;
        }
    }

    // Also when nothing changed, as earlier changes may be pending
    if reboot {
        log::info!("Rebooting {vm_configuration}...");
        set_vm_status(host, vm_config.id, "reboot")?;
    }

    Ok(())
}

#[cfg(test)]
use crate::commands::util::Affinity;

#[cfg(test)]
fn ha(positive: &[&str], negative: &[&str]) -> HaConfig {
    // No nodes: unset
    let nodes = |nodes: &[&str]| -> Option<Vec<String>> {
        (!nodes.is_empty()).then(|| nodes.iter().map(|node| node.to_string()).collect())
    };
    HaConfig {
        affinity: Affinity {
            positive: nodes(positive),
            negative: nodes(negative),
        },
        strict: true,
    }
}

#[test]
fn test_parse_size() {
    assert_eq!(parse_size("16G"), Some(16 << 30));
    assert_eq!(parse_size("17352M"), Some(17352 << 20));
    assert_eq!(parse_size("512"), Some(512));
    assert_eq!(parse_size("1.5T"), Some(3 << 39));
    assert_eq!(parse_size("G"), None);
    assert_eq!(parse_size("16X"), None);
}

#[test]
fn test_plan_config() {
    let nix: Map<String, Value> = serde_json::from_str(
        r#"{"additionalSpace":"1G","agent":"1","boot":"order=virtio0","cores":2,"memory":4096,
            "kvm":1,"net0":"virtio=00:00:00:00:00:00,bridge=VLAN22,firewall=1",
            "virtio0":"vm_datastore:vm-2202-disk-0"}"#,
    )
    .unwrap();
    let live: Map<String, Value> = serde_json::from_str(
        r#"{"agent":"1","boot":"order=scsi0","cores":2,"memory":"4096","digest":"abc",
            "net0":"virtio=BC:24:11:F7:1A:D9,bridge=VLAN22,firewall=1","vga":"none",
            "unused0":"vm_datastore:vm-2202-disk-1",
            "virtio0":"vm_datastore:vm-2202-disk-0,size=17352M"}"#,
    )
    .unwrap();

    let mut plan = Plan::default();
    plan_config(&mut plan, "/vm", &nix, &live, "32G").unwrap();
    assert_eq!(
        plan.commands,
        vec![
            "pvesh set /vm/config --digest 'abc' --boot 'order=virtio0' --kvm '1'",
            "pvesh set /vm/resize --disk virtio0 --size '32G'",
        ]
    );

    let mut plan = Plan::default();
    plan_config(&mut plan, "/vm", &nix, &live, "16G").unwrap();
    assert_eq!(plan.commands.len(), 1);
}

#[test]
fn test_node_affinity_rules_match_exactly() {
    let rule = |affinity: &str, nodes: &str| NodeAffinityRule {
        affinity: Some(affinity.to_string()),
        nodes: nodes.to_string(),
        strict: 1,
        ..Default::default()
    };
    let wanted =
        |positive: &[&str], negative: &[&str]| WantedRule::new(&ha(positive, negative)).unwrap();

    let px1_px2 = wanted(&["px-1", "px-2"], &[]);
    assert!(px1_px2.matches(&rule("positive", "px-1:2,px-2:1")));
    assert!(!wanted(&["px-2", "px-1"], &[]).matches(&rule("positive", "px-1:2,px-2:1")));
    assert!(!px1_px2.matches(&NodeAffinityRule {
        strict: 0,
        ..rule("positive", "px-1:2,px-2:1")
    }));
    assert!(!px1_px2.matches(&NodeAffinityRule {
        disable: 1,
        ..rule("positive", "px-1:2,px-2:1")
    }));
    assert!(!px1_px2.matches(&rule("positive", "px-1:3,px-2:1")));
    assert!(!px1_px2.matches(&rule("positive", "px-1:2,px-2:1,px-gpu")));
    assert!(wanted(&["px-gpu"], &[]).matches(&rule("positive", "px-gpu:1")));
    assert!(!wanted(&["px-gpu"], &[]).matches(&rule("positive", "px-gpu")));
    assert!(
        wanted(&["px-1", "px-2", "px-gpu"], &[])
            .matches(&rule("positive", "px-1:3,px-2:2,px-gpu:1"))
    );

    let negative = wanted(&[], &["px-1", "px-gpu"]);
    assert!(negative.matches(&rule("negative", "px-1,px-gpu")));
    assert!(negative.matches(&rule("negative", "px-gpu,px-1")));
    assert!(!negative.matches(&rule("negative", "px-gpu")));
    assert!(!negative.matches(&rule("positive", "px-1,px-gpu")));
}

#[test]
fn test_plan_ha() {
    let resources: Vec<HaResource> = serde_json::from_str(
        r#"[{"sid":"vm:2222","state":"started","type":"vm"},
            {"sid":"vm:2209","state":"started","type":"vm"},
            {"sid":"vm:2213","state":"started","type":"vm"},
            {"sid":"vm:2005","state":"started","type":"vm"}]"#,
    )
    .unwrap();
    let rules: Vec<NodeAffinityRule> = serde_json::from_str(
        r#"[{"affinity":"positive","nodes":"px-gpu:1","resources":"vm:2101","rule":"gpu","strict":1},
            {"affinity":"positive","nodes":"px-1:1,px-2:2","resources":"vm:2209,vm:2222","rule":"home-2","strict":1},
            {"affinity":"positive","nodes":"px-1:2,px-2:1","resources":"vm:3002","rule":"home-1","strict":1},
            {"affinity":"negative","nodes":"px-gpu","resources":"vm:2005","rule":"no-gpu","strict":1}]"#,
    )
    .unwrap();
    let commands = |id: usize, ha: HaConfig| -> Result<Vec<String>> {
        let mut plan = Plan::default();
        plan_ha(&mut plan, id, &ha, &resources, &rules)?;
        Ok(plan.commands)
    };

    // Matching rule: nothing to do
    assert!(
        commands(2209, ha(&["px-2", "px-1"], &[]))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        commands(2101, ha(&["px-gpu"], &[])).unwrap(),
        vec![
            "ha-manager add vm:2101 --state ignored",
            "ha-manager set vm:2101 --state started",
        ]
    );

    // Moving homes: leave the old rule, join the new one
    assert_eq!(
        commands(2222, ha(&["px-1", "px-2"], &[])).unwrap(),
        vec![
            "ha-manager rules set node-affinity home-2 --resources vm:2209",
            "ha-manager rules set node-affinity home-1 --resources vm:3002,vm:2222",
        ]
    );

    // Not yet HA-managed: added and joined
    assert_eq!(
        commands(3033, ha(&["px-1", "px-2"], &[])).unwrap(),
        vec![
            "ha-manager add vm:3033 --state ignored",
            "ha-manager rules set node-affinity home-1 --resources vm:3002,vm:3033",
            "ha-manager set vm:3033 --state started",
        ]
    );

    // Rules are never created, nor emptied
    assert!(commands(2213, ha(&["px-1"], &[])).is_err());
    assert!(commands(2101, ha(&["px-2", "px-1"], &[])).is_err());

    // Negative rules, distinct from positive ones with the same nodes
    assert!(commands(2005, ha(&[], &["px-gpu"])).unwrap().is_empty());
    assert_eq!(
        commands(2213, ha(&[], &["px-gpu"])).unwrap(),
        vec!["ha-manager rules set node-affinity no-gpu --resources vm:2005,vm:2213"]
    );
    assert!(commands(2101, ha(&[], &["px-gpu"])).is_err());
    assert!(commands(2213, ha(&[], &["px-1", "px-gpu"])).is_err());

    // Neither: HA-managed in no rule
    assert!(commands(2213, ha(&[], &[])).unwrap().is_empty());
    assert_eq!(
        commands(2222, ha(&[], &[])).unwrap(),
        vec!["ha-manager rules set node-affinity home-2 --resources vm:2209"]
    );
    assert_eq!(
        commands(3033, ha(&[], &[])).unwrap(),
        vec![
            "ha-manager add vm:3033 --state ignored",
            "ha-manager set vm:3033 --state started",
        ]
    );
    assert!(commands(2101, ha(&[], &[])).is_err());
}
