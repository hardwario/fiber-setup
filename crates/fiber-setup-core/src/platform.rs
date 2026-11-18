//! The FIBER platform a new FIBER installs on its first start: its components, and the seed that
//! hands the choice to it.
//!
//! The components come from `image/components.yaml`, a copy of `provision/components.yaml` of
//! the release FIBER Setup names ([`catalog::FIBER_PLATFORM`]), so the picker offers what that
//! release has. The seed is the file its installer reads once, on the first start (`fiber
//! platform seed`; the platform's `docs/provisioning.md`, "The seed").

use crate::catalog::{self, PlatformRelease};
use crate::firstboot::{Settings, q};
use anyhow::{Context, Result, bail};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;

/// The copy of the release's `provision/components.yaml`.
const COMPONENTS_YAML: &str = include_str!("../../../image/components.yaml");
/// The schema of `components.yaml` and of the seed that this FIBER Setup knows.
const SCHEMA: u32 = 1;
/// The seed's name on the boot partition, beside the first-start script.
pub const SEED_NAME: &str = "fiber-seed.yaml";

/// One component the platform can install.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Component {
    pub id: String,
    /// Its name and what it brings, in English, as the release has them. The app shows its own
    /// translation where it has one.
    pub title: String,
    pub about: String,
    /// Components that are installed with it.
    pub requires: Vec<String>,
    /// Components that go well with it; nothing turns them on.
    pub suggests: Vec<String>,
    /// Where it listens.
    pub ports: Vec<Port>,
    /// Download and installed size in MB, once measured on a FIBER.
    pub size: Option<Size>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub port: u16,
    pub listen: Listen,
    /// The setting that opens it, as in `mqtt.lan`.
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Listen {
    /// The FIBER itself only.
    Loopback,
    Network,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub download: Option<u32>,
    pub installed: Option<u32>,
}

/// The card in the mini-PCIe slot (`hardware.minipcie`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MiniPcie {
    #[default]
    None,
    Lte,
    Rak5146,
}

impl MiniPcie {
    /// As the platform's files write it.
    fn as_str(self) -> &'static str {
        match self {
            MiniPcie::None => "none",
            MiniPcie::Lte => "lte",
            MiniPcie::Rak5146 => "rak5146",
        }
    }
}

/// What is ticked to begin with: the agent and Node-RED, and what they require. A FIBER then
/// shows its board and its thermometers on MQTT and in a flow, and nothing listens on the network
/// but Node-RED and SSH.
pub const DEFAULT_CHOICE: [&str; 2] = ["agent", "node-red"];

/// The release FIBER Setup installs and what it offers: for the picker.
#[derive(Debug, Clone, Serialize)]
pub struct Offer {
    pub release: PlatformRelease,
    pub components: Vec<Component>,
    /// What is ticked to begin with, requirements included.
    pub default: Vec<String>,
}

/// What a new FIBER can install.
pub fn offer() -> Result<Offer> {
    let components = components()?;
    let default = with_requirements(&DEFAULT_CHOICE.map(String::from), &components)?;
    Ok(Offer { release: catalog::FIBER_PLATFORM, components, default })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    components: Ordered<Fields>,
}

// Every field is read, the ones FIBER Setup does not use too: a release whose file has a field
// this FIBER Setup does not know fails its tests when the copy is moved to it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    title: String,
    about: String,
    #[allow(dead_code)]
    roles: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    suggests: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    services: Vec<String>,
    #[serde(default)]
    ports: Vec<Port>,
    #[serde(default)]
    size: Option<Size>,
}

/// A YAML map, read with its entries in the order of the file: the platform installs its
/// components in that order, and the picker lists them so.
struct Ordered<V>(Vec<(String, V)>);

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Ordered<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedVisitor<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for OrderedVisitor<V> {
            type Value = Ordered<V>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Ordered(entries))
            }
        }
        deserializer.deserialize_map(OrderedVisitor(PhantomData))
    }
}

/// The components of the release, in its order.
pub fn components() -> Result<Vec<Component>> {
    parse(COMPONENTS_YAML)
}

fn parse(text: &str) -> Result<Vec<Component>> {
    let file: File = serde_saphyr::from_str(text).context("reading the copy of components.yaml")?;
    if file.schema != SCHEMA {
        bail!("components.yaml is written for schema {}; this FIBER Setup reads schema {SCHEMA}", file.schema);
    }
    let components: Vec<Component> = file
        .components
        .0
        .into_iter()
        .map(|(id, f)| Component {
            id,
            title: f.title,
            about: f.about,
            requires: f.requires,
            suggests: f.suggests,
            ports: f.ports,
            size: f.size,
        })
        .collect();
    for c in &components {
        for other in c.requires.iter().chain(&c.suggests) {
            if !components.iter().any(|k| &k.id == other) {
                bail!("components.yaml: `{}` names `{other}`, which is not there", c.id);
            }
        }
    }
    Ok(components)
}

/// The chosen components with everything they require, in the order of the release. An id the
/// release does not have is refused.
pub fn with_requirements(chosen: &[String], all: &[Component]) -> Result<Vec<String>> {
    for id in chosen {
        if !all.iter().any(|c| &c.id == id) {
            bail!("the FIBER platform {} has no component `{id}`", catalog::FIBER_PLATFORM.version);
        }
    }
    let mut wanted: Vec<&str> = chosen.iter().map(String::as_str).collect();
    // Requirements of requirements: until nothing is added. The list is a handful long.
    loop {
        let more: Vec<&str> = all
            .iter()
            .filter(|c| wanted.contains(&c.id.as_str()))
            .flat_map(|c| c.requires.iter().map(String::as_str))
            .filter(|id| !wanted.contains(id))
            .collect();
        if more.is_empty() {
            break;
        }
        wanted.extend(more);
    }
    Ok(all.iter().filter(|c| wanted.contains(&c.id.as_str())).map(|c| c.id.clone()).collect())
}

/// `fiber-seed.yaml`: the components (with what they require), the settings and the login for
/// the services (the platform's D16). `None` without components: then nothing is installed.
///
/// The installer reads it once, then overwrites it with zeros and deletes it, as it holds the
/// password. The platform's `seed.rs` has a test with the same bytes as the test here.
pub fn seed(s: &Settings, release: &PlatformRelease) -> Result<Option<String>> {
    if s.components.is_empty() {
        return Ok(None);
    }
    let ids = with_requirements(&s.components, &components()?)?;
    let components: Vec<String> = ids.iter().map(|id| format!("{id}: true")).collect();
    Ok(Some(format!(
        "# Written by FIBER Setup {} for the FIBER platform {}. The installer reads it on the\n\
         # first start, then overwrites it with zeros and deletes it.\n\
         schema: {SCHEMA}\n\
         components: {{{}}}\n\
         settings: {{mqtt: {{lan: {}}}, hardware: {{minipcie: {}}}}}\n\
         login: {{user: {}, password: {}}}\n",
        crate::VERSION,
        release.version,
        components.join(", "),
        s.mqtt_lan,
        s.minipcie.as_str(),
        q(&s.username),
        q(&s.password),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_copy_is_read() {
        let all = components().unwrap();
        let names: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(names, ["base", "mosquitto", "agent", "node-red", "chirpstack"]);
        let red = &all[3];
        assert_eq!(red.title, "Node-RED");
        assert_eq!(red.requires, ["mosquitto"]);
        assert_eq!(red.suggests, ["agent"]);
        assert_eq!(red.ports, [Port { port: 1880, listen: Listen::Network, when: None }]);
    }

    #[test]
    fn requirements_come_along_in_the_release_order() {
        let all = components().unwrap();
        assert_eq!(with_requirements(&ids(&["agent"]), &all).unwrap(), ["base", "mosquitto", "agent"]);
        assert_eq!(
            with_requirements(&ids(&["chirpstack", "node-red"]), &all).unwrap(),
            ["mosquitto", "node-red", "chirpstack"]
        );
        assert!(with_requirements(&[], &all).unwrap().is_empty());
        let e = with_requirements(&ids(&["nodered"]), &all).unwrap_err().to_string();
        assert!(e.contains("no component `nodered`"), "{e}");
    }

    #[test]
    fn a_file_that_names_what_is_not_there_is_refused() {
        let e = parse("schema: 1\ncomponents:\n  a: {title: A, about: x, roles: [a], requires: [b]}\n").unwrap_err();
        assert!(e.to_string().contains("`a` names `b`"), "{e}");
        let e = parse("schema: 2\ncomponents: {}\n").unwrap_err();
        assert!(e.to_string().contains("schema 2"), "{e}");
        assert!(parse("schema: 1\ncomponents:\n  a: {title: A, about: x, roles: [a], colour: red}\n").is_err());
    }

    /// The same bytes as the platform's test (`agent/src/platform/seed.rs`,
    /// `a_seed_as_fiber_setup_writes_it`), but for the version of FIBER Setup in its first line:
    /// the platform reads this seed as it is meant.
    #[test]
    fn the_seed_the_platform_reads() {
        let mut s = crate::firstboot::tests::sample();
        s.username = "fiber".into();
        s.password = "a\"b\\c: #d\te\u{7}\u{7f}\u{85}\u{2028}\u{feff}\u{fffe} Dílna 📶".into();
        s.components = ids(&["chirpstack", "agent", "node-red"]);
        s.mqtt_lan = true;
        s.minipcie = MiniPcie::Rak5146;
        let release = PlatformRelease { version: "0.1.0", installer_sha256: "" };
        let expected = r#"# Written by FIBER Setup 1.3.0 for the FIBER platform 0.1.0. The installer reads it on the
# first start, then overwrites it with zeros and deletes it.
schema: 1
components: {base: true, mosquitto: true, agent: true, node-red: true, chirpstack: true}
settings: {mqtt: {lan: true}, hardware: {minipcie: rak5146}}
login: {user: "fiber", password: "a\"b\\c: #d\te\u0007\u007f\u0085\u2028\ufeff\ufffe Dílna 📶"}
"#
        .replace("FIBER Setup 1.3.0", &format!("FIBER Setup {}", crate::VERSION));
        assert_eq!(seed(&s, &release).unwrap().unwrap(), expected);
    }

    #[test]
    fn the_default_choice() {
        assert_eq!(offer().unwrap().default, ["base", "mosquitto", "agent", "node-red"]);
    }

    #[test]
    fn no_components_no_seed() {
        let mut s = crate::firstboot::tests::sample();
        s.components.clear();
        assert_eq!(seed(&s, &catalog::FIBER_PLATFORM).unwrap(), None);
    }
}
